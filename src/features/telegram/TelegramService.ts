// @ts-nocheck
import { TelegramClient, Api } from 'telegram';
import { StringSession } from 'telegram/sessions';
import { NewMessage } from 'telegram/events';
import { convertFileSrc, invokeCommand as invoke } from '../../shared/platform/tauri';
import bigInt from 'big-integer';

import { downloadUrlInBrowser } from '../../shared/platform/browserDownload';
import { openDialog as open, saveDialog as save } from '../../shared/platform/dialog';
import { getDownloadDir, joinPath as joinSystemPath } from '../../shared/platform/files';
import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { canUseServiceWorker } from '../../shared/platform/serviceWorker';
import { appStorage } from '../../shared/storage/appStorage';
import { downloadService } from '../downloader/DownloadService';
import { mediaCache } from './MediaCacheService';
import {
  basename,
  formatTelegramMessage,
  getMessageDownloadFilename,
  getMessageGroupedId,
  getMessageText,
  isDownloadableMedia,
  isLikelyAlbumSeparator,
  joinPath,
  messageCacheKey,
  nextFrame,
  sanitizeForAlbumFolderName,
  sanitizeForFilename,
  sanitizeForFolderName,
  toNumberValue,
  toUint8Array,
} from './TelegramMessageUtils';
import {
  readTwitterFakeChats,
  TWITTER_FAKE_CHAT_EVENT,
  twitterFakeDialogs,
  writeTwitterFakeChats,
} from './TwitterFakeChatStore';
import { telegramApiCredentials } from './TelegramConfig';
import { TelegramTdlibBridge } from './TelegramTdlibBridge';
import { TelegramTwitterFakeBridge } from './TelegramTwitterFakeBridge';

class TelegramService {
  private client: TelegramClient | null = null;
  private connectPromise: Promise<void> | null = null;
  private session: StringSession;
  private mediaProgressCallbacks: Set<Function> = new Set();
  private downloadProgressCallbacks: Set<Function> = new Set();
  private saveMultipleProgressCallbacks: Set<Function> = new Set();
  private sendProgressCallbacks: Set<Function> = new Set();
  private newMessageCallbacks: Set<Function> = new Set();
  private newMessageHandlerRegistered = false;
  private activeDownloadAborted = false;
  private saveMultipleAborted = false;
  private serviceWorkerMessageHandler: (event: MessageEvent) => void;
  private tdlibBridge = new TelegramTdlibBridge(() => telegramApiCredentials);
  private twitterFakeBridge = new TelegramTwitterFakeBridge(data => {
    this.mediaProgressCallbacks.forEach(cb => cb(data));
  });
  public skipLogin: boolean = false;

  constructor() {
    const savedSession = appStorage.get('telegram_session') || '';
    this.session = new StringSession(savedSession);
    
    if (!savedSession) {
      // Define DC 4 (Américas) como padrão inicial usando DOMÍNIO em vez de IP
      // Isso é obrigatório no Tauri (Linux/WebKit) para não dar erro de certificado SSL
      this.session.setDC(4, "vesta.web.telegram.org", 443);
    }

    this.serviceWorkerMessageHandler = this.handleServiceWorkerMessage.bind(this);

    if (canUseServiceWorker()) {
      const previousHandler = (window as any).__telegramServiceSwHandler;
      if (previousHandler) {
        navigator.serviceWorker.removeEventListener('message', previousHandler);
      }

      (window as any).__telegramServiceSwHandler = this.serviceWorkerMessageHandler;
      navigator.serviceWorker.addEventListener('message', this.serviceWorkerMessageHandler);
      if (typeof navigator.serviceWorker.startMessages === 'function') {
        navigator.serviceWorker.startMessages();
      }
    }
  }

  resetSession() {
    appStorage.remove('telegram_session');
    if (this.client) {
      Promise.resolve(this.client.disconnect()).catch(error => {
        console.warn('[TelegramService] Failed to disconnect old client while resetting session:', error);
      });
    }
    this.client = null;
    this.connectPromise = null;
    this.newMessageHandlerRegistered = false;
    this.session = new StringSession('');
    this.session.setDC(4, "vesta.web.telegram.org", 443);
  }

  private streamIterators = new Map<string, { iter: AsyncIterator<any>, message: any, fileSize: number, mimeType: string, offset: number }>();

  private isTwitterFakeChat(chatId: any) {
    return this.twitterFakeBridge.isFakeChat(chatId);
  }

  private getTwitterFakeChat(chatId: any) {
    return this.twitterFakeBridge.getFakeChat(chatId);
  }

  private findTwitterFakeMessage(chatId: any, messageId: any) {
    return this.twitterFakeBridge.findFakeMessage(chatId, messageId);
  }

  private async downloadTwitterFakeMedia(chatId: any, messageId: any, message: any) {
    return this.twitterFakeBridge.downloadFakeMedia(chatId, messageId, message);
  }

  private async handleServiceWorkerMessage(event: MessageEvent) {
    const { type, chatId, messageId, requestId, offset, streamId } = event.data;
    const streamKey = `${chatId}_${messageId}_${streamId}`; // Unique per video stream instance

    console.log(`[TelegramService] Received message from SW: type=${type}, streamId=${streamId}, offset=${offset}`);

    if (type === 'init_stream') {
      try {
        // Handle Twitter fake chats stream initialization from local cache
        const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
        if (fakeMessage) {
          console.log(`[TelegramService] Initializing stream for Twitter fake message: ${chatId}/${messageId}`);
          const cacheKey = `twitter_fake_${chatId}_${messageId}_full`;
          const buffer = await mediaCache.getMediaBuffer(cacheKey);
          if (!buffer) {
            throw new Error("Twitter media buffer not found in cache");
          }
          
          const fileSize = buffer.byteLength;
          const mimeType = 'video/mp4';
          const startOffset = offset || 0;
          
          console.log(`[TelegramService] Creating buffer iterator for Twitter video: streamKey=${streamKey}, offset=${startOffset}, totalSize=${fileSize}`);
          
          let currentOffset = startOffset;
          const chunkSize = 512 * 1024; // 512KB chunks
          const iter = {
            async next() {
              if (currentOffset >= fileSize) {
                return { done: true, value: undefined };
              }
              const end = Math.min(currentOffset + chunkSize, fileSize);
              const chunk = buffer.slice(currentOffset, end);
              currentOffset = end;
              return { done: false, value: new Uint8Array(chunk) };
            }
          };

          this.streamIterators.set(streamKey, {
            iter,
            message: fakeMessage,
            fileSize,
            mimeType,
            offset: startOffset
          });

          const response = { type: 'chunk_response', requestId, totalSize: fileSize, mimeType };
          console.log(`[TelegramService] Sending Twitter init_stream success: size=${fileSize}, mime=${mimeType}`);
          navigator.serviceWorker.controller?.postMessage(response);
          return;
        }

        if (!this.client) throw new Error("Client not connected");
        console.log(`[TelegramService] Fetching message ${messageId} for stream...`);
        const entity = await this.client.getEntity(chatId);
        const messages = await this.client.getMessages(entity, { ids: [Number(messageId)] });
        if (!messages || messages.length === 0 || !messages[0].media) {
          throw new Error("Message or media not found");
        }
        
        const message = messages[0];
        
        let rawSize = message.media?.document?.size || message.media?.photo?.sizes?.slice(-1)[0]?.size || 0;
        let fileSize = typeof rawSize === 'number' ? rawSize : (rawSize.toJSNumber ? rawSize.toJSNumber() : Number(rawSize));
        
        const mimeType = message.media?.document?.mimeType || 'video/mp4';
        const startOffset = offset || 0;

        console.log(`[TelegramService] Creating iterDownload for streamKey=${streamKey}, offset=${startOffset}, totalSize=${fileSize}`);
        const iter = this.client.iterDownload({
          file: message.media,
          requestSize: 512 * 1024, // 512KB chunks
          offset: bigInt(startOffset),
        });

        this.streamIterators.set(streamKey, { 
          iter: iter[Symbol.asyncIterator](), 
          message, 
          fileSize, 
          mimeType,
          offset: startOffset
        });

        const response = { type: 'chunk_response', requestId, totalSize: fileSize, mimeType };
        console.log(`[TelegramService] Sending init_stream success response: size=${fileSize}, mime=${mimeType}`);
        if (navigator.serviceWorker.controller) {
          navigator.serviceWorker.controller.postMessage(response);
        } else {
          console.warn("[TelegramService] No service worker controller found to send init_stream success response");
        }
      } catch (err: any) {
        console.error(`[TelegramService] Error in init_stream:`, err);
        navigator.serviceWorker.controller?.postMessage({ type: 'chunk_response', requestId, error: err.message });
      }
    } 
    else if (type === 'get_chunk') {
      try {
        const streamData = this.streamIterators.get(streamKey);
        if (!streamData) throw new Error("Stream not initialized");

        console.log(`[TelegramService] Fetching next chunk for streamKey=${streamKey}...`);
        const next = await streamData.iter.next();
        if (next.done) {
          console.log(`[TelegramService] Iterator done for streamKey=${streamKey}`);
          this.streamIterators.delete(streamKey);
          navigator.serviceWorker.controller?.postMessage({ type: 'chunk_response', requestId, done: true });
        } else {
          const chunk = next.value;
          console.log(`[TelegramService] Iterator yielded chunk of size=${chunk.length} for streamKey=${streamKey}`);
          // Ensure we send a clean ArrayBuffer that is exactly the size of the chunk
          const chunkArray = new Uint8Array(chunk);
          const chunkBuffer = chunkArray.buffer.slice(chunkArray.byteOffset, chunkArray.byteOffset + chunkArray.byteLength);

          const response = { type: 'chunk_response', requestId, chunk: chunkBuffer };
          if (navigator.serviceWorker.controller) {
            navigator.serviceWorker.controller.postMessage(response, [chunkBuffer]);
          } else {
            console.warn("[TelegramService] No service worker controller found to send chunk response");
          }
        }
      } catch (err: any) {
        console.error(`[TelegramService] Error in get_chunk for streamKey=${streamKey}:`, err);
        navigator.serviceWorker.controller?.postMessage({ type: 'chunk_response', requestId, error: err.message });
      }
    }
    else if (type === 'cancel_stream') {
      console.log(`[TelegramService] Cancelling stream for streamKey=${streamKey}`);
      this.streamIterators.delete(streamKey);
    }
  }

  async connect() {
    if (this.client && this.client.connected) {
      this.setupNewMessageHandler();
      return;
    }
    if (this.connectPromise) return this.connectPromise;
    
    if (!telegramApiCredentials.apiId || !telegramApiCredentials.apiHash) {
      console.warn("API_ID and API_HASH are required in .env");
      throw new Error("Credenciais do Telegram ausentes. Crie um arquivo .env com VITE_API_ID e VITE_API_HASH.");
    }

    this.connectPromise = (async () => {
      if (!this.client) {
        this.client = new TelegramClient(this.session, telegramApiCredentials.apiId, telegramApiCredentials.apiHash, {
          connectionRetries: 5,
          useWSS: true,
        });
      }

      await this.client.connect();
      this.setupNewMessageHandler();
    })();

    try {
      await this.connectPromise;
    } finally {
      this.connectPromise = null;
    }
  }

  private setupNewMessageHandler() {
    if (!this.client || this.newMessageHandlerRegistered) return;
    this.newMessageHandlerRegistered = true;

    this.client.addEventHandler(async (event: any) => {
      try {
        const rawMessage = event.message;
        if (!rawMessage || rawMessage.action || rawMessage.className === 'MessageService') return;

        const chatId = event.chatId?.toString?.() || rawMessage.chatId?.toString?.();
        if (!chatId) return;

        const message = formatTelegramMessage(rawMessage);
        const topicId = message.topicId || undefined;
        await this.mergeMessageIntoCache(chatId, message, topicId);
        this.newMessageCallbacks.forEach(cb => cb({ chatId, topicId, message }));
      } catch (error) {
        console.warn('[TelegramService] Failed to handle new message update:', error);
      }
    }, new NewMessage({}));
  }

  private async mergeMessageIntoCache(chatId: string, message: any, topicId?: number) {
    const keys = [messageCacheKey(chatId, topicId)];
    if (topicId) keys.push(messageCacheKey(chatId));

    for (const key of keys) {
      const cached = await mediaCache.getMessages(key);
      const byId = new Map<number, any>();
      cached.forEach(item => byId.set(Number(item.id), item));
      byId.set(Number(message.id), message);
      const merged = Array.from(byId.values()).sort((a, b) => Number(a.id) - Number(b.id));
      await mediaCache.saveMessages(key, merged);
    }
  }

  async checkAuth() {
    try {
      await this.connect();
      if (!this.client) return { isAuthorized: false };
      const isAuth = await this.client.checkAuthorization();
      return { isAuthorized: isAuth };
    } catch (e) {
      console.error("Auth check failed:", e);
      return { isAuthorized: false };
    }
  }

  async tdlibInit() {
    return this.tdlibBridge.init();
  }

  async tdlibStatus() {
    return this.tdlibBridge.status();
  }

  async tdlibSetPhone(phoneNumber: string) {
    return this.tdlibBridge.setPhone(phoneNumber);
  }

  async tdlibCheckCode(code: string) {
    return this.tdlibBridge.checkCode(code);
  }

  async tdlibCheckPassword(password: string) {
    return this.tdlibBridge.checkPassword(password);
  }

  async tdlibGetMe() {
    return this.tdlibBridge.getMe();
  }

  async tdlibDownloadMessageMedia({ chatId, messageId, folderPath }: any) {
    return this.tdlibBridge.downloadMessageMedia({ chatId, messageId, folderPath });
  }

  async tdlibStartMassDownload({ chatId, folderPath, topicId = null, splitByUser = false }: any) {
    return this.tdlibBridge.startMassDownload({ chatId, folderPath, topicId, splitByUser });
  }

  async tdlibStopDownload() {
    return this.tdlibBridge.stopDownload();
  }

  onTdlibAuthState(cb: (state: string) => void) {
    return this.tdlibBridge.onAuthState(cb);
  }

  onTdlibDownloadProgress(cb: (data: any) => void) {
    return this.tdlibBridge.onDownloadProgress(cb);
  }

  async sendCode(phoneNumber: string) {
    try {
      await this.connect();
      
      let cleanPhone = phoneNumber.replace(/[^0-9+]/g, '');
      if (!cleanPhone.startsWith('+')) {
        cleanPhone = '+' + cleanPhone;
      }
      
      const result: any = await this.client!.sendCode(
        { apiId: telegramApiCredentials.apiId, apiHash: telegramApiCredentials.apiHash },
        cleanPhone
      );

      return { success: true, phoneCodeHash: result.phoneCodeHash };
    } catch (error: any) {
      console.error("Error sending code:", error);
      return { success: false, error: error?.message || String(error) };
    }
  }

  async signIn(phoneNumber: string, phoneCodeHash: string, phoneCode: string) {
    try {
      let cleanPhone = phoneNumber.replace(/[^0-9+]/g, '');
      if (!cleanPhone.startsWith('+')) {
        cleanPhone = '+' + cleanPhone;
      }

      await this.client!.invoke(new Api.auth.SignIn({
        phoneNumber: cleanPhone,
        phoneCodeHash,
        phoneCode,
      }));
      
      const savedSessionString = this.client!.session.save() as unknown as string;
      appStorage.set('telegram_session', savedSessionString);
      
      return { success: true };
    } catch (error: any) {
      return { success: false, error: error.message };
    }
  }

  async getDialogs() {
    const fakeDialogs = twitterFakeDialogs();
    if (this.skipLogin) return { success: true, dialogs: fakeDialogs };
    try {
      if (!this.client || !this.client.connected) {
        const savedSession = appStorage.get('telegram_session');
        if (!savedSession) return { success: true, dialogs: fakeDialogs };
        await this.connect();
      }

      const isAuthorized = await this.client.checkAuthorization();
      if (!isAuthorized) return { success: true, dialogs: fakeDialogs };

      const dialogs = await this.client.getDialogs();
      return {
        success: true,
        dialogs: [
          ...fakeDialogs,
          ...dialogs.map(d => ({
          id: d.id?.toString(),
          title: d.title,
          date: d.date,
          unreadCount: d.unreadCount,
          isGroup: d.isGroup,
          isChannel: d.isChannel,
          hasTopics: (d.entity as any)?.forum || false,
          lastMessageText: d.message?.message || (d.message?.media ? '' : 'Sem mensagem'),
          lastMessageDate: d.message?.date,
          lastMessageHasMedia: !!d.message?.media,
          lastMessageIsVideo: !!(d.message?.media?.document?.mimeType && typeof d.message.media.document.mimeType === 'string' && d.message.media.document.mimeType.includes('video')),
          lastMessageIsPhoto: !!(d.message?.media?.photo || d.message?.media?.className === 'MessageMediaPhoto')
          }))
        ]
      };
    } catch (e: any) {
      console.error("Failed to get dialogs:", e);
      if (fakeDialogs.length > 0) return { success: true, dialogs: fakeDialogs };
      return { success: false, error: e.message || "Failed to fetch dialogs" };
    }
  }

  async getCachedMessages({ chatId, limit = 50, topicId = undefined }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) return this.getMessages({ chatId, limit, topicId });

    const cached = await mediaCache.getMessages(messageCacheKey(chatId, topicId));
    const ordered = [...cached].sort((a, b) => Number(a.id) - Number(b.id));
    const page = ordered.slice(-limit);
    return {
      success: true,
      messages: page,
      hasMore: ordered.length > page.length,
      oldestMessageId: page[0]?.id ?? null,
      fromCache: true,
    };
  }

  async getMessages({ chatId, limit = 50, offsetId = 0, topicId = undefined, refresh = false }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) {
      const allMessages = fakeChat.messages.map(message => ({
        id: message.id,
        message: message.text,
        date: message.date,
        out: false,
        senderId: fakeChat.id,
        senderName: fakeChat.title,
        replyToMsgId: null,
        media: Boolean(message.url),
        text: message.text,
        hasMedia: Boolean(message.url),
        isPhoto: Boolean(message.url) && !message.isVideo,
        isVideo: message.isVideo,
        videoDuration: null,
        mediaSize: message.mediaSize ?? null,
        isDeleted: false
      }));
      const filtered = offsetId
        ? allMessages.filter(message => message.id < Number(offsetId))
        : allMessages;
      const page = filtered.slice(-limit);
      return {
        success: true,
        messages: page,
        hasMore: filtered.length > page.length,
        oldestMessageId: page[0]?.id ?? null,
      };
    }

    if (!this.client) return { messages: [] };
    try {
      const entity = await this.client.getEntity(chatId);
      let allValidMessages: any[] = [];
      let currentOffsetId = offsetId || undefined;
      let lastMessageId = null;
      let hasMore = true;

      const cacheKey = messageCacheKey(chatId, topicId);
      const isFirstPage = !offsetId;

      // 1. Fetch Local Cached Messages. Keep this separate per topic so stale
      // topic history cannot be mistaken for the main chat timeline.
      const localMessages = await mediaCache.getMessages(cacheKey);

      while (allValidMessages.length < limit) {
        const batch = await this.client.getMessages(entity, {
          limit,
          offsetId: currentOffsetId,
          replyTo: topicId
        });
        const messagesBatch = Array.isArray(batch) ? batch : batch ? [batch] : [];
        if (messagesBatch.length === 0) {
          hasMore = false;
          break;
        }

        lastMessageId = messagesBatch[messagesBatch.length - 1].id;
        currentOffsetId = lastMessageId;

        const valid = messagesBatch.filter((m: any) => !m.action && m.className !== 'MessageService');
        allValidMessages = allValidMessages.concat(valid);

        if (messagesBatch.length < limit) {
          hasMore = false;
          break;
        }
      }

      // 2. Map formatted messages
      const formattedMessages = allValidMessages.map(formatTelegramMessage);

      // 3. Anti-Delete Logic
      // Find the ID range of the fetched messages
      if (formattedMessages.length > 0) {
        const maxId = Math.max(...formattedMessages.map(m => m.id));
        const minId = Math.min(...formattedMessages.map(m => m.id));

        // Find messages in local cache that belong to this ID range but are NOT in the fetched messages
        const fetchedIds = new Set(formattedMessages.map(m => m.id));
        const deletedMessages = localMessages.filter(localMsg => 
          localMsg.id >= minId && localMsg.id <= maxId && !fetchedIds.has(localMsg.id)
        ).map(msg => ({ ...msg, isDeleted: true }));

        // Merge and sort chronologically for the virtualized timeline.
        formattedMessages.push(...deletedMessages);
        formattedMessages.sort((a, b) => Number(a.id) - Number(b.id));
      }

      // 4. Save merged result to local cache
      // We merge with all existing local messages outside this range
      const mergedById = new Map<number, any>();
      localMessages.forEach(message => mergedById.set(Number(message.id), message));
      formattedMessages.forEach(fm => {
        mergedById.set(Number(fm.id), fm);
      });
      const mergedLocal = Array.from(mergedById.values()).sort((a, b) => Number(a.id) - Number(b.id));
      await mediaCache.saveMessages(cacheKey, mergedLocal);

      return {
        success: true,
        messages: formattedMessages,
        hasMore,
        oldestMessageId: formattedMessages[0]?.id ?? lastMessageId
      };
    } catch (e) {
      console.error("Failed to get messages:", e);
      return { messages: [], hasMore: false, oldestMessageId: null };
    }
  }

  async getAvatar(id: any) {
    const fakeChat = this.getTwitterFakeChat(id);
    if (fakeChat?.avatarUrl) return { success: true, dataUrl: fakeChat.avatarUrl };
    if (!this.client) return { success: false, error: "Not connected" };
    try {
      const cacheKey = `avatar_${id}`;
      const cachedUrl = await mediaCache.getMedia(cacheKey, 'image/jpeg');
      if (cachedUrl) return { success: true, dataUrl: cachedUrl };

      const buffer = await this.client.downloadProfilePhoto(id, { isBig: false });
      if (buffer && buffer.length > 0) {
        const url = await mediaCache.saveMedia(cacheKey, buffer, 'image/jpeg');
        return { success: true, dataUrl: url };
      }
      return { success: false, error: "No photo" };
    } catch (e: any) {
      return { success: false, error: e.message };
    }
  }
  async getFullChat(chatId: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) {
      return {
        success: true,
        fullInfo: {
          about: `Perfil importado do Twitter/X: @${fakeChat.username}\n${fakeChat.originalUrl}`,
          participantsCount: fakeChat.messages.length,
          username: fakeChat.username,
          pinnedMsgId: null,
        }
      };
    }
    return null;
  }
  async resolveLink(url: string) { return { success: false, chat: null }; }
  async readHistory(chatId: any) { return { success: true }; }
  async getForumTopics(chatId: any) {
    if (!this.client) return { success: false, topics: [] };
    try {
      const channel = await this.client.getInputEntity(chatId);
      const result: any = await this.client.invoke(new Api.channels.GetForumTopics({
        channel,
        offsetDate: 0,
        offsetId: 0,
        offsetTopic: 0,
        limit: 100,
      }));
      return {
        success: true,
        topics: result.topics.map((t: any) => ({
          id: t.id,
          title: t.title,
          isClosed: t.closed,
          isPinned: t.pinned,
          unreadCount: t.unreadCount || 0,
        }))
      };
    } catch (e) {
      console.error(e);
      return { success: false, topics: [] };
    }
  }
  private emitDownloadProgress(data: any) {
    this.downloadProgressCallbacks.forEach(cb => cb(data));
  }

  private emitSaveMultipleProgress(data: any) {
    this.saveMultipleProgressCallbacks.forEach(cb => cb(data));
  }

  private async pathExists(path: string) {
    return invoke<boolean>('path_exists', { path });
  }

  private async ensureDir(path: string) {
    await invoke('ensure_dir', { path });
  }

  private async saveBytesToFile(filePath: string, data: any) {
    const bytes = toUint8Array(data);
    const chunkSize = 512 * 1024;

    await invoke('begin_download_file', { filePath });
    try {
      for (let offset = 0; offset < bytes.byteLength; offset += chunkSize) {
        const chunk = bytes.slice(offset, offset + chunkSize);
        await invoke('append_download_file_chunk', { filePath, data: Array.from(chunk) });
        await nextFrame();
      }
      await invoke('finish_download_file', { filePath });
    } catch (error) {
      await invoke('abort_download_file', { filePath }).catch(() => {});
      throw error;
    }
  }

  private async appendBytesToFile(filePath: string, data: any) {
    const bytes = toUint8Array(data);
    const chunkSize = 256 * 1024;

    for (let offset = 0; offset < bytes.byteLength; offset += chunkSize) {
      const chunk = bytes.slice(offset, offset + chunkSize);
      await invoke('append_download_file_chunk', { filePath, data: Array.from(chunk) });
      await nextFrame();
    }
  }

  private async downloadUrlToFile(url: string, filePath: string, onProgress?: (percent: number) => void) {
    const response = await tauriFetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    onProgress?.(20);
    const buffer = await response.arrayBuffer();
    onProgress?.(80);
    await this.saveBytesToFile(filePath, buffer);
    onProgress?.(100);
  }

  private async bufferFromUrl(url: string) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Falha ao ler mídia baixada (${response.status}).`);
    return response.arrayBuffer();
  }

  private async saveCachedMediaToDownloads(cacheKey: string, fileName: string, fallbackUrl?: string) {
    let buffer = await mediaCache.getMediaBuffer(cacheKey);
    if ((!buffer || buffer.byteLength === 0) && fallbackUrl) {
      buffer = await this.bufferFromUrl(fallbackUrl);
    }
    if (!buffer || buffer.byteLength === 0) {
      throw new Error('Mídia baixada sem bytes. Tente abrir a mídia e baixar novamente.');
    }

    const downloadDir = await getDownloadDir();
    const filePath = await joinSystemPath(downloadDir, sanitizeForFilename(fileName));
    await this.saveBytesToFile(filePath, buffer);
    return filePath;
  }

  private getDownloadWorkersFallback() {
    return 4;
  }

  private async getDownloadWorkers() {
    try {
      const settings = await mediaCache.getCacheSettings();
      return Math.min(4, Math.max(1, Number(settings.downloadWorkers || 4)));
    } catch {
      return this.getDownloadWorkersFallback();
    }
  }

  private async getSenderFolderName(message: any) {
    let senderName = 'Desconhecido';
    try {
      const sender = await message.getSender?.();
      if (sender?.firstName || sender?.lastName) {
        senderName = [sender.firstName, sender.lastName].filter(Boolean).join(' ').trim();
      } else if (sender?.title || sender?.username) {
        senderName = sender.title || sender.username;
      } else if (message.senderId) {
        senderName = `ID_${message.senderId.toString()}`;
      }
    } catch (e) {
      console.error('Error getting sender for splitting media:', e);
      if (message.senderId) senderName = `ID_${message.senderId.toString()}`;
    }
    return sanitizeForFolderName(senderName);
  }

  private async saveTelegramMessageToFile(message: any, filePath: string, workers: number, onProgress?: (percent: number) => void) {
    let lastProgressPercent = -1;
    let lastProgressAt = 0;
    const rawSize = message.document?.size || message.media?.document?.size || message.photo?.sizes?.slice(-1)?.[0]?.size || 0;
    const totalBytes = toNumberValue(rawSize);
    let downloadedBytes = 0;

    await invoke('begin_download_file', { filePath });
    try {
      const iter = this.client!.iterDownload({
        file: message.media,
        requestSize: 512 * 1024,
      });

      for await (const chunk of iter) {
        if (this.activeDownloadAborted || this.saveMultipleAborted) throw new Error('STOP_ABORTED');
        const bytes = toUint8Array(chunk);
        await this.appendBytesToFile(filePath, bytes);
        downloadedBytes += bytes.byteLength;

        const percent = totalBytes > 0
          ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
          : 0;
        const now = Date.now();
        if (percent !== lastProgressPercent && (percent === 100 || now - lastProgressAt > 250)) {
          lastProgressPercent = percent;
          lastProgressAt = now;
          onProgress?.(percent);
        }
      }
      await invoke('finish_download_file', { filePath });
    } catch (error) {
      await invoke('abort_download_file', { filePath }).catch(() => {});
      throw error;
    }
  }

  private async tryStartTdlibDownload({ chatId, folderPath, topic, splitByUser, splitByAlbum, chatMeta }: any) {
    if (!runtimeCapabilities.supportsTdlib) return null;
    const numericChatId = toNumberValue(chatId);
    if (!Number.isFinite(numericChatId) || numericChatId === 0) return null;
    if (splitByAlbum) return null;

    try {
      const initStatus: any = await this.tdlibInit();
      if (!initStatus?.ready) return null;

      const runId = Date.now();
      const batchId = `telegram_mass_${numericChatId}_${topic?.id || 'all'}_${runId}`;
      const batchTitle = topic?.title
        ? `${chatMeta?.title || 'Telegram'} · ${topic.title}`
        : `${chatMeta?.title || 'Telegram'} · Mass Download`;
      const nativeFolderPath = topic?.title ? joinPath(folderPath, sanitizeForFolderName(topic.title)) : folderPath;
      const knownDownloadIds = new Map<string, string>();
      const unsubscribe = await this.onTdlibDownloadProgress((data: any) => {
        const items = Array.isArray(data.items) ? data.items : [];
        const batchMeta = {
          batchTotal: data.total || 0,
          batchDownloaded: data.downloaded || 0,
          batchCompleted: items.filter((item: any) => item?.status === 'completed').length,
          batchSkipped: items.filter((item: any) => item?.status === 'skipped').length,
          batchFailed: items.filter((item: any) => item?.status === 'failed' || item?.status === 'stopped').length,
        };

        for (const item of items) {
          if (!item?.filePath) continue;
          const key = item.filePath;
          const thumbnailUrl = item.thumbnailPath ? convertFileSrc(item.thumbnailPath) : undefined;
          const existingId = knownDownloadIds.get(key);
          const status = item.status === 'completed'
            ? 'completed'
            : item.status === 'failed' || item.status === 'stopped'
              ? 'failed'
              : 'downloading';
          const error = item.status === 'stopped' ? 'Cancelado pelo usuário' : undefined;

          if (!existingId && item.status !== 'skipped') {
            const downloadId = `telegram_tdlib_${numericChatId}_${runId}_${knownDownloadIds.size}`;
            knownDownloadIds.set(key, downloadId);
            downloadService.addDownload({
              id: downloadId,
              chatId,
              fileName: item.name,
              filePath: item.filePath,
              fileSize: item.size || undefined,
              progress: item.progress || 0,
              status,
              error,
              platform: 'telegram',
              thumbnailUrl,
              chatTitle: chatMeta?.title,
              chatKind: chatMeta?.kind,
              topicTitle: topic?.title || undefined,
              sourceLabel: topic?.title ? `${chatMeta?.title || 'Telegram'} / ${topic.title}` : chatMeta?.title,
              batchId,
              batchTitle,
              batchKind: 'mass',
              ...batchMeta,
            });
          } else if (existingId) {
            const updates: any = {
              progress: item.progress || 0,
              status,
              error,
              ...batchMeta,
            };
            if (item.size > 0) updates.fileSize = item.size;
            if (thumbnailUrl) updates.thumbnailUrl = thumbnailUrl;
            downloadService.updateDownload(existingId, updates);
          }
        }

        this.emitDownloadProgress({
          chatId,
          total: data.total || 0,
          downloaded: data.downloaded || 0,
          currentFile: data.currentFile || 'Baixando com TDLib...',
          topicTitle: topic?.title || null,
          isScanning: !!data.isScanning,
          items,
        });
      });

      try {
        const result: any = await this.tdlibStartMassDownload({
          chatId: numericChatId,
          folderPath: nativeFolderPath,
          topicId: topic?.id ?? null,
          splitByUser: topic ? false : splitByUser,
        });

        if (!result?.success) return null;
        return {
          success: true,
          downloadedCount: result.downloaded_count ?? result.downloadedCount ?? 0,
          skippedCount: result.skipped_count ?? result.skippedCount ?? 0,
          failedCount: result.failed_count ?? result.failedCount ?? 0,
          total: result.total ?? 0,
          aborted: !!result.aborted,
          native: true,
        };
      } finally {
        unsubscribe();
      }
    } catch (error) {
      console.warn('[TelegramService] TDLib mass download unavailable, falling back to GramJS:', error);
      return null;
    }
  }

  async startDownload({ chatId, folderPath, topic, splitByUser, splitByAlbum = false, albumSplitMode = 'separator', chatMeta }: any) {
    try {
      this.activeDownloadAborted = false;
      this.saveMultipleAborted = false;

      const fakeChat = this.getTwitterFakeChat(chatId);
      if (fakeChat) {
        return this.startFakeChatDownload({ chatId, folderPath, fakeChat, chatMeta });
      }

      const nativeResult = await this.tryStartTdlibDownload({ chatId, folderPath, topic, splitByUser, splitByAlbum, chatMeta });
      if (nativeResult) return nativeResult;

      if (!this.client) return { success: false, error: 'Telegram não conectado.' };

      const entity = await this.client.getEntity(chatId);
      const downloadFolder = topic?.title ? joinPath(folderPath, sanitizeForFolderName(topic.title)) : folderPath;
      await this.ensureDir(downloadFolder);

      let downloadedCount = 0;
      let skippedCount = 0;
      let failedCount = 0;
      let current = 0;
      let totalMedia = 0;
      const runId = Date.now();
      const batchId = `telegram_mass_${chatId}_${topic?.id || 'all'}_${runId}`;
      const batchTitle = topic?.title
        ? `${chatMeta?.title || 'Telegram'} · ${topic.title}`
        : `${chatMeta?.title || 'Telegram'} · Mass Download`;
      const processedItems: any[] = [];
      const batchDownloadIds = new Set<string>();
      const workers = await this.getDownloadWorkers();
      const shouldSplitByAlbum = !!splitByAlbum;
      const splitAlbumByComment = shouldSplitByAlbum && albumSplitMode === 'comment';
      const splitAlbumBySeparator = shouldSplitByAlbum && !splitAlbumByComment;
      let albumIndex = splitAlbumBySeparator ? 1 : 0;
      let currentAlbumFolder: string | null = splitAlbumBySeparator ? joinPath(downloadFolder, '001') : null;
      let currentAlbumHasMedia = false;
      let messagesForAlbumDownload: any[] | null = null;
      const commentAlbumFolders = new Map<string, string>();
      if (currentAlbumFolder) await this.ensureDir(currentAlbumFolder);

      this.emitDownloadProgress({
        chatId,
        total: 0,
        downloaded: 0,
        currentFile: splitAlbumByComment ? 'Escaneando comentários de álbum...' : shouldSplitByAlbum ? 'Escaneando separadores de álbum...' : 'Contando mídias...',
        topicTitle: topic?.title || null,
        isScanning: true,
        items: processedItems
      });

      if (shouldSplitByAlbum) {
        messagesForAlbumDownload = [];
        let scanned = 0;
        const scanIter = this.client.iterMessages(entity, {
          limit: undefined,
          replyTo: topic?.id || undefined
        });

        for await (const message of scanIter) {
          if (this.activeDownloadAborted) break;
          scanned++;
          messagesForAlbumDownload.push(message);
          if (isDownloadableMedia(message)) totalMedia++;
          if (scanned % 100 === 0) {
            this.emitDownloadProgress({
              chatId,
              total: totalMedia,
              downloaded: 0,
              currentFile: splitAlbumByComment
                ? `Escaneando comentários... ${totalMedia} mídias`
                : `Escaneando separadores... ${totalMedia} mídias`,
              topicTitle: topic?.title || null,
              isScanning: true,
              items: processedItems
            });
            await nextFrame();
          }
        }

        messagesForAlbumDownload.reverse();
        if (splitAlbumByComment) {
          const groupedAlbums = new Map<string, any[]>();
          for (const message of messagesForAlbumDownload) {
            if (!isDownloadableMedia(message)) continue;
            const groupedId = getMessageGroupedId(message);
            if (!groupedId) continue;
            if (!groupedAlbums.has(groupedId)) groupedAlbums.set(groupedId, []);
            groupedAlbums.get(groupedId)!.push(message);
          }

          let commentAlbumIndex = 1;
          for (const [groupedId, albumMessages] of groupedAlbums) {
            const comment = albumMessages.map(getMessageText).find(Boolean);
            const folderName = comment
              ? `${String(commentAlbumIndex).padStart(3, '0')} ${comment}`
              : String(commentAlbumIndex).padStart(3, '0');
            const folderPathForAlbum = joinPath(downloadFolder, sanitizeForAlbumFolderName(folderName));
            commentAlbumFolders.set(groupedId, folderPathForAlbum);
            await this.ensureDir(folderPathForAlbum);
            commentAlbumIndex++;
          }
        }
      } else {
        try {
          const countRes: any = await this.client.getMessages(entity, {
            filter: new Api.InputMessagesFilterPhotoVideo(),
            limit: 1,
            replyTo: topic?.id || undefined
          });
          totalMedia = countRes?.total || 0;
        } catch (err) {
          console.error('Error fetching media count:', err);
        }
      }

      if (this.activeDownloadAborted) return { success: true, aborted: true };

      const sendProgressUpdate = (currentFile: string, partialDownloaded?: number) => {
        this.emitDownloadProgress({
          chatId,
          total: totalMedia,
          downloaded: Math.min(totalMedia || Number.MAX_SAFE_INTEGER, partialDownloaded ?? (downloadedCount + skippedCount)),
          currentFile,
          topicTitle: topic?.title || null,
          isScanning: false,
          items: processedItems
        });
      };
      const batchMeta = (partialDownloaded?: number) => ({
        batchTotal: totalMedia,
        batchDownloaded: Math.min(totalMedia || Number.MAX_SAFE_INTEGER, partialDownloaded ?? (downloadedCount + skippedCount)),
        batchCompleted: downloadedCount,
        batchSkipped: skippedCount,
        batchFailed: failedCount,
      });
      const updateBatchDownloads = (partialDownloaded?: number) => {
        const updates = batchMeta(partialDownloaded);
        batchDownloadIds.forEach(id => downloadService.updateDownload(id, updates));
      };

      const messagesIter = messagesForAlbumDownload || this.client.iterMessages(entity, {
        filter: new Api.InputMessagesFilterPhotoVideo(),
        limit: undefined,
        replyTo: topic?.id || undefined
      });

      for await (const message of messagesIter) {
        if (this.activeDownloadAborted) break;
        if (splitAlbumBySeparator) {
          const text = getMessageText(message);
          if (isLikelyAlbumSeparator(message)) {
            if (currentAlbumHasMedia) {
              albumIndex++;
              currentAlbumHasMedia = false;
            }
            const albumName = sanitizeForAlbumFolderName(String(albumIndex).padStart(3, '0'));
            currentAlbumFolder = joinPath(downloadFolder, albumName);
            await this.ensureDir(currentAlbumFolder);
            sendProgressUpdate(`Álbum ${albumName}: ${text}`);
            continue;
          }
        }
        if (!isDownloadableMedia(message)) continue;

        current++;
        const safeName = getMessageDownloadFilename(message);
        const itemSize = Number(message.document?.size || message.photo?.sizes?.slice(-1)?.[0]?.size || 0);
        let currentFolder = downloadFolder;

        if (splitAlbumByComment) {
          const groupedId = getMessageGroupedId(message);
          if (groupedId && commentAlbumFolders.has(groupedId)) {
            currentFolder = commentAlbumFolders.get(groupedId)!;
          } else if (currentAlbumFolder) {
            currentFolder = currentAlbumFolder;
          }
        } else if (splitAlbumBySeparator && currentAlbumFolder) {
          currentFolder = currentAlbumFolder;
          currentAlbumHasMedia = true;
        } else if (splitByUser && !topic) {
          currentFolder = joinPath(downloadFolder, await this.getSenderFolderName(message));
          await this.ensureDir(currentFolder);
        }

        const filePath = joinPath(currentFolder, safeName);
        const downloadId = `telegram_mass_${chatId}_${message.id}_${runId}`;
        if (await this.pathExists(filePath)) {
          skippedCount++;
          processedItems.push({ name: safeName, status: 'skipped', progress: 100, size: itemSize });
          updateBatchDownloads();
          if (skippedCount % 10 === 0 || current === totalMedia) {
            sendProgressUpdate(`Ignorando arquivos já baixados... (${skippedCount} ignorados)`);
          }
          continue;
        }

        let thumbnailUrl: string | undefined;
        try {
          const thumbRes: any = await this.getMessageMedia({ chatId, messageId: message.id });
          if (thumbRes?.success && thumbRes.filePath) thumbnailUrl = thumbRes.filePath;
        } catch {
          // Thumbnail is best-effort; mass download should not wait on preview failures.
        }

        const item = { name: safeName, status: 'downloading', progress: 0, size: itemSize };
        processedItems.push(item);
        downloadService.addDownload({
          id: downloadId,
          chatId,
          messageId: message.id,
          fileName: safeName,
          filePath,
          fileSize: itemSize || undefined,
          progress: 0,
          status: 'downloading',
          platform: 'telegram',
          thumbnailUrl,
          chatTitle: chatMeta?.title,
          chatKind: chatMeta?.kind,
          topicTitle: topic?.title || undefined,
          sourceLabel: topic?.title ? `${chatMeta?.title || 'Telegram'} / ${topic.title}` : chatMeta?.title,
          batchId,
          batchTitle,
          batchKind: 'mass',
          ...batchMeta(),
        });
        batchDownloadIds.add(downloadId);
        sendProgressUpdate(safeName);

        try {
          await this.saveTelegramMessageToFile(message, filePath, workers, (percent) => {
            if (this.activeDownloadAborted) throw new Error('STOP_ABORTED');
            item.progress = percent;
            const partialDownloaded = downloadedCount + skippedCount + (percent / 100);
            downloadService.updateDownload(downloadId, { progress: percent, ...batchMeta(partialDownloaded) });
            sendProgressUpdate(`${safeName} (${percent}%)`, partialDownloaded);
          });

          if (this.activeDownloadAborted) break;
          downloadedCount++;
          item.status = 'completed';
          item.progress = 100;
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100, ...batchMeta() });
          sendProgressUpdate(safeName);
        } catch (err: any) {
          if (err?.message === 'STOP_ABORTED' || this.activeDownloadAborted) break;
          console.error(`Failed to download ${safeName}:`, err);
          failedCount++;
          item.status = 'failed';
          downloadService.updateDownload(downloadId, { status: 'failed', error: err?.message || String(err), ...batchMeta() });
          sendProgressUpdate(`Falhou: ${safeName}`);
        }
        await nextFrame();
      }

      const finalStatus = this.activeDownloadAborted
        ? `Parado: ${downloadedCount} baixados, ${skippedCount} ignorados`
        : totalMedia === 0
          ? 'Nenhuma mídia encontrada'
          : `Concluído: ${downloadedCount} baixados, ${skippedCount} ignorados, ${failedCount} falharam`;

      this.emitDownloadProgress({
        chatId,
        total: totalMedia,
        downloaded: this.activeDownloadAborted ? Math.min(totalMedia, downloadedCount + skippedCount) : totalMedia,
        currentFile: finalStatus,
        topicTitle: topic?.title || null,
        isScanning: false,
        items: processedItems
      });

      return { success: true, downloadedCount, skippedCount, failedCount, total: totalMedia, aborted: this.activeDownloadAborted };
    } catch (error: any) {
      console.error('Download error:', error);
      return { success: false, error: error?.message || String(error) };
    }
  }

  private async startFakeChatDownload({ chatId, folderPath, fakeChat, chatMeta }: any) {
    const mediaMessages = fakeChat.messages.filter((message: TwitterFakeMessage) => message.url);
    const total = mediaMessages.length;
    let downloadedCount = 0;
    let skippedCount = 0;
    let failedCount = 0;
    const runId = Date.now();
    const batchId = `telegram_mass_${chatId}_fake_${runId}`;
    const batchTitle = `${chatMeta?.title || fakeChat.title || 'Twitter/X'} · Mass Download`;
    const processedItems: any[] = [];
    const batchDownloadIds = new Set<string>();
    const batchMeta = (partialDownloaded?: number) => ({
      batchTotal: total,
      batchDownloaded: Math.min(total || Number.MAX_SAFE_INTEGER, partialDownloaded ?? (downloadedCount + skippedCount)),
      batchCompleted: downloadedCount,
      batchSkipped: skippedCount,
      batchFailed: failedCount,
    });
    const updateBatchDownloads = (partialDownloaded?: number) => {
      const updates = batchMeta(partialDownloaded);
      batchDownloadIds.forEach(id => downloadService.updateDownload(id, updates));
    };

    await this.ensureDir(folderPath);
    this.emitDownloadProgress({ chatId, total, downloaded: 0, currentFile: 'Iniciando...', isScanning: false, items: processedItems });

    for (const message of mediaMessages) {
      if (this.activeDownloadAborted) break;
      const ext = message.isVideo ? '.mp4' : '.jpg';
      const safeName = sanitizeForFilename(`media_${message.id}${ext}`);
      const filePath = joinPath(folderPath, safeName);
      const downloadId = `telegram_mass_${chatId}_${message.id}_${runId}`;

      if (await this.pathExists(filePath)) {
        skippedCount++;
        processedItems.push({ name: safeName, status: 'skipped', progress: 100, size: message.mediaSize || 0 });
        updateBatchDownloads();
        continue;
      }

      const item = { name: safeName, status: 'downloading', progress: 0, size: message.mediaSize || 0 };
      processedItems.push(item);
      downloadService.addDownload({
        id: downloadId,
        chatId,
        messageId: message.id,
        fileName: safeName,
        filePath,
        fileSize: message.mediaSize || undefined,
        progress: 0,
        status: 'downloading',
        platform: 'telegram',
        thumbnailUrl: message.thumbnailUrl || fakeChat.thumbnailUrl || undefined,
        chatTitle: chatMeta?.title || fakeChat.title,
        chatKind: chatMeta?.kind || 'twitter',
        sourceLabel: chatMeta?.title || fakeChat.title,
        batchId,
        batchTitle,
        batchKind: 'mass',
        ...batchMeta(),
      });
      batchDownloadIds.add(downloadId);
      try {
        await this.downloadUrlToFile(message.url!, filePath, (percent) => {
          item.progress = percent;
          const partialDownloaded = downloadedCount + skippedCount + (percent / 100);
          downloadService.updateDownload(downloadId, { progress: percent, ...batchMeta(partialDownloaded) });
          this.emitDownloadProgress({ chatId, total, downloaded: partialDownloaded, currentFile: `${safeName} (${percent}%)`, items: processedItems });
        });
        downloadedCount++;
        item.status = 'completed';
        item.progress = 100;
        downloadService.updateDownload(downloadId, { status: 'completed', progress: 100, ...batchMeta() });
      } catch (err) {
        console.error(`Failed to download ${safeName}:`, err);
        failedCount++;
        item.status = 'failed';
        downloadService.updateDownload(downloadId, { status: 'failed', error: err instanceof Error ? err.message : String(err), ...batchMeta() });
      }

      this.emitDownloadProgress({ chatId, total, downloaded: downloadedCount + skippedCount, currentFile: safeName, items: processedItems });
      await nextFrame();
    }

    this.emitDownloadProgress({
      chatId,
      total,
      downloaded: this.activeDownloadAborted ? downloadedCount + skippedCount : total,
      currentFile: this.activeDownloadAborted ? 'Cancelado pelo usuário' : 'Concluído!',
      items: processedItems
    });

    return { success: true, downloadedCount, skippedCount, failedCount, total, aborted: this.activeDownloadAborted };
  }

  async stopDownload() {
    this.activeDownloadAborted = true;
    await this.tdlibStopDownload().catch(() => {});
    return { success: true };
  }

  onDownloadProgress(cb: any) {
    this.downloadProgressCallbacks.add(cb);
    return () => this.downloadProgressCallbacks.delete(cb);
  }
  async getMessageMedia({ chatId, messageId }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      return { success: true, filePath: fakeMessage.thumbnailUrl || null };
    }

    if (!this.client) return { success: false };
    try {
      const cacheKey = `media_${chatId}_${messageId}_thumb`;
      const cachedUrl = await mediaCache.getMedia(cacheKey, 'image/jpeg');
      if (cachedUrl) return { success: true, filePath: cachedUrl };

      const entity = await this.client.getEntity(chatId);
      const messages = await this.client.getMessages(entity, { ids: [messageId] });
      if (messages && messages.length > 0 && messages[0].media) {
        const buffer = await this.client.downloadMedia(messages[0], { thumb: 1 });
        if (buffer) {
          const url = await mediaCache.saveMedia(cacheKey, buffer, 'image/jpeg');
          return { success: true, filePath: url };
        }
      }
      return { success: false };
    } catch (e: any) {
      return { success: false, error: e.message };
    }
  }

  async getMessageMediaFile({ chatId, messageId }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      return this.downloadTwitterFakeMedia(chatId, messageId, fakeMessage);
    }

    if (!this.client) return { success: false };
    try {
      const cacheKey = `media_${chatId}_${messageId}_full`;
      const cachedUrl = await mediaCache.getMedia(cacheKey);
      if (cachedUrl) return { success: true, filePath: cachedUrl };

      const entity = await this.client.getEntity(chatId);
      const messages = await this.client.getMessages(entity, { ids: [messageId] });
      if (messages && messages.length > 0 && messages[0].media) {
        const buffer = await this.client.downloadMedia(messages[0], {
          progressCallback: (downloaded, total) => {
            if (total) {
              const progress = Math.round((Number(downloaded) / Number(total)) * 100);
              this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress, stage: 'downloading' }));
            }
          }
        });
        if (buffer) {
          const mimeType = (messages[0].media as any)?.document?.mimeType || undefined;
          const url = await mediaCache.saveMedia(cacheKey, buffer, mimeType);
          return { success: true, filePath: url };
        }
      }
      return { success: false };
    } catch (e: any) {
      return { success: false, error: e.message };
    }
  }

  async getMessageMediaStream(opts: any): Promise<{ success: boolean; streamUrl?: string; filePath?: string; error?: any }> {
    const { chatId, messageId } = opts;
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      const downloadRes = await this.downloadTwitterFakeMedia(chatId, messageId, fakeMessage);
      if (!downloadRes.success) return downloadRes;
      
      if ('serviceWorker' in navigator && navigator.serviceWorker.controller) {
        console.log(`[TelegramService] getMessageMediaStream using service worker stream for Twitter video ${chatId}/${messageId}`);
        return { success: true, streamUrl: `/stream_media/${chatId}/${messageId}` };
      }
      return downloadRes;
    }

    if ('serviceWorker' in navigator && navigator.serviceWorker.controller) {
      // Prefer range-capable streaming for video playback. Blob URLs from the
      // full-media cache are still useful as a fallback, but they can be less
      // reliable for media seeking/playback in WebView.
      console.log(`[TelegramService] getMessageMediaStream using service worker stream for ${chatId}/${messageId}`);
      return { success: true, streamUrl: `/stream_media/${chatId}/${messageId}` };
    }

    const cacheKey = `media_${chatId}_${messageId}_full`;
    const cachedUrl = await mediaCache.getMedia(cacheKey, 'video/mp4');
    if (cachedUrl) {
      console.log(`[TelegramService] getMessageMediaStream using cached blob fallback for ${chatId}/${messageId}`);
      return { success: true, streamUrl: cachedUrl };
    }

    // Fallback: download full file
    console.log(`[TelegramService] getMessageMediaStream downloading full media fallback for ${chatId}/${messageId}`);
    const res = await this.getMessageMediaFile(opts);
    if (res.success && res.filePath) {
      return { success: true, streamUrl: res.filePath };
    }
    console.warn(`[TelegramService] getMessageMediaStream failed for ${chatId}/${messageId}: ${res.error || 'unknown error'}`);
    return res;
  }

  onMediaProgress(cb: any) {
    this.mediaProgressCallbacks.add(cb);
    return () => this.mediaProgressCallbacks.delete(cb);
  }
  
  onSaveMultipleProgress(cb: any) {
    this.saveMultipleProgressCallbacks.add(cb);
    return () => this.saveMultipleProgressCallbacks.delete(cb);
  }
  onDeepLink(cb: any) { return () => {}; }
  private emitSendProgress(data: any) {
    this.sendProgressCallbacks.forEach(cb => cb(data));
  }

  onSendProgress(cb: any) {
    this.sendProgressCallbacks.add(cb);
    return () => this.sendProgressCallbacks.delete(cb);
  }

  onNewMessage(cb: any) {
    this.newMessageCallbacks.add(cb);
    return () => this.newMessageCallbacks.delete(cb);
  }
  
  async checkInvite(url: string) { return { success: false, chat: null, alreadyMember: false }; }
  openExternal(url: string) {}
  async searchUserMedia({ chatId, userId, limit = 100 }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) {
      return {
        success: true,
        media: fakeChat.messages
          .filter(message => message.url && String(fakeChat.id) === String(userId))
          .slice(-limit)
          .map(message => ({
            id: message.id,
            isVideo: message.isVideo,
            mediaSize: message.mediaSize ?? null,
          })),
      };
    }

    if (!this.client) return { success: false, error: 'Telegram não conectado.' };

    const peerIdToString = (value: any): string => {
      if (value == null) return '';
      if (typeof value === 'string' || typeof value === 'number' || typeof value === 'bigint') {
        return String(value);
      }
      if (typeof value.toString === 'function' && value.toString !== Object.prototype.toString) {
        const str = value.toString();
        if (str && str !== '[object Object]') return str;
      }
      return String(value.userId ?? value.channelId ?? value.chatId ?? value.value ?? value.id ?? '');
    };
    const normalizePeerId = (value: any) => peerIdToString(value).replace(/[^\d-]/g, '');
    const targetUserId = normalizePeerId(userId);

    const mapMediaMessage = (m: any) => ({
      id: m.id,
      isVideo: !!(m.media?.document?.mimeType && typeof m.media.document.mimeType === 'string' && m.media.document.mimeType.includes('video')),
      mediaSize: m.media?.document?.size ? Number(m.media.document.size) : (m.media?.photo?.sizes?.slice(-1)[0]?.size || null),
    });

    try {
      const entity = await this.client.getEntity(chatId);
      let fromUser: any = userId;
      try {
        fromUser = await this.client.getEntity(userId);
      } catch {
        // Numeric sender IDs still work as a fallback in some GramJS paths.
      }

      let messages: any[] = [];
      try {
        messages = await this.client.getMessages(entity, {
          limit,
          filter: new Api.InputMessagesFilterPhotoVideo(),
          fromUser,
        });
      } catch (searchErr) {
        console.warn('Telegram fromUser media search failed, falling back to scan:', searchErr);
      }

      let mediaMessages = (Array.isArray(messages) ? messages : []).filter((m: any) => m?.media);

      if (mediaMessages.length === 0) {
        const scanLimit = Math.max(limit * 10, 1000);
        messages = await this.client.getMessages(entity, {
          limit: scanLimit,
          filter: new Api.InputMessagesFilterPhotoVideo(),
        });
        mediaMessages = (Array.isArray(messages) ? messages : [])
          .filter((m: any) => {
            const senderId = normalizePeerId(m.senderId ?? m.fromId);
            return m?.media && senderId && senderId === targetUserId;
          })
          .slice(0, limit);
      }

      return {
        success: true,
        media: mediaMessages.map(mapMediaMessage),
      };
    } catch (e: any) {
      console.error('Failed to search user media:', e);
      return { success: false, error: e.message || 'Falha ao buscar mídias do usuário.' };
    }
  }
  async selectFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Selecionar pasta de destino',
      });

      if (!selected || Array.isArray(selected)) return { success: false, folderPath: '' };
      return { success: true, folderPath: selected };
    } catch (e: any) {
      console.error('Failed to select folder:', e);
      return { success: false, folderPath: '', error: e?.message || String(e) };
    }
  }
  async saveMultipleMediaFiles({ chatId, messageIds, folderPath }: any) {
    try {
      this.saveMultipleAborted = false;
      this.activeDownloadAborted = false;
      await this.ensureDir(folderPath);

      const fakeChat = this.getTwitterFakeChat(chatId);
      if (fakeChat) {
        return this.saveMultipleFakeMediaFiles({ chatId, messageIds, folderPath, fakeChat });
      }

      if (!this.client) return { success: false, error: 'Telegram não conectado.' };

      const entity = await this.client.getEntity(chatId);
      const total = messageIds.length;
      let downloadedCount = 0;
      let failedCount = 0;
      const runId = Date.now();
      const workers = await this.getDownloadWorkers();

      const batchResult = await this.client.getMessages(entity, { ids: messageIds });
      const messages = Array.isArray(batchResult) ? batchResult : (batchResult ? [batchResult] : []);
      const messageMap = new Map();
      for (const msg of messages) {
        if (msg?.id) messageMap.set(msg.id, msg);
      }

      for (let i = 0; i < messageIds.length; i++) {
        if (this.saveMultipleAborted) break;

        const messageId = messageIds[i];
        const message = messageMap.get(messageId);
        if (!message?.media) {
          failedCount++;
          continue;
        }

        const filename = getMessageDownloadFilename(message);
        const filePath = joinPath(folderPath, filename);
        const downloadId = `telegram_bulk_${chatId}_${messageId}_${runId}`;

        if (await this.pathExists(filePath)) {
          downloadedCount++;
          this.emitSaveMultipleProgress({
            chatId,
            total,
            downloaded: downloadedCount,
            currentFile: `Ignorado (já existe): ${filename}`,
            status: 'progress'
          });
          continue;
        }

        downloadService.addDownload({
          id: downloadId,
          chatId,
          messageId,
          fileName: filename,
          filePath,
          progress: 0,
          status: 'downloading',
          platform: 'telegram',
        });
        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'downloading' });

        try {
          await this.saveTelegramMessageToFile(message, filePath, workers, (percent) => {
            if (this.saveMultipleAborted) throw new Error('STOP_ABORTED');
            downloadService.updateDownload(downloadId, { progress: percent });
            this.emitSaveMultipleProgress({
              chatId,
              total,
              downloaded: downloadedCount + (percent / 100),
              currentFile: `${filename} (${percent}%)`,
              status: 'progress'
            });
          });

          if (this.saveMultipleAborted) break;
          downloadedCount++;
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        } catch (err: any) {
          if (err?.message === 'STOP_ABORTED' || this.saveMultipleAborted) break;
          console.error(`Error downloading media for message ${messageId}:`, err);
          failedCount++;
          downloadService.updateDownload(downloadId, { status: 'failed', error: err?.message || String(err) });
        }

        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'progress' });
        await nextFrame();
      }

      this.emitSaveMultipleProgress({
        chatId,
        total,
        downloaded: downloadedCount,
        currentFile: this.saveMultipleAborted ? 'Cancelado pelo usuário' : 'Concluído!',
        status: 'completed'
      });

      return { success: true, downloadedCount, failedCount, aborted: this.saveMultipleAborted };
    } catch (error: any) {
      console.error('Error in saveMultipleMediaFiles:', error);
      return { success: false, error: error?.message || String(error) };
    }
  }

  private async saveMultipleFakeMediaFiles({ chatId, messageIds, folderPath, fakeChat }: any) {
    const total = messageIds.length;
    let downloadedCount = 0;
    let failedCount = 0;
    const runId = Date.now();
    const messageMap = new Map(fakeChat.messages.map((message: TwitterFakeMessage) => [message.id, message]));

    for (const messageId of messageIds) {
      if (this.saveMultipleAborted) break;
      const message = messageMap.get(messageId);
      if (!message?.url) {
        failedCount++;
        continue;
      }

      const filename = sanitizeForFilename(`media_${message.id}${message.isVideo ? '.mp4' : '.jpg'}`);
      const filePath = joinPath(folderPath, filename);
      const downloadId = `telegram_bulk_${chatId}_${messageId}_${runId}`;

      if (await this.pathExists(filePath)) {
        downloadedCount++;
        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: `Ignorado (já existe): ${filename}`, status: 'progress' });
        continue;
      }

      try {
        downloadService.addDownload({
          id: downloadId,
          chatId,
          messageId,
          fileName: filename,
          filePath,
          progress: 0,
          status: 'downloading',
          platform: 'telegram',
        });
        await this.downloadUrlToFile(message.url, filePath, (percent) => {
          downloadService.updateDownload(downloadId, { progress: percent });
          this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount + (percent / 100), currentFile: `${filename} (${percent}%)`, status: 'progress' });
        });
        downloadedCount++;
        downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
      } catch (err) {
        console.error(`Error downloading fake media for message ${messageId}:`, err);
        failedCount++;
        downloadService.updateDownload(downloadId, { status: 'failed', error: err instanceof Error ? err.message : String(err) });
      }
      await nextFrame();
    }

    this.emitSaveMultipleProgress({
      chatId,
      total,
      downloaded: downloadedCount,
      currentFile: this.saveMultipleAborted ? 'Cancelado pelo usuário' : 'Concluído!',
      status: 'completed'
    });

    return { success: true, downloadedCount, failedCount, aborted: this.saveMultipleAborted };
  }

  async stopSaveMultiple() {
    this.saveMultipleAborted = true;
    return { success: true };
  }
  async selectFile() {
    const selected = await open({
      multiple: false,
      directory: false,
    });
    if (!selected || Array.isArray(selected)) return { success: false, filePath: '', fileName: '' };
    return { success: true, filePath: selected, fileName: basename(selected) };
  }

  async sendMedia({ chatId, filePath, caption = '', replyToId, topicId }: any) {
    if (!this.client) return { success: false, error: 'Telegram não conectado.' };

    try {
      const entity = await this.client.getEntity(chatId);
      let lastProgress = -1;
      const replyTo = replyToId || topicId || undefined;
      const message = await this.client.sendFile(entity, {
        file: filePath,
        caption: caption || '',
        replyTo,
        progressCallback: (uploaded: any, total: any) => {
          const totalNumber = toNumberValue(total);
          const progress = totalNumber > 0
            ? Math.min(100, Math.round((toNumberValue(uploaded) / totalNumber) * 100))
            : 0;
          if (progress !== lastProgress) {
            lastProgress = progress;
            this.emitSendProgress({ chatId, progress });
          }
        },
      });
      this.emitSendProgress({ chatId, progress: 100 });
      return { success: true, message };
    } catch (error: any) {
      console.error('Failed to send media:', error);
      return { success: false, error: error?.message || String(error) };
    }
  }

  async sendMessage({ chatId, text, replyToId, topicId }: any) {
    if (!this.client) return { success: false, error: 'Telegram não conectado.' };
    if (!String(text || '').trim()) return { success: false, error: 'Mensagem vazia.' };

    try {
      const entity = await this.client.getEntity(chatId);
      const replyTo = replyToId || topicId || undefined;
      const message = await this.client.sendMessage(entity, {
        message: String(text),
        replyTo,
      });
      return { success: true, message };
    } catch (error: any) {
      console.error('Failed to send message:', error);
      return { success: false, error: error?.message || String(error) };
    }
  }
  async createTopic(opts: any) { return { success: true }; }
  async getOriginalMessage(opts: any) { return { success: true, message: null }; }
  async sendReaction(opts: any) { return { success: true }; }
  getPathForFile(file: any) { return ''; }
  async joinChat(chatId: any) { return { success: true }; }
  async saveMessageMediaFile({ chatId, messageId, downloadMeta = {}, saveAs = false }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (!this.client && !fakeMessage) return { success: false };
    let message: any = null;
    let suggestedName = `media_${messageId}`;

    if (fakeMessage) {
      suggestedName = sanitizeForFilename(`media_${messageId}${fakeMessage.isVideo ? '.mp4' : '.jpg'}`);
    } else {
      try {
        const entity = await this.client!.getEntity(chatId);
        const messages = await this.client!.getMessages(entity, { ids: [messageId] });
        message = Array.isArray(messages) ? messages[0] : messages;
        if (message) suggestedName = getMessageDownloadFilename(message);
      } catch (e) {
        console.error('Failed to load media before Save As:', e);
      }
    }

    let saveAsPath = '';
    if (saveAs) {
      const selectedPath = await save({
        title: 'Salvar mídia como',
        defaultPath: suggestedName,
      });
      if (!selectedPath) return { success: false, canceled: true };
      saveAsPath = selectedPath;
      suggestedName = basename(selectedPath);
    }

    const id = `${chatId}_${messageId}`;
    let thumbnailUrl = fakeMessage?.thumbnailUrl || null;
    if (!thumbnailUrl) {
      try {
        const thumbRes = await this.getMessageMedia({ chatId, messageId });
        if (thumbRes?.success && thumbRes.filePath) thumbnailUrl = thumbRes.filePath;
      } catch {
        // Thumbnail is best-effort; downloads should continue without it.
      }
    }
    
    downloadService.addDownload({
      id,
      chatId,
      messageId,
      fileName: suggestedName,
      progress: 0,
      status: 'downloading',
      platform: 'telegram',
      thumbnailUrl: thumbnailUrl || undefined,
      ...downloadMeta,
    });

    try {
      const unsub = this.onMediaProgress((data: any) => {
        if (data.chatId === chatId && data.messageId === messageId) {
          downloadService.updateDownload(id, { progress: data.progress });
        }
      });

      if (saveAs && saveAsPath) {
        if (fakeMessage) {
          if (!fakeMessage.url) throw new Error('Mídia sem URL.');
          await this.downloadUrlToFile(fakeMessage.url, saveAsPath, (progress) => {
            downloadService.updateDownload(id, { progress });
            this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress, stage: 'downloading' }));
          });
        } else {
          if (!message) {
            const entity = await this.client!.getEntity(chatId);
            const messages = await this.client!.getMessages(entity, { ids: [messageId] });
            message = Array.isArray(messages) ? messages[0] : messages;
          }
          if (!message?.media) throw new Error('Mídia não encontrada.');
          await this.saveTelegramMessageToFile(message, saveAsPath, await this.getDownloadWorkers(), (progress) => {
            downloadService.updateDownload(id, { progress });
            this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress, stage: 'downloading' }));
          });
        }
        unsub();
        downloadService.updateDownload(id, {
          status: 'completed',
          progress: 100,
          fileName: basename(saveAsPath),
        });
        return { success: true, filePath: saveAsPath };
      }

      const res = await this.getMessageMediaFile({ chatId, messageId });
      unsub();

      if (res.success && res.filePath) {
        let savedFilePath: string | undefined;
        if (runtimeCapabilities.isAndroid) {
          if (fakeMessage?.url) {
            const downloadDir = await getDownloadDir();
            savedFilePath = await joinSystemPath(downloadDir, suggestedName);
            await this.downloadUrlToFile(fakeMessage.url, savedFilePath, (progress) => {
              downloadService.updateDownload(id, { progress });
              this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress, stage: 'saving' }));
            });
          } else {
            savedFilePath = await this.saveCachedMediaToDownloads(`media_${chatId}_${messageId}_full`, suggestedName, res.filePath);
          }
        }

        downloadService.updateDownload(id, { 
          status: 'completed', 
          progress: 100, 
          fileName: savedFilePath ? basename(savedFilePath) : suggestedName,
          filePath: savedFilePath,
        });

        if (!runtimeCapabilities.isAndroid) {
          downloadUrlInBrowser(res.filePath, suggestedName, '_blank');
        }
        return { success: true, filePath: savedFilePath };
      }

      downloadService.updateDownload(id, { 
        status: 'failed', 
        error: "Download failed" 
      });
      return { success: false };
    } catch (e: any) {
      downloadService.updateDownload(id, { 
        status: 'failed', 
        error: e.message 
      });
      return { success: false, error: e.message };
    }
  }
  async muteChat(opts: any) { return { success: true }; }
  async leaveChat(chatId: any) {
    if (this.isTwitterFakeChat(chatId)) {
      writeTwitterFakeChats(readTwitterFakeChats().filter(chat => chat.id !== chatId));
      window.dispatchEvent(new CustomEvent(TWITTER_FAKE_CHAT_EVENT, { detail: { chatId, removed: true } }));
    }
    return { success: true };
  }
  async getSharedMedia({ chatId, limit = 12 }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) {
      return {
        success: true,
        media: fakeChat.messages.filter(message => message.url).slice(-limit).map(message => ({
          id: message.id,
          isVideo: message.isVideo,
          mediaSize: message.mediaSize ?? null,
        })),
      };
    }

    if (!this.client) return { success: false };
    try {
      const entity = await this.client.getEntity(chatId);
      const messages = await this.client.getMessages(entity, {
        limit,
        filter: new Api.InputMessagesFilterPhotoVideo()
      });
      return {
        success: true,
        media: messages.map(m => ({
          id: m.id,
          isVideo: !!m.video,
          mediaSize: m.document?.size ? Number(m.document.size) : null
        }))
      };
    } catch (e: any) {
      return { success: false, error: e.message };
    }
  }

  async getCacheStats() {
    return mediaCache.getCacheStats();
  }

  async getCacheSettings() {
    const settings = await mediaCache.getCacheSettings();
    return {
      success: true,
      maxCacheSize: settings.maxCacheSize,
      avatarRefreshHours: settings.avatarRefreshHours,
      downloadWorkers: settings.downloadWorkers
    };
  }

  async setCacheSettings(settings: any) {
    const success = await mediaCache.setCacheSettings(settings);
    return { success };
  }

  async clearCache() {
    await mediaCache.clearCache();
    return { success: true };
  }
}

export const telegramService = new TelegramService();

// @ts-nocheck
import { TelegramClient, Api } from 'telegram';
import { StringSession } from 'telegram/sessions';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { fetch as tauriFetch } from '@tauri-apps/plugin-http';
import bigInt from 'big-integer';

import { downloadService } from '../downloader/DownloadService';
import { mediaCache } from './MediaCacheService';
import {
  findTwitterFakeMessage,
  getTwitterFakeChat,
  isTwitterFakeChatId,
  readTwitterFakeChats,
  TWITTER_FAKE_CHAT_EVENT,
  twitterFakeDialogs,
  writeTwitterFakeChats,
  type TwitterFakeMessage,
} from './TwitterFakeChatStore';

const apiId = Number(import.meta.env.VITE_API_ID || "0");
const apiHash = import.meta.env.VITE_API_HASH || "";

function toNumberValue(value: any) {
  if (typeof value === 'number') return value;
  if (typeof value === 'bigint') return Number(value);
  if (value && typeof value.toJSNumber === 'function') return value.toJSNumber();
  if (value && typeof value.toString === 'function') return Number(value.toString());
  return 0;
}

function sanitizeForFilename(value: any) {
  return String(value || 'Desconhecido')
    .replace(/[^\w.-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 160) || 'arquivo';
}

function joinPath(...parts: string[]) {
  const filtered = parts.filter(Boolean);
  if (!filtered.length) return '';
  return filtered
    .map((part, index) => {
      if (index === 0) return part.replace(/\/+$/g, '');
      return part.replace(/^\/+|\/+$/g, '');
    })
    .join('/');
}

function basename(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

function getMediaFileExtension(message: any) {
  if (message?.photo) return '.jpg';
  if (message?.video) return '.mp4';

  const fileExt = message?.file?.ext;
  if (fileExt) return fileExt.startsWith('.') ? fileExt : `.${fileExt}`;

  const mimeType = message?.document?.mimeType || '';
  if (mimeType === 'video/webm') return '.webm';
  if (mimeType === 'video/quicktime') return '.mov';
  if (mimeType.startsWith('video/')) return '.mp4';
  if (mimeType.startsWith('image/')) return '.jpg';
  if (mimeType.startsWith('audio/')) return '.mp3';
  return '.bin';
}

function getMessageDownloadFilename(message: any) {
  let name = message?.file?.name || `media_${message?.id || 'file'}${getMediaFileExtension(message)}`;
  if (name.endsWith('.bin')) {
    const ext = getMediaFileExtension(message);
    if (ext !== '.bin') name = `${name.slice(0, -4)}${ext}`;
  }
  return sanitizeForFilename(name);
}

function isDownloadableMedia(message: any) {
  if (!message || message.out) return false;
  if (message.photo || message.video) return true;
  const mime = message.document?.mimeType || '';
  return mime.startsWith('video/') || mime.startsWith('image/') || mime.startsWith('audio/');
}

function toUint8Array(data: any) {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new Uint8Array(data || []);
}

class TelegramService {
  private client: TelegramClient | null = null;
  private connectPromise: Promise<void> | null = null;
  private session: StringSession;
  private mediaProgressCallbacks: Set<Function> = new Set();
  private downloadProgressCallbacks: Set<Function> = new Set();
  private saveMultipleProgressCallbacks: Set<Function> = new Set();
  private activeDownloadAborted = false;
  private saveMultipleAborted = false;
  private serviceWorkerMessageHandler: (event: MessageEvent) => void;
  public skipLogin: boolean = false;

  constructor() {
    const savedSession = localStorage.getItem('telegram_session') || '';
    this.session = new StringSession(savedSession);
    
    if (!savedSession) {
      // Define DC 4 (Américas) como padrão inicial usando DOMÍNIO em vez de IP
      // Isso é obrigatório no Tauri (Linux/WebKit) para não dar erro de certificado SSL
      this.session.setDC(4, "vesta.web.telegram.org", 443);
    }

    this.serviceWorkerMessageHandler = this.handleServiceWorkerMessage.bind(this);

    if ('serviceWorker' in navigator) {
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
    localStorage.removeItem('telegram_session');
    if (this.client) {
      Promise.resolve(this.client.disconnect()).catch(error => {
        console.warn('[TelegramService] Failed to disconnect old client while resetting session:', error);
      });
    }
    this.client = null;
    this.session = new StringSession('');
    this.session.setDC(4, "vesta.web.telegram.org", 443);
  }

  private streamIterators = new Map<string, { iter: AsyncIterator<any>, message: any, fileSize: number, mimeType: string, offset: number }>();

  private isTwitterFakeChat(chatId: any) {
    return isTwitterFakeChatId(chatId);
  }

  private getTwitterFakeChat(chatId: any) {
    return getTwitterFakeChat(chatId);
  }

  private findTwitterFakeMessage(chatId: any, messageId: any) {
    return findTwitterFakeMessage(chatId, messageId);
  }

  private async downloadTwitterFakeMedia(chatId: any, messageId: any, message: TwitterFakeMessage) {
    if (!message.url) return { success: false, error: 'Mídia sem URL.' };

    try {
      const cacheKey = `twitter_fake_${chatId}_${messageId}_full`;
      const cachedUrl = await mediaCache.getMedia(cacheKey, message.isVideo ? 'video/mp4' : undefined);
      if (cachedUrl) return { success: true, filePath: cachedUrl, streamUrl: cachedUrl };

      this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress: 1, stage: 'downloading' }));
      
      console.log(`[TelegramService] Downloading Twitter/X media from: ${message.url}`);
      const response = await tauriFetch(message.url, {
        method: 'GET',
        headers: {
          'Referer': 'https://x.com/',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
        }
      });
      
      if (!response.ok) {
        throw new Error(`Twitter/X media HTTP ${response.status}`);
      }
      
      const buffer = await response.arrayBuffer();
      // Force correct MIME type specifically for WebKit video player support
      const contentType = message.isVideo ? 'video/mp4' : (response.headers.get('content-type') || 'image/jpeg');
      
      const filePath = await mediaCache.saveMedia(cacheKey, buffer, contentType);
      this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress: 100, stage: 'ready' }));
      return { success: true, filePath, streamUrl: filePath };
    } catch (error: any) {
      console.error("[TelegramService] Failed to download Twitter/X media:", error);
      this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress: 0, stage: 'failed' }));
      return { success: false, error: error.message || String(error) };
    }
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
    if (this.client && this.client.connected) return;
    if (this.connectPromise) return this.connectPromise;
    
    if (!apiId || !apiHash) {
      console.warn("API_ID and API_HASH are required in .env");
      throw new Error("Credenciais do Telegram ausentes. Crie um arquivo .env com VITE_API_ID e VITE_API_HASH.");
    }

    this.connectPromise = (async () => {
      if (!this.client) {
        this.client = new TelegramClient(this.session, apiId, apiHash, {
          connectionRetries: 5,
          useWSS: true,
        });
      }

      await this.client.connect();
    })();

    try {
      await this.connectPromise;
    } finally {
      this.connectPromise = null;
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

  async sendCode(phoneNumber: string) {
    try {
      await this.connect();
      
      let cleanPhone = phoneNumber.replace(/[^0-9+]/g, '');
      if (!cleanPhone.startsWith('+')) {
        cleanPhone = '+' + cleanPhone;
      }
      
      const result: any = await this.client!.sendCode(
        { apiId, apiHash },
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
      localStorage.setItem('telegram_session', savedSessionString);
      
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
        const savedSession = localStorage.getItem('telegram_session');
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

  async getMessages({ chatId, limit = 50, offsetId = 0, topicId = undefined }: any) {
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

      // 1. Fetch Local Cached Messages
      const localMessages = await mediaCache.getMessages(chatId);

      while (allValidMessages.length < 15) { // Fetch at least 15 valid messages for the UI
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
      const getSenderName = (m: any) => {
        if (m.out) return null;
        const sender = m.sender;
        if (!sender) return null;
        if (sender.firstName || sender.lastName) {
          return [sender.firstName, sender.lastName].filter(Boolean).join(' ').trim();
        }
        return sender.title || sender.username || null;
      };

      const formattedMessages = allValidMessages.map(m => {
        const isVideo = !!(m.media?.document?.mimeType && typeof m.media.document.mimeType === 'string' && m.media.document.mimeType.includes('video'));
        const isPhoto = !!(m.media?.photo || m.media?.className === 'MessageMediaPhoto');
        const videoDuration = isVideo ? m.media?.document?.attributes?.find((a: any) => a.className === 'DocumentAttributeVideo')?.duration : null;
        const mediaSize = m.media?.document?.size || m.media?.photo?.sizes?.slice(-1)[0]?.size || null;
        
        return {
          id: m.id,
          message: m.message,
          date: m.date,
          out: m.out,
          senderId: m.senderId?.toString(),
          senderName: getSenderName(m),
          replyToMsgId: m.replyTo?.replyToMsgId,
          media: m.media ? true : false,
          text: m.message,
          hasMedia: m.media ? true : false,
          isPhoto,
          isVideo,
          groupedId: m.groupedId ? m.groupedId.toString() : null,
          videoDuration,
          mediaSize,
          isDeleted: false
        };
      });

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

        // Merge and sort by ID descending (or as required by UI)
        formattedMessages.push(...deletedMessages);
        formattedMessages.sort((a, b) => b.id - a.id); // GramJS returns highest ID first
      }

      // 4. Save merged result to local cache
      // We merge with all existing local messages outside this range
      const mergedLocal = [...localMessages];
      formattedMessages.forEach(fm => {
        const idx = mergedLocal.findIndex(lm => lm.id === fm.id);
        if (idx !== -1) mergedLocal[idx] = fm;
        else mergedLocal.push(fm);
      });
      mergedLocal.sort((a, b) => b.id - a.id);
      await mediaCache.saveMessages(chatId, mergedLocal);

      return {
        success: true,
        messages: formattedMessages,
        hasMore,
        oldestMessageId: lastMessageId
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
    await invoke('save_download_file', { filePath, data: Array.from(bytes) });
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

  private getDownloadWorkersFallback() {
    return 4;
  }

  private async getDownloadWorkers() {
    try {
      const settings = await mediaCache.getCacheSettings();
      return Math.max(1, Number(settings.downloadWorkers || 4));
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
    return sanitizeForFilename(senderName);
  }

  private async saveTelegramMessageToFile(message: any, filePath: string, workers: number, onProgress?: (percent: number) => void) {
    const buffer = await this.client!.downloadMedia(message, {
      workers,
      progressCallback: (downloaded: any, total: any) => {
        if (this.activeDownloadAborted || this.saveMultipleAborted) throw new Error('STOP_ABORTED');
        const totalNumber = toNumberValue(total);
        const percent = totalNumber > 0
          ? Math.min(100, Math.round((toNumberValue(downloaded) / totalNumber) * 100))
          : 0;
        onProgress?.(percent);
      }
    });

    if (!buffer) throw new Error('Download retornou vazio.');
    await this.saveBytesToFile(filePath, buffer);
  }

  async startDownload({ chatId, folderPath, topic, splitByUser }: any) {
    try {
      this.activeDownloadAborted = false;
      this.saveMultipleAborted = false;

      const fakeChat = this.getTwitterFakeChat(chatId);
      if (fakeChat) {
        return this.startFakeChatDownload({ chatId, folderPath, fakeChat });
      }

      if (!this.client) return { success: false, error: 'Telegram não conectado.' };

      const entity = await this.client.getEntity(chatId);
      const downloadFolder = topic?.title ? joinPath(folderPath, sanitizeForFilename(topic.title)) : folderPath;
      await this.ensureDir(downloadFolder);

      let downloadedCount = 0;
      let skippedCount = 0;
      let failedCount = 0;
      let current = 0;
      let totalMedia = 0;
      const processedItems: any[] = [];
      const workers = await this.getDownloadWorkers();

      this.emitDownloadProgress({
        chatId,
        total: 0,
        downloaded: 0,
        currentFile: 'Contando mídias...',
        topicTitle: topic?.title || null,
        isScanning: true,
        items: processedItems
      });

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

      const messagesIter = this.client.iterMessages(entity, {
        filter: new Api.InputMessagesFilterPhotoVideo(),
        limit: undefined,
        replyTo: topic?.id || undefined
      });

      for await (const message of messagesIter) {
        if (this.activeDownloadAborted) break;
        if (!isDownloadableMedia(message)) continue;

        current++;
        const safeName = getMessageDownloadFilename(message);
        const itemSize = Number(message.document?.size || message.photo?.sizes?.slice(-1)?.[0]?.size || 0);
        let currentFolder = downloadFolder;

        if (splitByUser && !topic) {
          currentFolder = joinPath(downloadFolder, await this.getSenderFolderName(message));
          await this.ensureDir(currentFolder);
        }

        const filePath = joinPath(currentFolder, safeName);
        if (await this.pathExists(filePath)) {
          skippedCount++;
          processedItems.push({ name: safeName, status: 'skipped', progress: 100, size: itemSize });
          if (skippedCount % 10 === 0 || current === totalMedia) {
            sendProgressUpdate(`Ignorando arquivos já baixados... (${skippedCount} ignorados)`);
          }
          continue;
        }

        const item = { name: safeName, status: 'downloading', progress: 0, size: itemSize };
        processedItems.push(item);
        sendProgressUpdate(safeName);

        try {
          await this.saveTelegramMessageToFile(message, filePath, workers, (percent) => {
            if (this.activeDownloadAborted) throw new Error('STOP_ABORTED');
            item.progress = percent;
            sendProgressUpdate(`${safeName} (${percent}%)`, downloadedCount + skippedCount + (percent / 100));
          });

          if (this.activeDownloadAborted) break;
          downloadedCount++;
          item.status = 'completed';
          item.progress = 100;
          sendProgressUpdate(safeName);
        } catch (err: any) {
          if (err?.message === 'STOP_ABORTED' || this.activeDownloadAborted) break;
          console.error(`Failed to download ${safeName}:`, err);
          failedCount++;
          item.status = 'failed';
          sendProgressUpdate(`Falhou: ${safeName}`);
        }
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

  private async startFakeChatDownload({ chatId, folderPath, fakeChat }: any) {
    const mediaMessages = fakeChat.messages.filter((message: TwitterFakeMessage) => message.url);
    const total = mediaMessages.length;
    let downloadedCount = 0;
    let skippedCount = 0;
    let failedCount = 0;
    const processedItems: any[] = [];

    await this.ensureDir(folderPath);
    this.emitDownloadProgress({ chatId, total, downloaded: 0, currentFile: 'Iniciando...', isScanning: false, items: processedItems });

    for (const message of mediaMessages) {
      if (this.activeDownloadAborted) break;
      const ext = message.isVideo ? '.mp4' : '.jpg';
      const safeName = sanitizeForFilename(`media_${message.id}${ext}`);
      const filePath = joinPath(folderPath, safeName);

      if (await this.pathExists(filePath)) {
        skippedCount++;
        processedItems.push({ name: safeName, status: 'skipped', progress: 100, size: message.mediaSize || 0 });
        continue;
      }

      const item = { name: safeName, status: 'downloading', progress: 0, size: message.mediaSize || 0 };
      processedItems.push(item);
      try {
        await this.downloadUrlToFile(message.url!, filePath, (percent) => {
          item.progress = percent;
          this.emitDownloadProgress({ chatId, total, downloaded: downloadedCount + skippedCount + (percent / 100), currentFile: `${safeName} (${percent}%)`, items: processedItems });
        });
        downloadedCount++;
        item.status = 'completed';
        item.progress = 100;
      } catch (err) {
        console.error(`Failed to download ${safeName}:`, err);
        failedCount++;
        item.status = 'failed';
      }

      this.emitDownloadProgress({ chatId, total, downloaded: downloadedCount + skippedCount, currentFile: safeName, items: processedItems });
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
  onSendProgress(cb: any) { return () => {}; }
  
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

        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'downloading' });

        try {
          await this.saveTelegramMessageToFile(message, filePath, workers, (percent) => {
            if (this.saveMultipleAborted) throw new Error('STOP_ABORTED');
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
        } catch (err: any) {
          if (err?.message === 'STOP_ABORTED' || this.saveMultipleAborted) break;
          console.error(`Error downloading media for message ${messageId}:`, err);
          failedCount++;
        }

        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'progress' });
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

      if (await this.pathExists(filePath)) {
        downloadedCount++;
        this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: `Ignorado (já existe): ${filename}`, status: 'progress' });
        continue;
      }

      try {
        await this.downloadUrlToFile(message.url, filePath, (percent) => {
          this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount + (percent / 100), currentFile: `${filename} (${percent}%)`, status: 'progress' });
        });
        downloadedCount++;
      } catch (err) {
        console.error(`Error downloading fake media for message ${messageId}:`, err);
        failedCount++;
      }
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
  async selectFile() { return { success: false, filePath: '', fileName: '' }; }
  async sendMedia(opts: any) { return { success: true }; }
  async sendMessage(opts: any) { return { success: true }; }
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
    } else if (saveAs) {
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
        downloadService.updateDownload(id, { 
          status: 'completed', 
          progress: 100, 
        });

        const a = document.createElement('a');
        a.href = res.filePath;
        a.download = `media_${messageId}`;
        a.target = '_blank';
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        return { success: true };
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

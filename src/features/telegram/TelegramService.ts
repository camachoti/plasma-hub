// @ts-nocheck
import { TelegramClient, Api } from 'telegram';
import { StringSession } from 'telegram/sessions';
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

class TelegramService {
  private client: TelegramClient | null = null;
  private session: StringSession;
  private mediaProgressCallbacks: Set<Function> = new Set();
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
    
    if (!apiId || !apiHash) {
      console.warn("API_ID and API_HASH are required in .env");
      throw new Error("Credenciais do Telegram ausentes. Crie um arquivo .env com VITE_API_ID e VITE_API_HASH.");
    }

    this.client = new TelegramClient(this.session, apiId, apiHash, {
      connectionRetries: 5,
      useWSS: true,
    });

    await this.client.connect();
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
    if (this.skipLogin || !this.client) return { success: true, dialogs: fakeDialogs };
    try {
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
          replyToMsgId: m.replyTo?.replyToMsgId,
          media: m.media ? true : false,
          text: m.message,
          hasMedia: m.media ? true : false,
          isPhoto,
          isVideo,
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
  async startDownload(opts: any) { return { success: true }; }
  async stopDownload() { return { success: true }; }
  onDownloadProgress(cb: any) { return () => {}; }
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
  
  onSaveMultipleProgress(cb: any) { return () => {}; }
  onDeepLink(cb: any) { return () => {}; }
  onSendProgress(cb: any) { return () => {}; }
  
  async checkInvite(url: string) { return { success: false, chat: null, alreadyMember: false }; }
  openExternal(url: string) {}
  async searchUserMedia(opts: any) { return { success: true, media: [] }; }
  async selectFolder() { return { success: false, folderPath: '' }; }
  async saveMultipleMediaFiles(opts: any) { return { success: true }; }
  async stopSaveMultiple() { return { success: true }; }
  async selectFile() { return { success: false, filePath: '', fileName: '' }; }
  async sendMedia(opts: any) { return { success: true }; }
  async sendMessage(opts: any) { return { success: true }; }
  async createTopic(opts: any) { return { success: true }; }
  async getOriginalMessage(opts: any) { return { success: true, message: null }; }
  async sendReaction(opts: any) { return { success: true }; }
  getPathForFile(file: any) { return ''; }
  async joinChat(chatId: any) { return { success: true }; }
  async saveMessageMediaFile({ chatId, messageId }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (!this.client && !fakeMessage) return { success: false };
    const id = `${chatId}_${messageId}`;
    
    downloadService.addDownload({
      id,
      chatId,
      messageId,
      fileName: `media_${messageId}`,
      progress: 0,
      status: 'downloading',
      platform: 'telegram'
    });

    try {
      const unsub = this.onMediaProgress((data: any) => {
        if (data.chatId === chatId && data.messageId === messageId) {
          downloadService.updateDownload(id, { progress: data.progress });
        }
      });

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

// @ts-nocheck
import { convertFileSrc, invokeCommand as invoke } from '../../shared/platform/tauri';

import { downloadUrlInBrowser } from '../../shared/platform/browserDownload';
import { openDialog as open, saveDialog as save } from '../../shared/platform/dialog';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { canUseServiceWorker } from '../../shared/platform/serviceWorker';
import { appStorage } from '../../shared/storage/appStorage';
import { debugLog, debugWarn } from '../../shared/debug/logger';
import { downloadService } from '../downloader/DownloadService';
import { mediaCache } from './MediaCacheService';
import {
  basename,
  joinPath,
  messageCacheKey,
  nextFrame,
  sanitizeForFilename,
  sanitizeForFolderName,
  toNumberValue,
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
import { telegramFileStorage } from './TelegramFileStorage';

class TelegramService {
  private static readonly MESSAGE_CACHE_FRESH_MS = 30 * 1000;
  private static readonly MAX_THUMBNAIL_DOWNLOADS = 4;
  private static readonly MAX_AVATAR_DOWNLOADS = 4;
  private static readonly MAX_FULL_MEDIA_DOWNLOADS = 2;
  private static readonly STREAM_RANGE_BYTES = 512 * 1024;
  private static readonly INITIAL_PLAYBACK_BUFFER_BYTES = 1024 * 1024;
  private mediaProgressCallbacks: Set<Function> = new Set();
  private downloadProgressCallbacks: Set<Function> = new Set();
  private saveMultipleProgressCallbacks: Set<Function> = new Set();
  private sendProgressCallbacks: Set<Function> = new Set();
  private newMessageCallbacks: Set<Function> = new Set();
  private activeDownloadAborted = false;
  private saveMultipleAborted = false;
  private serviceWorkerMessageHandler: (event: MessageEvent) => void;
  private tdlibBridge = new TelegramTdlibBridge(() => telegramApiCredentials);
  private twitterFakeBridge = new TelegramTwitterFakeBridge(data => {
    this.mediaProgressCallbacks.forEach(cb => cb(data));
  });
  private fileStorage = telegramFileStorage;
  private mediaFileRequests = new Map<string, Promise<any>>();
  private mediaThumbRequests = new Map<string, Promise<any>>();
  private activeNativeMediaDownloads = new Map<string, { chatId: string; messageId: number; priority: 'user' | 'background' }>();
  private avatarRequests = new Map<string, Promise<any>>();
  private sharedMediaCache = new Map<string, { loadedAt: number; media: any[] }>();
  private activeThumbnailDownloads = 0;
  private activeAvatarDownloads = 0;
  private activeFullMediaDownloads = 0;
  private thumbnailQueue: Array<{
    key: string;
    chatId: string;
    messageId: number;
    run: () => void;
    cancel: () => void;
    priority: 'visible' | 'background';
  }> = [];
  private avatarQueue: Array<{
    key: string;
    run: () => void;
    cancel: () => void;
    priority: 'visible' | 'background';
  }> = [];
  private fullMediaQueue: Array<{
    key: string;
    chatId: string;
    messageId: number;
    run: () => void;
    cancel: () => void;
    priority: 'user' | 'background';
  }> = [];
  public skipLogin: boolean = false;
  private readonly tdlibSessionKey = 'telegram_tdlib_session';

  constructor() {
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

    if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      this.tdlibBridge.onNativeMediaProgress((data: any) => {
        this.emitMediaProgress(data);
      }).catch(error => {
        debugWarn('[TelegramService] Failed to listen to native media progress:', error);
      });
    }
  }

  resetSession() {
    appStorage.remove(this.tdlibSessionKey);
  }

  private mediaStreamSessions = new Map<string, {
    chatId: any;
    messageId: any;
    cacheKey: string;
    message?: any;
    totalSize: number;
    mimeType: string;
    fileName?: string;
  }>();

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

  private fullMediaCacheKey(chatId: any, messageId: any) {
    return this.findTwitterFakeMessage(chatId, messageId)
      ? `twitter_fake_${chatId}_${messageId}_full`
      : `media_${chatId}_${messageId}_full`;
  }

  private mimeTypeFromFileName(fileName?: string | null) {
    const lower = String(fileName || '').toLowerCase();
    if (/\.(jpe?g|jfif)(?:$|[?#])/.test(lower)) return 'image/jpeg';
    if (/\.png(?:$|[?#])/.test(lower)) return 'image/png';
    if (/\.webp(?:$|[?#])/.test(lower)) return 'image/webp';
    if (/\.gif(?:$|[?#])/.test(lower)) return 'image/gif';
    if (/\.mp4(?:$|[?#])/.test(lower)) return 'video/mp4';
    if (/\.webm(?:$|[?#])/.test(lower)) return 'video/webm';
    if (/\.(mov|qt)(?:$|[?#])/.test(lower)) return 'video/quicktime';
    if (/\.mp3(?:$|[?#])/.test(lower)) return 'audio/mpeg';
    if (/\.m4a(?:$|[?#])/.test(lower)) return 'audio/mp4';
    if (/\.ogg(?:$|[?#])/.test(lower)) return 'audio/ogg';
    return null;
  }

  private getMessageMediaMimeType(message: any, fileName?: string | null) {
    const explicitMimeType = message?.media?.document?.mimeType
      || message?.document?.mimeType
      || message?.file?.mimeType
      || message?.file?.mime;
    if (explicitMimeType) return explicitMimeType;

    if (message?.media?.photo || message?.photo || message?.media?.className === 'MessageMediaPhoto') {
      return 'image/jpeg';
    }

    if (message?.video || message?.media?.video) return 'video/mp4';

    return this.mimeTypeFromFileName(fileName) || 'application/octet-stream';
  }

  private emitMediaProgress(data: any) {
    this.mediaProgressCallbacks.forEach(cb => cb(data));
  }

  private streamUrlForMessage(chatId: any, messageId: any) {
    return `/stream_media/${chatId}/${messageId}`;
  }

  private async getMediaDescriptor(chatId: any, messageId: any) {
    const cacheKey = this.fullMediaCacheKey(chatId, messageId);
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) throw new Error('Mídia sem URL.');
      const cached = await this.downloadTwitterFakeMedia(chatId, messageId, fakeMessage);
      if (!cached.success) throw new Error(cached.error || 'Falha ao preparar mídia.');
      const buffer = await mediaCache.getMediaBuffer(cacheKey);
      const totalSize = buffer?.byteLength || toNumberValue(fakeMessage.mediaSize || 0);
      return {
        cacheKey,
        fakeMessage,
        totalSize,
        mimeType: fakeMessage.isVideo ? 'video/mp4' : 'image/jpeg',
        fileName: sanitizeForFilename(`media_${messageId}${fakeMessage.isVideo ? '.mp4' : '.jpg'}`),
      };
    }

    throw new Error('Descritor de mídia disponível apenas pelo TDLib nativo.');
  }

  private async downloadMediaRange({ chatId, messageId, offset, length, session }: any) {
    const descriptor = session || await this.getMediaDescriptor(chatId, messageId);
    const cacheKey = descriptor.cacheKey;
    const safeOffset = Math.max(0, Number(offset) || 0);
    const safeLength = Math.max(0, Number(length) || 0);
    if (safeLength <= 0) return new ArrayBuffer(0);

    const cached = await mediaCache.getMediaSegment(cacheKey, safeOffset, safeLength);
    if (cached) return cached;

    if (descriptor.fakeMessage) {
      const fakeRange = await mediaCache.getMediaSegment(cacheKey, safeOffset, safeLength);
      if (fakeRange) return fakeRange;
      throw new Error('Mídia fake não encontrada no cache.');
    }

    throw new Error('Streaming legado indisponível. Use playback/cache TDLib nativo.');
  }

  private async completeMediaFromRanges({ chatId, messageId, descriptor, priority = 'user' }: any) {
    const cacheKey = descriptor.cacheKey;
    const totalSize = Number(descriptor.totalSize || 0);
    if (totalSize <= 0) return { success: false, error: 'Tamanho da mídia indisponível.' };

    for (let offset = 0; offset < totalSize; offset += TelegramService.STREAM_RANGE_BYTES) {
      const length = Math.min(TelegramService.STREAM_RANGE_BYTES, totalSize - offset);
      const cached = await mediaCache.getMediaSegment(cacheKey, offset, length);
      if (!cached) {
        await this.downloadMediaRange({ chatId, messageId, offset, length, session: descriptor });
      }
      if (priority === 'background') await nextFrame();
    }

    const filePath = await mediaCache.finalizeMediaSegments(cacheKey, descriptor.mimeType);
    if (!filePath) return { success: false, error: 'Não foi possível finalizar o cache da mídia.' };
    return { success: true, filePath };
  }

  private async handleServiceWorkerMessage(event: MessageEvent) {
    const { type, chatId, messageId, requestId, offset, length, streamId } = event.data || {};
    const streamKey = `${chatId}_${messageId}_${streamId}`;

    debugLog(`[TelegramService] Received message from SW: type=${type}, streamId=${streamId}, offset=${offset}, length=${length}`);

    if (type === 'prepare_stream') {
      try {
        const descriptor = await this.getMediaDescriptor(chatId, messageId);
        if (!descriptor.totalSize || descriptor.totalSize <= 0) {
          throw new Error('Tamanho da mídia indisponível.');
        }

        this.mediaStreamSessions.set(streamKey, {
          chatId,
          messageId,
          cacheKey: descriptor.cacheKey,
          message: descriptor.message,
          totalSize: descriptor.totalSize,
          mimeType: descriptor.mimeType,
          fileName: descriptor.fileName,
        });

        await mediaCache.saveMediaAssetMeta(descriptor.cacheKey, {
          state: 'partial',
          totalBytes: descriptor.totalSize,
          mimeType: descriptor.mimeType,
          fileName: descriptor.fileName,
        });

        navigator.serviceWorker.controller?.postMessage({
          type: 'stream_response',
          requestId,
          totalSize: descriptor.totalSize,
          mimeType: descriptor.mimeType,
        });
      } catch (err: any) {
        debugWarn(`[TelegramService] Error in prepare_stream:`, err);
        navigator.serviceWorker.controller?.postMessage({ type: 'stream_response', requestId, error: err.message || String(err) });
      }
    } else if (type === 'get_range') {
      try {
        const session = this.mediaStreamSessions.get(streamKey) || await this.getMediaDescriptor(chatId, messageId);
        const chunk = await this.downloadMediaRange({ chatId, messageId, offset, length, session });
        const chunkBuffer = chunk.slice(0);
        navigator.serviceWorker.controller?.postMessage(
          { type: 'range_response', requestId, chunk: chunkBuffer },
          [chunkBuffer]
        );
      } catch (err: any) {
        debugWarn(`[TelegramService] Error in get_range for streamKey=${streamKey}:`, err);
        navigator.serviceWorker.controller?.postMessage({ type: 'range_response', requestId, error: err.message || String(err) });
      }
    } else if (type === 'cancel_stream') {
      debugLog(`[TelegramService] Cancelling stream for streamKey=${streamKey}`);
      this.mediaStreamSessions.delete(streamKey);
    }
  }

  async connect() {
    throw new Error('Use TDLib nativo.');
  }

  private setupNewMessageHandler() {
    // New-message updates should be wired through TDLib native events.
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

  private isMessageCacheFresh(meta: any) {
    const lastFetchedAt = Number(meta?.lastFetchedAt || 0);
    return lastFetchedAt > 0 && Date.now() - lastFetchedAt < TelegramService.MESSAGE_CACHE_FRESH_MS;
  }

  private useTdlibOnly() {
    return runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib;
  }

  private enqueueThumbnailRequest<T>(
    key: string,
    chatId: any,
    messageId: any,
    task: () => Promise<T>,
    priority: 'visible' | 'background' = 'visible',
    cancelResult: T,
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const run = () => {
        this.activeThumbnailDownloads++;
        task()
          .then(resolve, reject)
          .finally(() => {
            this.activeThumbnailDownloads = Math.max(0, this.activeThumbnailDownloads - 1);
            this.drainThumbnailQueue();
          });
      };
      const cancel = () => resolve(cancelResult);
      const queueItem = { key, chatId: String(chatId), messageId: Number(messageId), run, cancel, priority };

      if (priority === 'visible') {
        this.thumbnailQueue.unshift(queueItem);
      } else {
        this.thumbnailQueue.push(queueItem);
      }
      this.drainThumbnailQueue();
    });
  }

  private promoteQueuedThumbnail(key: string) {
    const existingIndex = this.thumbnailQueue.findIndex(item => item.key === key);
    if (existingIndex < 0) return;

    const [existing] = this.thumbnailQueue.splice(existingIndex, 1);
    existing.priority = 'visible';
    this.thumbnailQueue.unshift(existing);
  }

  private drainThumbnailQueue() {
    while (
      this.activeThumbnailDownloads < TelegramService.MAX_THUMBNAIL_DOWNLOADS &&
      this.thumbnailQueue.length > 0
    ) {
      const next = this.thumbnailQueue.shift();
      next?.run();
    }
  }

  cancelQueuedThumbnails({ activeChatId, keepMessageIds }: { activeChatId?: any; keepMessageIds?: Iterable<number> } = {}) {
    const activeKey = activeChatId == null ? null : String(activeChatId);
    const keepIds = keepMessageIds ? new Set(Array.from(keepMessageIds, id => Number(id))) : null;
    const keep: typeof this.thumbnailQueue = [];

    for (const item of this.thumbnailQueue) {
      const keepChat = activeKey && item.chatId === activeKey;
      const keepVisible = keepIds?.has(item.messageId) ?? false;
      const shouldKeep = keepChat && (keepIds ? keepVisible : item.priority === 'visible');
      if (shouldKeep) {
        keep.push(item);
      } else {
        item.cancel();
      }
    }

    this.thumbnailQueue = keep;
  }

  private enqueueAvatarRequest<T>(
    key: string,
    task: () => Promise<T>,
    priority: 'visible' | 'background' = 'visible',
    cancelResult: T,
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const run = () => {
        this.activeAvatarDownloads++;
        task()
          .then(resolve, reject)
          .finally(() => {
            this.activeAvatarDownloads = Math.max(0, this.activeAvatarDownloads - 1);
            this.drainAvatarQueue();
          });
      };
      const cancel = () => resolve(cancelResult);
      const queueItem = { key, run, cancel, priority };

      if (priority === 'visible') {
        this.avatarQueue.unshift(queueItem);
      } else {
        this.avatarQueue.push(queueItem);
      }
      this.drainAvatarQueue();
    });
  }

  private drainAvatarQueue() {
    while (
      this.activeAvatarDownloads < TelegramService.MAX_AVATAR_DOWNLOADS &&
      this.avatarQueue.length > 0
    ) {
      const next = this.avatarQueue.shift();
      next?.run();
    }
  }

  cancelQueuedAvatarsExcept(keepIds: Iterable<any> = []) {
    const keepKeys = new Set(Array.from(keepIds, id => `avatar_${id}`));
    const keep: typeof this.avatarQueue = [];

    for (const item of this.avatarQueue) {
      if (item.priority === 'visible' || keepKeys.has(item.key)) {
        keep.push(item);
      } else {
        item.cancel();
      }
    }

    this.avatarQueue = keep;
  }

  private async getEntityCached(chatId: any) {
    throw new Error('Entity cache legado indisponível. Use TDLib nativo.');
  }

  private pruneEntityCache() {
    // No-op: entity cache legado indisponível.
  }

  invalidateSharedMedia(chatId: any) {
    const prefix = `${chatId}:`;
    for (const key of Array.from(this.sharedMediaCache.keys())) {
      if (key.startsWith(prefix)) this.sharedMediaCache.delete(key);
    }
  }

  private enqueueFullMediaRequest<T>(
    key: string,
    chatId: any,
    messageId: any,
    task: () => Promise<T>,
    priority: 'user' | 'background' = 'user',
    cancelResult: T,
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const run = () => {
        this.activeFullMediaDownloads++;
        task()
          .then(resolve, reject)
          .finally(() => {
            this.activeFullMediaDownloads = Math.max(0, this.activeFullMediaDownloads - 1);
            this.drainFullMediaQueue();
          });
      };
      const cancel = () => resolve(cancelResult);
      const queueItem = { key, chatId: String(chatId), messageId: Number(messageId), run, cancel, priority };

      if (priority === 'user') {
        this.fullMediaQueue.unshift(queueItem);
      } else {
        this.fullMediaQueue.push(queueItem);
      }
      this.drainFullMediaQueue();
    });
  }

  private drainFullMediaQueue() {
    while (
      this.activeFullMediaDownloads < TelegramService.MAX_FULL_MEDIA_DOWNLOADS &&
      this.fullMediaQueue.length > 0
    ) {
      const next = this.fullMediaQueue.shift();
      next?.run();
    }
  }

  cancelQueuedFullMediaExceptChat(activeChatId: any, keepMessageIds?: Iterable<number>) {
    const activeKey = activeChatId == null ? null : String(activeChatId);
    const keepIds = keepMessageIds ? new Set(Array.from(keepMessageIds, id => Number(id))) : null;
    const keep: typeof this.fullMediaQueue = [];

    for (const item of this.fullMediaQueue) {
      const keepChat = activeKey && item.chatId === activeKey;
      const keepVisible = keepIds?.has(item.messageId) ?? false;
      const shouldKeep = keepChat && (item.priority === 'user' || !keepIds || keepVisible);
      if (shouldKeep) {
        keep.push(item);
      } else {
        item.cancel();
      }
    }

    this.fullMediaQueue = keep;

    for (const [key, item] of Array.from(this.activeNativeMediaDownloads.entries())) {
      if (item.priority !== 'background') continue;
      if (activeKey && item.chatId === activeKey && (!keepIds || keepIds.has(item.messageId))) continue;
      this.activeNativeMediaDownloads.delete(key);
      void this.tdlibBridge.cancelNativeMedia({
        chatId: item.chatId,
        messageId: item.messageId,
      }).catch(debugWarn);
    }
  }

  cancelBackgroundMessageMedia({ chatId, messageId }: any) {
    const key = `${chatId}_${messageId}`;
    const keepFull: typeof this.fullMediaQueue = [];
    for (const item of this.fullMediaQueue) {
      if (item.key === key && item.priority === 'background') {
        item.cancel();
      } else {
        keepFull.push(item);
      }
    }
    this.fullMediaQueue = keepFull;

    const keepThumbs: typeof this.thumbnailQueue = [];
    for (const item of this.thumbnailQueue) {
      if (item.key === key && item.priority === 'background') {
        item.cancel();
      } else {
        keepThumbs.push(item);
      }
    }
    this.thumbnailQueue = keepThumbs;

    const active = this.activeNativeMediaDownloads.get(key);
    if (active?.priority === 'background') {
      this.activeNativeMediaDownloads.delete(key);
      void this.tdlibBridge.cancelNativeMedia({ chatId, messageId }).catch(debugWarn);
    }
  }

  async cancelMessageMediaDownload({ chatId, messageId }: any) {
    const key = `${chatId}_${messageId}`;
    const keepFull: typeof this.fullMediaQueue = [];

    for (const item of this.fullMediaQueue) {
      if (item.key === key) {
        item.cancel();
      } else {
        keepFull.push(item);
      }
    }
    this.fullMediaQueue = keepFull;

    this.activeNativeMediaDownloads.delete(key);
    await this.tdlibBridge.cancelNativeMedia({ chatId, messageId }).catch(debugWarn);
    this.emitMediaProgress({
      chatId,
      messageId,
      progress: 0,
      downloadedBytes: undefined,
      totalBytes: undefined,
      stage: 'canceled',
    });
    return { success: true, canceled: true };
  }

  async checkAuth() {
    if (this.useTdlibOnly()) {
      try {
        await this.tdlibInit();
        const status: any = await this.tdlibStatus();
        const isAuthorized = Boolean(status?.ready);
        if (isAuthorized) appStorage.set(this.tdlibSessionKey, 'ready');
        else appStorage.remove(this.tdlibSessionKey);
        return { isAuthorized };
      } catch (e) {
        debugWarn("TDLib auth check failed:", e);
        return { isAuthorized: false };
      }
    }
    return { isAuthorized: false };
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

  async tdlibCacheMessageMedia({ chatId, messageId }: any) {
    return this.tdlibBridge.cacheMessageMedia({ chatId, messageId });
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
    if (this.useTdlibOnly()) {
      try {
        await this.tdlibInit();
        const res: any = await this.tdlibSetPhone(phoneNumber);
        if (res?.success !== false) {
          return { success: true, phoneCodeHash: 'tdlib' };
        }
        return { success: false, error: res?.error || 'Falha ao enviar telefone para TDLib.' };
      } catch (error: any) {
        debugWarn("Error sending TDLib code:", error);
        return { success: false, error: error?.message || String(error) };
      }
    }
    return { success: false, error: 'TDLib nativo indisponível.' };
  }

  async signIn(phoneNumber: string, phoneCodeHash: string, phoneCode: string) {
    if (this.useTdlibOnly()) {
      try {
        const codeRes: any = await this.tdlibCheckCode(phoneCode);
        if (codeRes?.ready || codeRes?.state === 'ready') {
          appStorage.set(this.tdlibSessionKey, 'ready');
          return { success: true };
        }
        if (codeRes?.state === 'wait_password') {
          return { success: false, error: 'Esta conta exige senha 2FA. Use Configurações > TDLib para validar a senha por enquanto.' };
        }
        return { success: false, error: codeRes?.error || `TDLib aguardando estado: ${codeRes?.state || 'desconhecido'}` };
      } catch (error: any) {
        return { success: false, error: error.message };
      }
    }
    return { success: false, error: 'TDLib nativo indisponível.' };
  }

  async getDialogs() {
    const fakeDialogs = twitterFakeDialogs();
    if (this.skipLogin) return { success: true, dialogs: fakeDialogs };

    if (this.useTdlibOnly()) {
      try {
        await this.tdlibInit();
        const nativeRes: any = await this.tdlibBridge.getChats(100);
        if (nativeRes?.success) {
          return {
            success: true,
            dialogs: [
              ...fakeDialogs,
              ...(nativeRes.dialogs || []),
            ],
          };
        }
        return { success: false, error: nativeRes?.error || 'TDLib não retornou chats.' };
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) };
      }
    }

    return { success: true, dialogs: fakeDialogs };
  }

  async getCachedMessages({ chatId, limit = 50, topicId = undefined }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) return this.getMessages({ chatId, limit, topicId });

    const cached = await mediaCache.getMessages(messageCacheKey(chatId, topicId));
    const cacheMeta = await mediaCache.getMessagePageMeta(messageCacheKey(chatId, topicId));
    const ordered = [...cached].sort((a, b) => Number(a.id) - Number(b.id));
    const page = ordered.slice(-limit);
    const newestMessageDate = ordered.reduce((newest, message) => {
      const date = Number(message?.date || 0);
      return date > newest ? date : newest;
    }, 0);
    return {
      success: true,
      messages: page,
      hasMore: ordered.length > page.length,
      oldestMessageId: page[0]?.id ?? null,
      fromCache: true,
      cacheMeta,
      isFresh: this.isMessageCacheFresh(cacheMeta),
      newestMessageDate,
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

    if (this.useTdlibOnly()) {
      try {
        const cacheKey = messageCacheKey(chatId, topicId);
        if (!offsetId && !refresh) {
          const cached = await this.getCachedMessages({ chatId, limit, topicId });
          const hasMissingSenderNames = cached.messages?.some((message: any) => !message.out && !message.senderName);
          if (cached.messages?.length && cached.isFresh && !hasMissingSenderNames) return cached;
        }

        await this.tdlibInit();
        const nativeRes: any = await this.tdlibBridge.getMessages({ chatId, limit, offsetId, topicId });
        if (nativeRes?.success) {
          if (!offsetId && Array.isArray(nativeRes.messages)) {
            await mediaCache.saveMessages(cacheKey, nativeRes.messages, { lastFetchedAt: Date.now() });
          }
          return nativeRes;
        }
        return { success: false, messages: [], hasMore: false, oldestMessageId: null, error: nativeRes?.error || 'TDLib não retornou mensagens.' };
      } catch (error) {
        return { success: false, messages: [], hasMore: false, oldestMessageId: null, error: error instanceof Error ? error.message : String(error) };
      }
    }

    return { success: false, messages: [], hasMore: false, oldestMessageId: null, error: 'TDLib nativo indisponível.' };
  }

  async getAvatar(id: any, options: { priority?: 'visible' | 'background' } = {}) {
    const fakeChat = this.getTwitterFakeChat(id);
    if (fakeChat?.avatarUrl) return { success: true, dataUrl: fakeChat.avatarUrl };

    const cacheKey = `avatar_${id}`;
    const cachedInfo = await mediaCache.getCacheItemInfo(cacheKey);
    const settings = await mediaCache.getCacheSettings();
    const refreshMs = Math.max(1, settings.avatarRefreshHours || 24) * 60 * 60 * 1000;
    const isFresh = cachedInfo?.addedAt && Date.now() - cachedInfo.addedAt < refreshMs;
    const cachedUrl = await mediaCache.getMedia(cacheKey, 'image/jpeg');
    if (cachedUrl && isFresh) return { success: true, dataUrl: cachedUrl };
    const cachedNativeUrl = await this.getCachedNativeFileUrl(cacheKey);
    if (cachedNativeUrl && isFresh) return { success: true, dataUrl: cachedNativeUrl };

    if (this.useTdlibOnly()) {
      const priority = options.priority || 'visible';
      const request = this.enqueueAvatarRequest(
        cacheKey,
        () => this.getAvatarInner(id, cacheKey),
        priority,
        cachedUrl || cachedNativeUrl
          ? { success: true, dataUrl: cachedUrl || cachedNativeUrl, canceled: true }
          : { success: false, canceled: true }
      );
      return request;
    }
    return cachedUrl
      ? { success: true, dataUrl: cachedUrl }
      : { success: false, error: 'Avatar nativo via TDLib ainda não disponível.' };
  }

  private async getAvatarInner(id: any, cacheKey: string, fallbackUrl?: string | null) {
    if (fallbackUrl) return { success: true, dataUrl: fallbackUrl };
    try {
      await this.tdlibInit();
      const nativeRes: any = await this.tdlibBridge.downloadChatAvatar(id);
      const nativeFilePath = nativeRes?.filePath || nativeRes?.file_path;
      if (nativeRes?.success && nativeFilePath) {
        const filePath = convertFileSrc(nativeFilePath);
        mediaCache.cacheUrlInMemory(cacheKey, filePath);
        await mediaCache.saveNativeFileReference(cacheKey, nativeFilePath, 'image/jpeg');
        return { success: true, dataUrl: filePath };
      }
      return { success: false, error: nativeRes?.error || 'TDLib não retornou avatar.' };
    } catch (error) {
      return { success: false, error: error instanceof Error ? error.message : String(error) };
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
    if (this.useTdlibOnly()) {
      try {
        await this.tdlibInit();
        const nativeRes: any = await this.tdlibBridge.getForumTopics(chatId, 100);
        if (nativeRes?.success) return nativeRes;
        return { success: false, topics: [], error: nativeRes?.error || 'TDLib não retornou tópicos.' };
      } catch (error) {
        return { success: false, topics: [], error: error instanceof Error ? error.message : String(error) };
      }
    }

    return { success: false, topics: [], error: 'TDLib nativo indisponível.' };
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

  private async getCachedNativeFileUrl(cacheKey: string) {
    const info = await mediaCache.getCacheItemInfo(cacheKey);
    const nativeFilePath = info?.nativeFilePath;
    if (!nativeFilePath) return null;

    try {
      const exists = await this.pathExists(nativeFilePath);
      if (!exists) return null;
    } catch {
      // If path checks are unavailable, let the webview try the converted URL.
    }

    const url = convertFileSrc(nativeFilePath);
    mediaCache.cacheUrlInMemory(cacheKey, url);
    return url;
  }

  private async cacheMessageMediaWithTdlib({ chatId, messageId }: any) {
    try {
      await this.tdlibInit();
      const nativeRes: any = await this.tdlibBridge.cacheMessageMedia({ chatId, messageId });
      const nativeFilePath = nativeRes?.filePath || nativeRes?.file_path;
      if (!nativeRes?.success || !nativeFilePath) return nativeRes || { success: false };

      const fileName = nativeRes.fileName || nativeRes.file_name || undefined;
      const cacheKey = this.fullMediaCacheKey(chatId, messageId);
      const mimeType = nativeRes.mimeType
        || nativeRes.mime_type
        || this.mimeTypeFromFileName(fileName)
        || 'application/octet-stream';
      await mediaCache.saveNativeFileReference(cacheKey, nativeFilePath, mimeType);
      await mediaCache.saveMediaAssetMeta(cacheKey, {
        state: 'native',
        totalBytes: nativeRes.size || undefined,
        downloadedBytes: nativeRes.size || undefined,
        mimeType,
        fileName,
        nativeFilePath,
        completedAt: Date.now(),
      });

      const filePath = convertFileSrc(nativeFilePath);
      mediaCache.cacheUrlInMemory(cacheKey, filePath);
      this.emitMediaProgress({
        chatId,
        messageId,
        progress: 100,
        downloadedBytes: nativeRes.size || undefined,
        totalBytes: nativeRes.size || undefined,
        stage: 'ready',
      });
      return { success: true, filePath, nativeFilePath, fileName, size: nativeRes.size, mimeType };
    } catch (error: any) {
      return { success: false, error: error?.message || String(error) };
    }
  }

  private nativeMediaResultToFile(result: any) {
    const playbackUrl = result?.url || result?.playbackUrl || result?.playback_url;
    const nativeFilePath = result?.nativeFilePath || result?.native_file_path || result?.filePath || result?.file_path || playbackUrl;
    if (!result?.success || !nativeFilePath) return null;
    const filePath = typeof playbackUrl === 'string' && /^[a-z][a-z0-9+.-]*:\/\//i.test(playbackUrl)
      ? playbackUrl
      : convertFileSrc(nativeFilePath);
    return {
      success: true,
      filePath,
      nativeFilePath,
      fileName: result.fileName || result.file_name,
      size: result.totalBytes || result.total_bytes || result.size,
      mimeType: result.mimeType || result.mime_type,
      cacheState: result.state || result.cacheState || result.cache_state || 'complete',
    };
  }

  private async getNativeCachedMessageMediaFile({ chatId, messageId }: any) {
    if (!runtimeCapabilities.isTauri || !runtimeCapabilities.supportsTdlib || this.findTwitterFakeMessage(chatId, messageId)) {
      return null;
    }

    try {
      const meta: any = await this.tdlibBridge.getNativeMediaMeta({ chatId, messageId });
      const file = this.nativeMediaResultToFile(meta);
      if (file && (meta.state === 'complete' || meta.state === 'native')) {
        await mediaCache.saveNativeFileReference(this.fullMediaCacheKey(chatId, messageId), file.nativeFilePath, file.mimeType);
        return file;
      }
    } catch (error) {
      debugWarn('[TelegramService] Native media meta unavailable:', error);
    }
    return null;
  }

  private async ensureNativeMessageMediaCached({ chatId, messageId, priority = 'user' }: any) {
    if (!runtimeCapabilities.isTauri || !runtimeCapabilities.supportsTdlib || this.findTwitterFakeMessage(chatId, messageId)) {
      return null;
    }

    const requestKey = `${chatId}_${messageId}`;
    try {
      this.activeNativeMediaDownloads.set(requestKey, {
        chatId: String(chatId),
        messageId: Number(messageId),
        priority: priority === 'background' ? 'background' : 'user',
      });
      const nativeRes: any = await this.tdlibBridge.ensureNativeMediaCached({ chatId, messageId, priority });
      const file = this.nativeMediaResultToFile(nativeRes);
      if (file) {
        await mediaCache.saveNativeFileReference(this.fullMediaCacheKey(chatId, messageId), file.nativeFilePath, file.mimeType);
        await mediaCache.saveMediaAssetMeta(this.fullMediaCacheKey(chatId, messageId), {
          state: 'native',
          totalBytes: file.size || undefined,
          downloadedBytes: file.size || undefined,
          mimeType: file.mimeType,
          fileName: file.fileName,
          nativeFilePath: file.nativeFilePath,
          completedAt: Date.now(),
        });
        return file;
      }
      if (nativeRes?.state === 'partial' && !nativeRes?.error) {
        return { success: false, canceled: true, error: 'Download cancelado.' };
      }
      if (nativeRes?.error) debugWarn('[TelegramService] Native media cache failed:', nativeRes.error);
    } catch (error) {
      debugWarn('[TelegramService] Native media cache command failed:', error);
    } finally {
      this.activeNativeMediaDownloads.delete(requestKey);
    }
    return null;
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
      debugWarn('Error getting sender for splitting media:', e);
      if (message.senderId) senderName = `ID_${message.senderId.toString()}`;
    }
    return sanitizeForFolderName(senderName);
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
      if (this.useTdlibOnly()) {
        return { success: false, error: error instanceof Error ? error.message : String(error), native: true };
      }
      debugWarn('[TelegramService] TDLib mass download unavailable:', error);
      return null;
    }
  }

  async startDownload({ chatId, folderPath, topic, splitByUser, splitByAlbum = false, chatMeta }: any) {
    try {
      this.activeDownloadAborted = false;
      this.saveMultipleAborted = false;

      const fakeChat = this.getTwitterFakeChat(chatId);
      if (fakeChat) {
        return this.startFakeChatDownload({ chatId, folderPath, fakeChat, chatMeta });
      }

      const nativeResult = await this.tryStartTdlibDownload({ chatId, folderPath, topic, splitByUser, splitByAlbum, chatMeta });
      if (nativeResult) return nativeResult;
      return {
        success: false,
        error: splitByAlbum
          ? 'Separação por álbum ainda não está disponível no TDLib nativo.'
          : 'TDLib nativo indisponível para este download.',
      };
    } catch (error: any) {
      debugWarn('Download error:', error);
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
        await this.fileStorage.downloadUrlToFile(message.url!, filePath, (payload) => {
          const percent = payload.percent;
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
        debugWarn(`Failed to download ${safeName}:`, err);
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
  async getMessageMedia({ chatId, messageId, priority = 'visible' }: any) {
    const requestKey = `${chatId}_${messageId}`;
    const pendingRequest = this.mediaThumbRequests.get(requestKey);
    if (pendingRequest) {
      if (priority === 'visible') this.promoteQueuedThumbnail(requestKey);
      return pendingRequest;
    }

    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      return { success: true, filePath: fakeMessage.thumbnailUrl || null };
    }

    const cacheKey = `media_${chatId}_${messageId}_thumb`;
    const cachedUrl = await mediaCache.getMedia(cacheKey, 'image/jpeg');
    if (cachedUrl) return { success: true, filePath: cachedUrl };
    const cachedNativeUrl = await this.getCachedNativeFileUrl(cacheKey);
    if (cachedNativeUrl) return { success: true, filePath: cachedNativeUrl };
    if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      try {
        const meta: any = await this.tdlibBridge.getNativeMediaMeta({ chatId, messageId });
        const nativeThumbPath = meta?.thumbnailPath || meta?.thumbnail_path;
        if (nativeThumbPath) {
          const filePath = convertFileSrc(nativeThumbPath);
          mediaCache.cacheUrlInMemory(cacheKey, filePath);
          await mediaCache.saveNativeFileReference(cacheKey, nativeThumbPath, 'image/jpeg');
          return { success: true, filePath };
        }

        const nativeMediaPath = meta?.nativeFilePath || meta?.native_file_path;
        const mimeType = meta?.mimeType || meta?.mime_type;
        if (nativeMediaPath && typeof mimeType === 'string' && mimeType.startsWith('image/')) {
          const filePath = convertFileSrc(nativeMediaPath);
          mediaCache.cacheUrlInMemory(cacheKey, filePath);
          await mediaCache.saveNativeFileReference(cacheKey, nativeMediaPath, mimeType);
          return { success: true, filePath };
        }
      } catch {
        // Native metadata is best-effort before asking TDLib for a thumbnail.
      }
    }
    const pendingAfterCacheCheck = this.mediaThumbRequests.get(requestKey);
    if (pendingAfterCacheCheck) return pendingAfterCacheCheck;

    const request = this.enqueueThumbnailRequest(
      requestKey,
      chatId,
      messageId,
      () => this.getMessageMediaInner({ chatId, messageId }),
      priority,
      { success: false, canceled: true }
    );
    this.mediaThumbRequests.set(requestKey, request);
    try {
      return await request;
    } finally {
      this.mediaThumbRequests.delete(requestKey);
    }
  }

  private async getMessageMediaInner({ chatId, messageId }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      return { success: true, filePath: fakeMessage.thumbnailUrl || null };
    }

    if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      try {
        await this.tdlibInit();
        const nativeRes: any = await this.tdlibBridge.downloadMessageThumbnail({ chatId, messageId });
        if (nativeRes?.success && nativeRes.filePath) {
          const cacheKey = `media_${chatId}_${messageId}_thumb`;
          const filePath = convertFileSrc(nativeRes.filePath);
          mediaCache.cacheUrlInMemory(cacheKey, filePath);
          await mediaCache.saveNativeFileReference(cacheKey, nativeRes.filePath, 'image/jpeg');
          return { success: true, filePath };
        }
        return { success: false, error: nativeRes?.error || 'TDLib não retornou thumbnail.' };
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) };
      }
    }

    return { success: false, error: 'TDLib nativo indisponível para carregar thumbnail.' };
  }

  async getCachedMessageMediaFile({ chatId, messageId, mimeType }: any) {
    const nativeCached = await this.getNativeCachedMessageMediaFile({ chatId, messageId });
    if (nativeCached) return nativeCached;

    const cacheKey = this.fullMediaCacheKey(chatId, messageId);
    const cachedNativeUrl = await this.getCachedNativeFileUrl(cacheKey);
    if (cachedNativeUrl) return { success: true, filePath: cachedNativeUrl };
    const cachedUrl = await mediaCache.getMedia(cacheKey, mimeType);
    if (cachedUrl) return { success: true, filePath: cachedUrl };
    return { success: false };
  }

  async isMessageMediaFileCached({ chatId, messageId, mimeType }: any) {
    const cached = await this.getCachedMessageMediaFile({ chatId, messageId, mimeType });
    return Boolean(cached?.success);
  }

  async ensureMessageMediaCached({ chatId, messageId, priority = 'user', mimeType }: any) {
    const requestKey = `${chatId}_${messageId}`;
    const pendingRequest = this.mediaFileRequests.get(requestKey);
    if (pendingRequest) return pendingRequest;

    const cacheKey = this.fullMediaCacheKey(chatId, messageId);
    const cachedNativeUrl = await this.getCachedNativeFileUrl(cacheKey);
    if (cachedNativeUrl) return { success: true, filePath: cachedNativeUrl };
    const cachedUrl = await mediaCache.getMedia(cacheKey, mimeType);
    if (cachedUrl) return { success: true, filePath: cachedUrl };

    const request = this.enqueueFullMediaRequest(
      requestKey,
      chatId,
      messageId,
      () => this.getMessageMediaFileInner({ chatId, messageId, priority, mimeType }),
      priority,
      { success: false, canceled: true, error: 'Download cancelado antes de iniciar.' }
    );
    this.mediaFileRequests.set(requestKey, request);
    try {
      return await request;
    } finally {
      this.mediaFileRequests.delete(requestKey);
    }
  }

  async getMessageMediaFile(opts: any) {
    return this.ensureMessageMediaCached(opts);
  }

  prefetchMessageMediaFile({ chatId, messageId }: any) {
    if (this.findTwitterFakeMessage(chatId, messageId)) return;

    const requestKey = `${chatId}_${messageId}`;
    if (this.mediaFileRequests.has(requestKey)) return;

    void this.getMessageMediaFile({ chatId, messageId, priority: 'background' }).then((res: any) => {
      if (!res?.success && !res?.canceled) {
        debugWarn(`[TelegramService] prefetchMessageMediaFile failed for ${chatId}/${messageId}: ${res?.error || 'unknown error'}`);
      }
    }).catch((error: any) => {
      debugWarn(`[TelegramService] prefetchMessageMediaFile failed for ${chatId}/${messageId}: ${error?.message || error}`);
    });
  }

  private async getMessageMediaFileInner({ chatId, messageId, priority = 'user', mimeType }: any) {
    const cacheKey = this.fullMediaCacheKey(chatId, messageId);
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (fakeMessage) {
      if (!fakeMessage.url) return { success: false };
      return this.downloadTwitterFakeMedia(chatId, messageId, fakeMessage);
    }

    try {
      const nativeCached = await this.ensureNativeMessageMediaCached({ chatId, messageId, priority });
      if (nativeCached) return nativeCached;

      const cachedNativeUrl = await this.getCachedNativeFileUrl(cacheKey);
      if (cachedNativeUrl) return { success: true, filePath: cachedNativeUrl };
      const cachedUrl = await mediaCache.getMedia(cacheKey, mimeType);
      if (cachedUrl) return { success: true, filePath: cachedUrl };

      const nativeRes = await this.cacheMessageMediaWithTdlib({ chatId, messageId });
      if (nativeRes?.success) return nativeRes;
      return { success: false, error: nativeRes?.error || 'TDLib não retornou mídia.' };
    } catch (e: any) {
      return { success: false, error: e.message };
    }
  }

  async prepareMessageMediaPlayback({ chatId, messageId, mode = 'inline' }: any) {
    const cacheKey = this.fullMediaCacheKey(chatId, messageId);
    if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib && !this.findTwitterFakeMessage(chatId, messageId)) {
      try {
        const nativeRes: any = await this.tdlibBridge.prepareNativeMediaPlayback({ chatId, messageId, mode });
        const file = this.nativeMediaResultToFile(nativeRes);
        debugLog('[TelegramService] Native playback response', {
          chatId,
          messageId,
          mode,
          nativeRes,
          normalized: file,
        });
        if (file) {
          await mediaCache.saveNativeFileReference(cacheKey, file.nativeFilePath, file.mimeType);
          return {
            success: true,
            source: 'native',
            cacheState: file.cacheState || 'native',
            playbackUrl: file.filePath,
            filePath: file.filePath,
            nativeFilePath: file.nativeFilePath,
            mimeType: file.mimeType,
            totalBytes: file.size,
          };
        }
        if ((nativeRes?.state === 'partial' || nativeRes?.cacheState === 'partial') && !nativeRes?.error) {
          return { success: false, canceled: true, cacheState: 'partial', error: 'Download cancelado.' };
        }
        return { success: false, error: nativeRes?.error || 'TDLib não preparou a reprodução.' };
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) };
      }
    }

    const cached = await this.getCachedMessageMediaFile({ chatId, messageId, mimeType: 'video/mp4' });
    if (cached.success && cached.filePath) {
      return {
        success: true,
        source: 'cache',
        cacheState: 'complete',
        playbackUrl: cached.filePath,
        filePath: cached.filePath,
      };
    }

    if (this.useTdlibOnly()) {
      const nativeRes = await this.ensureMessageMediaCached({ chatId, messageId, priority: 'user' });
      if (nativeRes?.success && nativeRes.filePath) {
        return {
          success: true,
          source: 'native',
          cacheState: 'native',
          playbackUrl: nativeRes.filePath,
          filePath: nativeRes.filePath,
        };
      }
      return nativeRes;
    }

    const fullRes = await this.ensureMessageMediaCached({ chatId, messageId, priority: mode === 'background' ? 'background' : 'user' });
    if (fullRes?.success && fullRes.filePath) {
      return {
        success: true,
        source: 'cache',
        cacheState: 'complete',
        playbackUrl: fullRes.filePath,
        filePath: fullRes.filePath,
      };
    }
    return fullRes;
  }

  async getMessageMediaStream(opts: any): Promise<{ success: boolean; streamUrl?: string; filePath?: string; error?: any }> {
    const res = await this.prepareMessageMediaPlayback(opts);
    if (res.success && res.playbackUrl) return { ...res, streamUrl: res.playbackUrl };
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

    return this.tdlibBridge.searchUserMedia({ chatId, userId, limit });
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
      debugWarn('Failed to select folder:', e);
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

      const total = messageIds.length;
      let downloadedCount = 0;
      let failedCount = 0;
      const runId = Date.now();
      const activeDownloadIds = new Map<number, string>();
      const unsubscribe = this.onMediaProgress((data: any) => {
        if (String(data.chatId) !== String(chatId)) return;
        const downloadId = activeDownloadIds.get(Number(data.messageId));
        if (!downloadId) return;
        downloadService.updateDownload(downloadId, { progress: data.progress || 0 });
      });

      try {
        for (const rawMessageId of messageIds) {
          if (this.saveMultipleAborted) break;
          const messageId = Number(rawMessageId);
          let filename = sanitizeForFilename(`media_${messageId}`);
          try {
            const meta: any = await this.tdlibBridge.getNativeMediaMeta({ chatId, messageId });
            if (meta?.fileName) filename = sanitizeForFilename(meta.fileName);
          } catch {
            // Metadata is best-effort; TDLib save can still resolve the final name.
          }

          const downloadId = `telegram_bulk_${chatId}_${messageId}_${runId}`;
          activeDownloadIds.set(messageId, downloadId);
          downloadService.addDownload({
            id: downloadId,
            chatId,
            messageId,
            fileName: filename,
            progress: 0,
            status: 'downloading',
            platform: 'telegram',
          });
          this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'downloading' });

          try {
            const cached: any = await this.tdlibBridge.ensureNativeMediaCached({ chatId, messageId, priority: 'user' });
            if (!cached?.success) {
              throw new Error(cached?.error || 'Falha ao preparar mídia.');
            }
            if (cached.fileName) filename = sanitizeForFilename(cached.fileName);
            const filePath = joinPath(folderPath, filename);
            downloadService.updateDownload(downloadId, { fileName: filename, filePath });

            const res: any = await this.tdlibBridge.saveNativeMedia({ chatId, messageId, destinationPath: filePath });
            if (this.saveMultipleAborted || res?.canceled) break;
            if (res?.success) {
              downloadedCount++;
              downloadService.updateDownload(downloadId, {
                status: 'completed',
                progress: 100,
                fileName: res.fileName || filename,
                filePath: res.filePath || filePath,
              });
            } else {
              failedCount++;
              downloadService.updateDownload(downloadId, { status: 'failed', error: res?.error || 'Falha ao salvar mídia.' });
            }
          } catch (error: any) {
            if (this.saveMultipleAborted || String(error).includes('STOP_ABORTED')) break;
            failedCount++;
            downloadService.updateDownload(downloadId, { status: 'failed', error: error?.message || String(error) });
          } finally {
            activeDownloadIds.delete(messageId);
          }

          this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount, currentFile: filename, status: 'progress' });
          await nextFrame();
        }
      } finally {
        unsubscribe();
      }

      this.emitSaveMultipleProgress({
        chatId,
        total,
        downloaded: downloadedCount,
        currentFile: this.saveMultipleAborted ? 'Cancelado pelo usuário' : 'Concluído!',
        status: 'completed',
      });

      return { success: true, downloadedCount, failedCount, aborted: this.saveMultipleAborted };
    } catch (error: any) {
      debugWarn('Error in saveMultipleMediaFiles:', error);
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
        await this.fileStorage.downloadUrlToFile(message.url, filePath, (payload) => {
          const percent = payload.percent;
          downloadService.updateDownload(downloadId, { progress: percent });
          this.emitSaveMultipleProgress({ chatId, total, downloaded: downloadedCount + (percent / 100), currentFile: `${filename} (${percent}%)`, status: 'progress' });
        });
        downloadedCount++;
        downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
      } catch (err) {
        debugWarn(`Error downloading fake media for message ${messageId}:`, err);
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

  async sendMedia(opts: any) {
    const res = await this.tdlibBridge.sendMedia(opts);
    if (res?.success) this.emitSendProgress({ chatId: opts.chatId, progress: 100 });
    return res;
  }

  async sendMessage(opts: any) {
    const { text } = opts;
    if (!String(text || '').trim()) return { success: false, error: 'Mensagem vazia.' };
    return this.tdlibBridge.sendMessage(opts);
  }
  async forwardMessage(opts: any) {
    const { messageId } = opts;
    if (!messageId) return { success: false, error: 'Mensagem inválida.' };
    return this.tdlibBridge.forwardMessage(opts);
  }
  async createTopic(opts: any) { return { success: true }; }
  async getOriginalMessage(opts: any) { return { success: true, message: null }; }
  async sendReaction(opts: any) { return { success: true }; }
  getPathForFile(file: any) { return ''; }
  async joinChat(chatId: any) { return { success: true }; }
  async saveMessageMediaFile({ chatId, messageId, downloadMeta = {}, saveAs = false }: any) {
    const fakeMessage = this.findTwitterFakeMessage(chatId, messageId);
    if (!runtimeCapabilities.isTauri && !fakeMessage) return { success: false, error: 'TDLib nativo indisponível.' };
    let suggestedName = `media_${messageId}`;

    if (fakeMessage) {
      suggestedName = sanitizeForFilename(`media_${messageId}${fakeMessage.isVideo ? '.mp4' : '.jpg'}`);
    } else if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      try {
        const meta: any = await this.tdlibBridge.getNativeMediaMeta({ chatId, messageId });
        if (meta?.fileName) suggestedName = sanitizeForFilename(meta.fileName);
      } catch {
        // Native metadata is best-effort before opening the save dialog.
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

      if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib && !fakeMessage) {
        try {
          const nativeSave: any = await this.tdlibBridge.saveNativeMedia({
            chatId,
            messageId,
            destinationPath: saveAs && saveAsPath ? saveAsPath : null,
          });

          if (nativeSave?.success && nativeSave.filePath) {
            unsub();
            downloadService.updateDownload(id, {
              status: 'completed',
              progress: 100,
              fileName: nativeSave.fileName || basename(nativeSave.filePath),
              filePath: nativeSave.filePath,
            });
            return { success: true, filePath: nativeSave.filePath };
          }

          if (nativeSave?.error === 'STOP_ABORTED' || nativeSave?.canceled) {
            unsub();
            downloadService.updateDownload(id, { status: 'canceled', progress: 0, error: 'Cancelado pelo usuário.' });
            return { success: false, canceled: true };
          }

          if (this.useTdlibOnly()) {
            unsub();
            const error = nativeSave?.error || 'TDLib não salvou a mídia.';
            downloadService.updateDownload(id, { status: 'failed', error });
            return { success: false, error };
          }

          debugWarn('[TelegramService] Native Save failed:', nativeSave?.error || nativeSave);
        } catch (error) {
          if (String(error).includes('STOP_ABORTED')) {
            unsub();
            downloadService.updateDownload(id, { status: 'canceled', progress: 0, error: 'Cancelado pelo usuário.' });
            return { success: false, canceled: true };
          }
          if (this.useTdlibOnly()) {
            unsub();
            const message = error instanceof Error ? error.message : String(error);
            downloadService.updateDownload(id, { status: 'failed', error: message });
            return { success: false, error: message };
          }
          debugWarn('[TelegramService] Native Save command failed:', error);
        }
      }

      if (saveAs && saveAsPath) {
        const cacheKey = this.fullMediaCacheKey(chatId, messageId);
        const cachedOrPending = await this.getMessageMediaFile({ chatId, messageId, priority: 'user' });
        if (cachedOrPending.canceled) {
          downloadService.updateDownload(id, { status: 'canceled', progress: 0, error: 'Cancelado antes de iniciar.' });
          unsub();
          return { success: false, canceled: true };
        }

        if (cachedOrPending.success && cachedOrPending.filePath) {
          await this.fileStorage.saveCachedMediaToPath(cacheKey, cachedOrPending.filePath, saveAsPath);
        } else if (fakeMessage) {
          if (!fakeMessage.url) throw new Error('Mídia sem URL.');
          await this.fileStorage.downloadUrlToFile(fakeMessage.url, saveAsPath, (payload) => {
            const progress = payload.percent;
            downloadService.updateDownload(id, { progress });
            this.mediaProgressCallbacks.forEach(cb => cb({ chatId, messageId, progress, downloadedBytes: payload.downloadedBytes, totalBytes: payload.totalBytes, stage: 'downloading' }));
          });
        } else {
          throw new Error('Download legado indisponível. Use TDLib nativo.');
        }
        unsub();
        downloadService.updateDownload(id, {
          status: 'completed',
          progress: 100,
          fileName: basename(saveAsPath),
        });
        return { success: true, filePath: saveAsPath };
      }

      const res = await this.getMessageMediaFile({ chatId, messageId, priority: 'user' });
      unsub();

      if (res.canceled) {
        downloadService.updateDownload(id, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado antes de iniciar.',
        });
        return { success: false, canceled: true };
      }

      if (res.success && res.filePath) {
        let savedFilePath: string | undefined;
        if (runtimeCapabilities.isAndroid) {
          if (fakeMessage?.url) {
            savedFilePath = await this.fileStorage.pathForDownload(suggestedName);
            await this.fileStorage.saveCachedMediaToPath(this.fullMediaCacheKey(chatId, messageId), res.filePath, savedFilePath);
          } else {
            savedFilePath = await this.fileStorage.saveCachedMediaToDownloads(this.fullMediaCacheKey(chatId, messageId), suggestedName, res.filePath);
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
  async getCachedSharedMedia({ chatId, limit = 12 }: any) {
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

    const cacheKey = `${chatId}:${limit}`;
    const cached = this.sharedMediaCache.get(cacheKey);
    if (cached && Date.now() - cached.loadedAt < 60 * 1000) {
      return { success: true, media: cached.media, fromCache: true };
    }

    const messageCache = messageCacheKey(String(chatId));
    const cachedSharedMedia = await mediaCache.getSharedMediaMessages(messageCache, limit);
    if (cachedSharedMedia.length > 0) {
      this.sharedMediaCache.set(cacheKey, { loadedAt: Date.now(), media: cachedSharedMedia });
      return { success: true, media: cachedSharedMedia, fromCache: true };
    }

    return { success: true, media: [], fromCache: true };
  }

  async refreshSharedMedia({ chatId, limit = 12 }: any) {
    const fakeChat = this.getTwitterFakeChat(chatId);
    if (fakeChat) return this.getCachedSharedMedia({ chatId, limit });

    const cacheKey = `${chatId}:${limit}`;
    try {
      await this.tdlibInit();
      const nativeRes: any = await this.tdlibBridge.getSharedMedia(chatId, limit);
      if (nativeRes?.success) {
        this.sharedMediaCache.set(cacheKey, { loadedAt: Date.now(), media: nativeRes.media || [] });
        return nativeRes;
      }
      return { success: false, media: [], error: nativeRes?.error || 'TDLib não retornou mídia compartilhada.' };
    } catch (error) {
      return { success: false, media: [], error: error instanceof Error ? error.message : String(error) };
    }
  }

  async getSharedMedia({ chatId, limit = 12, refresh = true }: any) {
    const cached = await this.getCachedSharedMedia({ chatId, limit });
    if (cached.media?.length || !refresh) return cached;
    return this.refreshSharedMedia({ chatId, limit });
  }

  async getCacheStats() {
    const stats: any = await mediaCache.getCacheStats();
    if (!runtimeCapabilities.isTauri || !runtimeCapabilities.supportsTdlib) return stats;

    try {
      const nativeStats: any = await this.tdlibBridge.getNativeMediaCacheStats();
      if (nativeStats?.success) {
        return {
          ...stats,
          totalSize: Number(stats.totalSize || 0) + Number(nativeStats.totalSize || 0),
          mediaCount: Number(stats.mediaCount || 0) + Number(nativeStats.mediaCount || 0),
          nativeTotalSize: Number(nativeStats.totalSize || 0),
          nativeMediaCount: Number(nativeStats.mediaCount || 0),
        };
      }
    } catch (error) {
      debugWarn('[TelegramService] Native cache stats unavailable:', error);
    }
    return stats;
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
    if (success && runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      try {
        await this.tdlibBridge.evictNativeMediaCache(Number(settings.maxCacheSize || 0));
      } catch (error) {
        debugWarn('[TelegramService] Native cache eviction failed:', error);
      }
    }
    return { success };
  }

  async clearCache() {
    await mediaCache.clearCache();
    if (runtimeCapabilities.isTauri && runtimeCapabilities.supportsTdlib) {
      try {
        await this.tdlibBridge.clearNativeMediaCache();
      } catch (error) {
        debugWarn('[TelegramService] Native cache clear failed:', error);
      }
    }
    return { success: true };
  }
}

export const telegramService = new TelegramService();

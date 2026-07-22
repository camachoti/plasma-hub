import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { toNumberValue } from './TelegramMessageUtils';

interface TelegramCredentials {
  apiId: number;
  apiHash: string;
}

interface TdlibStatus {
  success: boolean;
  ready: boolean;
  state: string;
  error?: string | null;
}

interface TdlibUserInfo {
  success: boolean;
  id?: number | null;
  firstName?: string | null;
  lastName?: string | null;
  phoneNumber?: string | null;
  error?: string | null;
}

interface TdlibChatsResult {
  success: boolean;
  dialogs: any[];
  error?: string | null;
}

interface TdlibMessagesResult {
  success: boolean;
  messages: any[];
  hasMore: boolean;
  oldestMessageId?: number | null;
  error?: string | null;
}

interface TdlibForumTopicsResult {
  success: boolean;
  topics: any[];
  error?: string | null;
}

interface TdlibSharedMediaResult {
  success: boolean;
  media: any[];
  error?: string | null;
}

interface TdlibSendResult {
  success: boolean;
  message?: any | null;
  error?: string | null;
}

interface TdlibForwardResult {
  success: boolean;
  messages: any[];
  error?: string | null;
}

interface DownloadMessageMediaRequest {
  chatId: unknown;
  messageId: unknown;
  folderPath: string;
}

interface TdlibMassDownloadRequest {
  chatId: unknown;
  folderPath: string;
  topicId?: unknown;
  splitByUser?: boolean;
}

interface TdlibDownloadResult {
  success: boolean;
  skipped: boolean;
  filePath?: string | null;
  fileName?: string | null;
  size: number;
  mimeType?: string | null;
  error?: string | null;
}

interface NativeMediaMeta {
  success: boolean;
  state: string;
  chatId: number;
  messageId: number;
  totalBytes?: number | null;
  downloadedBytes?: number | null;
  mimeType?: string | null;
  fileName?: string | null;
  nativeFilePath?: string | null;
  thumbnailPath?: string | null;
  completedAt?: number | null;
  updatedAt: number;
  error?: string | null;
}

interface NativePlaybackSource {
  success: boolean;
  kind: 'file' | 'stream' | 'fallback' | string;
  url?: string | null;
  filePath?: string | null;
  mimeType?: string | null;
  cacheState: string;
  totalBytes?: number | null;
  downloadedBytes?: number | null;
  fileName?: string | null;
  error?: string | null;
}

interface NativeSavedMedia {
  success: boolean;
  filePath?: string | null;
  fileName?: string | null;
  size?: number | null;
  skipped: boolean;
  error?: string | null;
}

interface NativeMediaCacheStats {
  success: boolean;
  totalSize: number;
  mediaCount: number;
  error?: string | null;
}

export interface NativeMediaProgress {
  chatId: number;
  messageId: number;
  stage: string;
  progress: number;
  downloadedBytes?: number | null;
  totalBytes?: number | null;
}

interface TdlibMassDownloadResult {
  success: boolean;
  downloadedCount: number;
  skippedCount: number;
  failedCount: number;
  total: number;
  aborted: boolean;
  error?: string | null;
}

export interface TdlibDownloadProgress {
  chatId: number;
  total: number;
  downloaded: number;
  currentFile: string;
  isScanning: boolean;
  items: Array<{
    name: string;
    status: string;
    progress: number;
    size: number;
    filePath?: string | null;
    thumbnailPath?: string | null;
  }>;
}

export class TelegramTdlibBridge {
  constructor(
    private readonly getCredentials: () => TelegramCredentials,
  ) {}

  async init() {
    const { apiId, apiHash } = this.getCredentials();
    if (!apiId || !apiHash) {
      return { success: false, ready: false, state: 'error', error: 'Credenciais do Telegram ausentes.' };
    }

    return invoke<TdlibStatus>('tdlib_init', { apiId, apiHash });
  }

  status() {
    return invoke<TdlibStatus>('tdlib_status');
  }

  setPhone(phoneNumber: string) {
    let cleanPhone = phoneNumber.replace(/[^0-9+]/g, '');
    if (!cleanPhone.startsWith('+')) {
      cleanPhone = '+' + cleanPhone;
    }

    return invoke<TdlibStatus>('tdlib_set_phone', { phoneNumber: cleanPhone });
  }

  checkCode(code: string) {
    return invoke<TdlibStatus>('tdlib_check_code', { code });
  }

  checkPassword(password: string) {
    return invoke<TdlibStatus>('tdlib_check_password', { password });
  }

  getMe() {
    return invoke<TdlibUserInfo>('tdlib_get_me');
  }

  getChats(limit = 100) {
    return invoke<TdlibChatsResult>('tdlib_get_chats', { limit });
  }

  getMessages({ chatId, limit = 50, offsetId = 0, topicId = null }: any) {
    return invoke<TdlibMessagesResult>('tdlib_get_messages', {
      chatId: toNumberValue(chatId),
      limit,
      offsetId: offsetId ? toNumberValue(offsetId) : null,
      topicId: topicId == null ? null : toNumberValue(topicId),
    });
  }

  getForumTopics(chatId: unknown, limit = 100) {
    return invoke<TdlibForumTopicsResult>('tdlib_get_forum_topics', {
      chatId: toNumberValue(chatId),
      limit,
    });
  }

  getSharedMedia(chatId: unknown, limit = 12) {
    return invoke<TdlibSharedMediaResult>('tdlib_get_shared_media', {
      chatId: toNumberValue(chatId),
      limit,
    });
  }

  searchUserMedia({ chatId, userId, limit = 100 }: { chatId: unknown; userId: unknown; limit?: number }) {
    return invoke<TdlibSharedMediaResult>('tdlib_search_user_media', {
      chatId: toNumberValue(chatId),
      userId: String(userId),
      limit,
    });
  }

  sendMessage({ chatId, text, replyToId = null, topicId = null }: any) {
    return invoke<TdlibSendResult>('tdlib_send_message', {
      chatId: toNumberValue(chatId),
      text: String(text || ''),
      replyToId: replyToId == null ? null : toNumberValue(replyToId),
      topicId: topicId == null ? null : toNumberValue(topicId),
    });
  }

  sendMedia({ chatId, filePath, caption = '', replyToId = null, topicId = null }: any) {
    return invoke<TdlibSendResult>('tdlib_send_media', {
      chatId: toNumberValue(chatId),
      filePath,
      caption: caption || '',
      replyToId: replyToId == null ? null : toNumberValue(replyToId),
      topicId: topicId == null ? null : toNumberValue(topicId),
    });
  }

  forwardMessage({ chatId, messageId, toChatId = chatId, topicId = null }: any) {
    return invoke<TdlibForwardResult>('tdlib_forward_message', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      toChatId: toNumberValue(toChatId),
      topicId: topicId == null ? null : toNumberValue(topicId),
    });
  }

  downloadMessageMedia({ chatId, messageId, folderPath }: DownloadMessageMediaRequest) {
    return invoke<TdlibDownloadResult>('tdlib_download_message_media', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      folderPath,
    });
  }

  cacheMessageMedia({ chatId, messageId }: Omit<DownloadMessageMediaRequest, 'folderPath'>) {
    return invoke<TdlibDownloadResult & { mimeType?: string | null }>('tdlib_cache_message_media', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
    });
  }

  getNativeMediaMeta({ chatId, messageId }: Omit<DownloadMessageMediaRequest, 'folderPath'>) {
    return invoke<NativeMediaMeta>('telegram_media_get_meta', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
    });
  }

  ensureNativeMediaCached({ chatId, messageId, priority = 'user' }: Omit<DownloadMessageMediaRequest, 'folderPath'> & { priority?: string }) {
    return invoke<NativeMediaMeta>('telegram_media_ensure_cached', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      priority,
    });
  }

  prepareNativeMediaPlayback({ chatId, messageId, mode = 'inline' }: Omit<DownloadMessageMediaRequest, 'folderPath'> & { mode?: string }) {
    return invoke<NativePlaybackSource>('telegram_media_prepare_playback', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      mode,
    });
  }

  saveNativeMedia({ chatId, messageId, destinationPath = null }: Omit<DownloadMessageMediaRequest, 'folderPath'> & { destinationPath?: string | null }) {
    return invoke<NativeSavedMedia>('telegram_media_save', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      destinationPath,
    });
  }

  cancelNativeMedia({ chatId, messageId }: Omit<DownloadMessageMediaRequest, 'folderPath'>) {
    return invoke<boolean>('telegram_media_cancel', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
    });
  }

  getNativeMediaCacheStats() {
    return invoke<NativeMediaCacheStats>('telegram_media_cache_stats');
  }

  clearNativeMediaCache() {
    return invoke<NativeMediaCacheStats>('telegram_media_clear_cache');
  }

  evictNativeMediaCache(maxCacheSize: number) {
    return invoke<NativeMediaCacheStats>('telegram_media_evict_cache', {
      maxCacheSize,
    });
  }

  downloadMessageThumbnail({ chatId, messageId }: Omit<DownloadMessageMediaRequest, 'folderPath'>) {
    return invoke<{ success: boolean; filePath?: string | null; error?: string | null }>('tdlib_download_message_thumbnail', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
    });
  }

  downloadChatAvatar(chatId: unknown) {
    return invoke<{ success: boolean; filePath?: string | null; error?: string | null }>('tdlib_download_chat_avatar', {
      chatId: toNumberValue(chatId),
    });
  }

  startMassDownload({ chatId, folderPath, topicId = null, splitByUser = false }: TdlibMassDownloadRequest) {
    return invoke<TdlibMassDownloadResult>('tdlib_start_mass_download', {
      request: {
        chatId: toNumberValue(chatId),
        folderPath,
        topicId: topicId == null ? null : toNumberValue(topicId),
        splitByUser,
      },
    });
  }

  stopDownload() {
    return invoke<void>('tdlib_stop_download');
  }

  onAuthState(cb: (state: string) => void) {
    return listen<string>('tdlib-auth-state', event => cb(event.payload));
  }

  onDownloadProgress(cb: (data: TdlibDownloadProgress) => void) {
    return listen<TdlibDownloadProgress>('tdlib-download-progress', event => cb(event.payload));
  }

  onNativeMediaProgress(cb: (data: NativeMediaProgress) => void) {
    return listen<NativeMediaProgress>('telegram-media-progress', event => cb(event.payload));
  }
}

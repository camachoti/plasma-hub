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
  error?: string | null;
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

  downloadMessageMedia({ chatId, messageId, folderPath }: DownloadMessageMediaRequest) {
    return invoke<TdlibDownloadResult>('tdlib_download_message_media', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      folderPath,
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
}

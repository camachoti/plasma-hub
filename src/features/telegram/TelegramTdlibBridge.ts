// @ts-nocheck
import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { toNumberValue } from './TelegramMessageUtils';

export class TelegramTdlibBridge {
  constructor(
    private readonly getCredentials: () => { apiId: number; apiHash: string },
  ) {}

  async init() {
    const { apiId, apiHash } = this.getCredentials();
    if (!apiId || !apiHash) {
      return { success: false, ready: false, state: 'error', error: 'Credenciais do Telegram ausentes.' };
    }

    return invoke('tdlib_init', { apiId, apiHash });
  }

  status() {
    return invoke('tdlib_status');
  }

  setPhone(phoneNumber: string) {
    let cleanPhone = phoneNumber.replace(/[^0-9+]/g, '');
    if (!cleanPhone.startsWith('+')) {
      cleanPhone = '+' + cleanPhone;
    }

    return invoke('tdlib_set_phone', { phoneNumber: cleanPhone });
  }

  checkCode(code: string) {
    return invoke('tdlib_check_code', { code });
  }

  checkPassword(password: string) {
    return invoke('tdlib_check_password', { password });
  }

  getMe() {
    return invoke('tdlib_get_me');
  }

  downloadMessageMedia({ chatId, messageId, folderPath }: any) {
    return invoke('tdlib_download_message_media', {
      chatId: toNumberValue(chatId),
      messageId: toNumberValue(messageId),
      folderPath,
    });
  }

  startMassDownload({ chatId, folderPath, topicId = null, splitByUser = false }: any) {
    return invoke('tdlib_start_mass_download', {
      request: {
        chatId: toNumberValue(chatId),
        folderPath,
        topicId: topicId == null ? null : toNumberValue(topicId),
        splitByUser,
      },
    });
  }

  stopDownload() {
    return invoke('tdlib_stop_download');
  }

  onAuthState(cb: (state: string) => void) {
    return listen<string>('tdlib-auth-state', event => cb(event.payload));
  }

  onDownloadProgress(cb: (data: any) => void) {
    return listen<any>('tdlib-download-progress', event => cb(event.payload));
  }
}

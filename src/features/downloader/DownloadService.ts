import { appStorage } from '../../shared/storage/appStorage';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { invokeCommand, listenEvent } from '../../shared/platform/tauri';

const DOWNLOAD_HISTORY_KEY = 'plasma_download_history_v1';
const MAX_DOWNLOAD_HISTORY = 300;
const DOWNLOAD_HISTORY_WRITE_DELAY_MS = 350;
export const DOWNLOAD_STATUS_EVENT = 'plasma-download-status';

export interface DownloadItem {
  id: string; // Unique ID for the download
  chatId?: string; // Optional, for Telegram
  messageId?: number; // Optional, for Telegram
  fileName: string;
  progress: number;
  status: 'downloading' | 'completed' | 'failed' | 'canceled';
  error?: string;
  filePath?: string;
  fileSize?: number;
  sourceUrl?: string;
  resumeHeaders?: Record<string, string>;
  platform?: 'telegram' | 'youtube' | 'tiktok' | 'instagram' | 'twitter' | 'reddit' | 'web';
  thumbnailUrl?: string;
  sourceLabel?: string;
  chatTitle?: string;
  chatKind?: string;
  topicTitle?: string;
  senderName?: string;
  senderId?: string | null;
  batchId?: string;
  batchTitle?: string;
  batchKind?: 'mass' | 'bulk' | 'single';
  batchTotal?: number;
  batchDownloaded?: number;
  batchCompleted?: number;
  batchSkipped?: number;
  batchFailed?: number;
  retrying?: boolean;
  retryAttempted?: boolean;
  createdAt?: number;
  canCancel?: boolean;
  canRetry?: boolean;
}

export interface DownloadActionHandlers {
  cancel?: () => unknown | Promise<unknown>;
  retry?: () => unknown | Promise<unknown>;
}

type NativeDownloadProgress = {
  id: string;
  percent: number;
  downloadedBytes?: number;
  totalBytes?: number;
};

class DownloadService {
  public activeDownloads: Map<string, DownloadItem> = new Map();
  private downloadsChangeCallbacks = new Set<(items: DownloadItem[]) => void>();
  private actionHandlers = new Map<string, DownloadActionHandlers>();
  private persistTimer: ReturnType<typeof setTimeout> | null = null;
  private nativePersistQueue: Promise<void> = Promise.resolve();

  constructor() {
    this.restoreFromStorage(appStorage.get(DOWNLOAD_HISTORY_KEY));
    if (runtimeCapabilities.isTauri) void this.restoreFromNativeStorage();
    window.addEventListener('pagehide', () => this.flushPersist());
  }

  private restoreFromStorage(stored: string | null) {
    try {
      if (!stored) return;
      const items = JSON.parse(stored) as DownloadItem[];
      for (const item of items.slice(-MAX_DOWNLOAD_HISTORY)) {
        const wasInterrupted = item.status === 'downloading';
        const restored = {
          ...item,
          retrying: false,
          ...(wasInterrupted
            ? { status: 'failed' as const, error: 'Download interrompido ao fechar o aplicativo.' }
            : {}),
        };
        if (runtimeCapabilities.isTauri && restored.sourceUrl && restored.filePath) {
          this.actionHandlers.set(restored.id, {
            cancel: () => invokeCommand<boolean>('cancel_native_download', { id: restored.id }),
            retry: () => this.retryPersistedNativeDownload(restored.id),
          });
        } else if (restored.platform === 'telegram' && restored.chatId && restored.messageId !== undefined) {
          this.actionHandlers.set(restored.id, {
            retry: async () => {
              const { telegramService } = await import('../telegram/TelegramService');
              return telegramService.retryStoredDownload(restored);
            },
          });
        }
        const actions = this.actionHandlers.get(restored.id);
        restored.canCancel = Boolean(actions?.cancel) && restored.status === 'downloading';
        restored.canRetry = Boolean(actions?.retry) && restored.status !== 'downloading';
        const current = this.activeDownloads.get(restored.id);
        if (!current || (restored.createdAt || 0) >= (current.createdAt || 0)) {
          this.activeDownloads.set(restored.id, restored);
        }
      }
    } catch {
      appStorage.remove(DOWNLOAD_HISTORY_KEY);
    }
  }

  private async retryPersistedNativeDownload(id: string) {
    const item = this.activeDownloads.get(id);
    if (!item?.sourceUrl || !item.filePath || !runtimeCapabilities.isTauri) return false;

    const unlisten = await listenEvent<NativeDownloadProgress>('native-file-download-progress', event => {
      if (event.payload.id !== id) return;
      this.updateDownload(id, {
        progress: event.payload.percent,
        ...(event.payload.totalBytes && event.payload.totalBytes > 0 ? { fileSize: event.payload.totalBytes } : {}),
      });
    });
    this.updateDownload(id, { status: 'downloading', error: undefined });
    try {
      await invokeCommand('download_url_to_file', {
        id,
        url: item.sourceUrl,
        filePath: item.filePath,
        headers: item.resumeHeaders,
      });
      this.updateDownload(id, { status: 'completed', progress: 100 });
      return true;
    } catch (error) {
      this.updateDownload(id, {
        status: 'failed',
        error: error instanceof Error ? error.message : String(error),
      });
      return false;
    } finally {
      unlisten();
    }
  }

  private async restoreFromNativeStorage() {
    try {
      const history = await invokeCommand<string | null>('download_history_get');
      this.restoreFromStorage(history);
      this.downloadsChangeCallbacks.forEach(callback => callback(Array.from(this.activeDownloads.values())));
    } catch {
      // The local copy remains available if the native store is not ready yet.
    }
  }

  private persist() {
    const items = Array.from(this.activeDownloads.values())
      .slice(-MAX_DOWNLOAD_HISTORY)
      .map(item => ({
        ...item,
        retrying: undefined,
        canCancel: undefined,
        canRetry: undefined,
        thumbnailUrl: item.thumbnailUrl?.startsWith('blob:') ? undefined : item.thumbnailUrl,
      }));
    const serialized = JSON.stringify(items);
    appStorage.set(DOWNLOAD_HISTORY_KEY, serialized);
    if (runtimeCapabilities.isTauri) {
      this.nativePersistQueue = this.nativePersistQueue
        .catch(() => {})
        .then(async () => { await invokeCommand('download_history_save', { historyJson: serialized }); })
        .catch(() => {});
    }
  }

  private schedulePersist() {
    if (this.persistTimer) clearTimeout(this.persistTimer);
    this.persistTimer = setTimeout(() => {
      this.persistTimer = null;
      this.persist();
    }, DOWNLOAD_HISTORY_WRITE_DELAY_MS);
  }

  private flushPersist() {
    if (this.persistTimer) {
      clearTimeout(this.persistTimer);
      this.persistTimer = null;
    }
    this.persist();
  }

  onDownloadsChange(cb: (items: DownloadItem[]) => void) {
    this.downloadsChangeCallbacks.add(cb);
    return () => { this.downloadsChangeCallbacks.delete(cb); };
  }

  public emitDownloadsChange(persistImmediately = false) {
    if (persistImmediately) this.flushPersist();
    else this.schedulePersist();
    const list = Array.from(this.activeDownloads.values());
    this.downloadsChangeCallbacks.forEach(cb => cb(list));
  }

  addDownload(item: DownloadItem, handlers: DownloadActionHandlers = {}) {
    if (Object.keys(handlers).length > 0) {
      this.actionHandlers.set(item.id, { ...this.actionHandlers.get(item.id), ...handlers });
    }
    const actionHandlers = this.actionHandlers.get(item.id);
    this.activeDownloads.set(item.id, {
      ...item,
      canCancel: Boolean(actionHandlers?.cancel) && item.status === 'downloading',
      canRetry: Boolean(actionHandlers?.retry) && item.status !== 'downloading',
      createdAt: item.createdAt || Date.now(),
    });
    this.emitDownloadsChange();
  }

  updateDownload(id: string, updates: Partial<DownloadItem>, persistImmediately = false) {
    const item = this.activeDownloads.get(id);
    if (item) {
      const previousStatus = item.status;
      Object.assign(item, updates);
      const actionHandlers = this.actionHandlers.get(id);
      item.canCancel = Boolean(actionHandlers?.cancel) && item.status === 'downloading';
      item.canRetry = Boolean(actionHandlers?.retry) && item.status !== 'downloading';
      this.activeDownloads.set(id, item);
      const reachedTerminalState = Boolean(
        updates.status && updates.status !== previousStatus && updates.status !== 'downloading',
      );
      this.emitDownloadsChange(persistImmediately || reachedTerminalState);
      if (reachedTerminalState) {
        window.dispatchEvent(new CustomEvent(DOWNLOAD_STATUS_EVENT, { detail: { ...item } }));
      }
    }
  }

  getDownload(id: string) {
    return this.activeDownloads.get(id);
  }

  removeDownload(id: string) {
    if (!this.activeDownloads.delete(id)) return;
    this.actionHandlers.delete(id);
    this.emitDownloadsChange(true);
  }

  clearFinished() {
    for (const [id, item] of this.activeDownloads) {
      if (item.status !== 'downloading') {
        this.activeDownloads.delete(id);
        this.actionHandlers.delete(id);
      }
    }
    this.emitDownloadsChange(true);
  }

  setActions(id: string, handlers: DownloadActionHandlers) {
    this.actionHandlers.set(id, { ...this.actionHandlers.get(id), ...handlers });
    const item = this.activeDownloads.get(id);
    if (!item) return;
    this.updateDownload(id, {});
  }

  clearActions(id: string, keys?: Array<keyof DownloadActionHandlers>) {
    const handlers = this.actionHandlers.get(id);
    if (!handlers) return;
    if (!keys) {
      this.actionHandlers.delete(id);
    } else {
      for (const key of keys) delete handlers[key];
      if (!handlers.cancel && !handlers.retry) this.actionHandlers.delete(id);
      else this.actionHandlers.set(id, handlers);
    }
    const item = this.activeDownloads.get(id);
    if (!item) return;
    this.updateDownload(id, {});
  }

  async cancelDownload(id: string) {
    const handler = this.actionHandlers.get(id)?.cancel;
    if (!handler) return false;
    await handler();
    return true;
  }

  async retryDownload(id: string) {
    const handler = this.actionHandlers.get(id)?.retry;
    if (!handler) return false;
    this.updateDownload(id, { status: 'downloading', progress: 0, error: undefined, retrying: true, retryAttempted: true });
    try {
      await handler();
      return true;
    } finally {
      if (this.activeDownloads.has(id)) this.updateDownload(id, { retrying: false });
    }
  }
}

export const downloadService = new DownloadService();

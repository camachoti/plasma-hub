import { appStorage } from '../../shared/storage/appStorage';

const DOWNLOAD_HISTORY_KEY = 'plasma_download_history_v1';
const MAX_DOWNLOAD_HISTORY = 300;
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
  createdAt?: number;
  canCancel?: boolean;
  canRetry?: boolean;
}

export interface DownloadActionHandlers {
  cancel?: () => unknown | Promise<unknown>;
  retry?: () => unknown | Promise<unknown>;
}

class DownloadService {
  public activeDownloads: Map<string, DownloadItem> = new Map();
  private downloadsChangeCallbacks = new Set<(items: DownloadItem[]) => void>();
  private actionHandlers = new Map<string, DownloadActionHandlers>();

  constructor() {
    this.restore();
  }

  private restore() {
    try {
      const stored = appStorage.get(DOWNLOAD_HISTORY_KEY);
      if (!stored) return;
      const items = JSON.parse(stored) as DownloadItem[];
      for (const item of items.slice(-MAX_DOWNLOAD_HISTORY)) {
        const restored = {
          ...item,
          canCancel: false,
          canRetry: false,
          ...(item.status === 'downloading'
            ? { status: 'failed' as const, error: 'Download interrompido ao fechar o aplicativo.' }
            : {}),
        };
        this.activeDownloads.set(restored.id, restored);
      }
    } catch {
      appStorage.remove(DOWNLOAD_HISTORY_KEY);
    }
  }

  private persist() {
    const items = Array.from(this.activeDownloads.values())
      .slice(-MAX_DOWNLOAD_HISTORY)
      .map(item => ({
        ...item,
        canCancel: undefined,
        canRetry: undefined,
        thumbnailUrl: item.thumbnailUrl?.startsWith('blob:') ? undefined : item.thumbnailUrl,
      }));
    appStorage.set(DOWNLOAD_HISTORY_KEY, JSON.stringify(items));
  }

  onDownloadsChange(cb: (items: DownloadItem[]) => void) {
    this.downloadsChangeCallbacks.add(cb);
    return () => { this.downloadsChangeCallbacks.delete(cb); };
  }

  public emitDownloadsChange() {
    this.persist();
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

  updateDownload(id: string, updates: Partial<DownloadItem>) {
    const item = this.activeDownloads.get(id);
    if (item) {
      const previousStatus = item.status;
      Object.assign(item, updates);
      const actionHandlers = this.actionHandlers.get(id);
      item.canCancel = Boolean(actionHandlers?.cancel) && item.status === 'downloading';
      item.canRetry = Boolean(actionHandlers?.retry) && item.status !== 'downloading';
      this.activeDownloads.set(id, item);
      this.emitDownloadsChange();
      if (updates.status && updates.status !== previousStatus && updates.status !== 'downloading') {
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
    this.emitDownloadsChange();
  }

  clearFinished() {
    for (const [id, item] of this.activeDownloads) {
      if (item.status !== 'downloading') {
        this.activeDownloads.delete(id);
        this.actionHandlers.delete(id);
      }
    }
    this.emitDownloadsChange();
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
    await handler();
    return true;
  }
}

export const downloadService = new DownloadService();

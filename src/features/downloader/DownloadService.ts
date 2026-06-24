export interface DownloadItem {
  id: string; // Unique ID for the download
  chatId?: string; // Optional, for Telegram
  messageId?: number; // Optional, for Telegram
  fileName: string;
  progress: number;
  status: 'downloading' | 'completed' | 'failed';
  error?: string;
  platform?: 'telegram' | 'youtube' | 'tiktok' | 'instagram' | 'twitter' | 'reddit' | 'web';
  thumbnailUrl?: string;
  sourceLabel?: string;
  chatTitle?: string;
  chatKind?: string;
  topicTitle?: string;
  senderName?: string;
  senderId?: string | null;
}

class DownloadService {
  public activeDownloads: Map<string, DownloadItem> = new Map();
  private downloadsChangeCallbacks: Set<Function> = new Set();

  onDownloadsChange(cb: Function) {
    this.downloadsChangeCallbacks.add(cb);
    return () => { this.downloadsChangeCallbacks.delete(cb); };
  }

  public emitDownloadsChange() {
    const list = Array.from(this.activeDownloads.values());
    this.downloadsChangeCallbacks.forEach(cb => cb(list));
  }

  addDownload(item: DownloadItem) {
    this.activeDownloads.set(item.id, item);
    this.emitDownloadsChange();
  }

  updateDownload(id: string, updates: Partial<DownloadItem>) {
    const item = this.activeDownloads.get(id);
    if (item) {
      Object.assign(item, updates);
      this.activeDownloads.set(id, item);
      this.emitDownloadsChange();
    }
  }

  getDownload(id: string) {
    return this.activeDownloads.get(id);
  }
}

export const downloadService = new DownloadService();

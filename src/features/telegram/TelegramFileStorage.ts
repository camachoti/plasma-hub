import { getDownloadDir, joinPath as joinSystemPath } from '../../shared/platform/files';
import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { invokeCommand as invoke } from '../../shared/platform/tauri';
import { mediaCache } from './MediaCacheService';
import { nextFrame, sanitizeForFilename, toUint8Array } from './TelegramMessageUtils';

type ProgressPayload = {
  percent: number;
  downloadedBytes?: number;
  totalBytes?: number;
};

type ProgressCallback = (progress: ProgressPayload) => void;

class TelegramFileStorage {
  async saveBytesToFile(filePath: string, data: any) {
    const bytes = toUint8Array(data);
    const chunkSize = 512 * 1024;

    if (bytes.byteLength === 0) {
      throw new Error('Download sem bytes.');
    }

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

  async appendBytesToFile(filePath: string, data: any) {
    const bytes = toUint8Array(data);
    const chunkSize = 256 * 1024;

    if (bytes.byteLength === 0) return;

    for (let offset = 0; offset < bytes.byteLength; offset += chunkSize) {
      const chunk = bytes.slice(offset, offset + chunkSize);
      await invoke('append_download_file_chunk', { filePath, data: Array.from(chunk) });
      await nextFrame();
    }
  }

  async downloadUrlToFile(url: string, filePath: string, onProgress?: ProgressCallback) {
    const response = await tauriFetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const totalBytes = Number(response.headers.get('content-length')) || undefined;
    onProgress?.({ percent: 20, downloadedBytes: 0, totalBytes });
    const buffer = await response.arrayBuffer();
    onProgress?.({ percent: 80, downloadedBytes: buffer.byteLength, totalBytes });
    await this.saveBytesToFile(filePath, buffer);
    onProgress?.({ percent: 100, downloadedBytes: buffer.byteLength, totalBytes: totalBytes || buffer.byteLength });
  }

  async saveCachedMediaToPath(cacheKey: string, fallbackUrl: string | undefined, filePath: string) {
    let buffer = await mediaCache.getMediaBuffer(cacheKey);
    if ((!buffer || buffer.byteLength === 0) && fallbackUrl) {
      buffer = await this.bufferFromUrl(fallbackUrl);
    }
    if (!buffer || buffer.byteLength === 0) {
      throw new Error('Mídia baixada sem bytes. Tente abrir a mídia e baixar novamente.');
    }

    await this.saveBytesToFile(filePath, buffer);
    return filePath;
  }

  async saveCachedMediaToDownloads(cacheKey: string, fileName: string, fallbackUrl?: string) {
    const filePath = await this.pathForDownload(fileName);
    return this.saveCachedMediaToPath(cacheKey, fallbackUrl, filePath);
  }

  async pathForDownload(fileName: string) {
    const downloadDir = await getDownloadDir();
    return joinSystemPath(downloadDir, sanitizeForFilename(fileName));
  }

  private async bufferFromUrl(url: string) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Falha ao ler mídia baixada (${response.status}).`);
    return response.arrayBuffer();
  }
}

export const telegramFileStorage = new TelegramFileStorage();

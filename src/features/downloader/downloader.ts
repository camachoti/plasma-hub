import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { downloadUrlInBrowser } from '../../shared/platform/browserDownload';
import { getDownloadDir, joinPath } from '../../shared/platform/files';
import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { debugWarn } from '../../shared/debug/logger';
import type { MediaInfo } from './types';
import { detectPlatform } from './platforms';
import { downloadService } from './DownloadService';
import { extractInstagram } from './extractors/instagram';
import { extractReddit } from './extractors/reddit';
import { extractTikTok } from './extractors/tiktok';
import { extractTwitter } from './extractors/twitter';
import { extractYoutube } from './extractors/youtube';
import {
  cleanOptional,
  extractTwitterId,
  getDownloadExtension,
  isTwitterProfileUrl,
} from './downloaderUtils';
import type { PlatformId } from './types';

export interface TwitterRequestOptions {
  twitterCookies?: string;
}

// ─── Public API ────────────────────────────────────────────────────────────────

export async function analyzeUrl(url: string, options: TwitterRequestOptions = {}): Promise<MediaInfo> {
  const platform = detectPlatform(url);
  if (!platform) throw new Error('unsupported');

  switch (platform) {
    case 'reddit':    return extractReddit(url);
    case 'youtube':   return extractYoutube(url);
    case 'tiktok':
      if (runtimeCapabilities.isTauri && !runtimeCapabilities.isAndroid) {
        return invoke<MediaInfo>('analyze_tiktok_native', { url });
      }
      return extractTikTok(url);
    case 'instagram': return extractInstagram(url);
    case 'twitter':
      if (!extractTwitterId(url)) {
        if (isTwitterProfileUrl(url)) {
          throw new Error('URL de perfil ainda nao e suportada no motor Rust. Cole a URL de um tweet, como https://x.com/usuario/status/123.');
        }
        throw new Error('Nao foi possivel identificar o tweet. Cole uma URL /status/... do Twitter/X.');
      }
      try {
        return await invoke<MediaInfo>('analyze_twitter_tweet_native', {
          url,
          cookies: cleanOptional(options.twitterCookies),
        });
      } catch (err) {
        debugWarn('Native Twitter analyzer failed, falling back to syndication extractor:', err);
        return extractTwitter(url);
      }
  }
}

// ─── Download ─────────────────────────────────────────────────────────────────

function getDownloadHeaders(platform: PlatformId): Record<string, string> {
  const defaultUserAgent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';

  switch (platform) {
    case 'youtube':
      return {
        'User-Agent': defaultUserAgent,
        'Referer': 'https://www.youtube.com/',
        'Origin': 'https://www.youtube.com',
      };
    case 'tiktok':
      return {
        'User-Agent': defaultUserAgent,
        'Accept': 'video/mp4,video/*;q=0.9,*/*;q=0.8',
      };
    case 'twitter':
      return {
        'User-Agent': defaultUserAgent,
        'Referer': 'https://x.com/',
        'Accept': 'video/mp4,image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8',
      };
    default:
      return {
        'User-Agent': defaultUserAgent,
        'Accept': '*/*',
      };
  }
}

async function downloadViaBlob(
  url: string,
  filename: string,
  platform: PlatformId,
  onProgress: (p: number) => void,
  signal?: AbortSignal
): Promise<void> {
  const res = await tauriFetch(url, { 
    method: 'GET',
    signal,
    headers: getDownloadHeaders(platform),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);

  const contentLength = res.headers.get('content-length');
  const total = contentLength ? parseInt(contentLength, 10) : 0;
  const reader = res.body!.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    if (value) {
      chunks.push(value);
      received += value.length;
      onProgress(total > 0 ? Math.min(97, (received / total) * 100) : Math.min(60, received / 20000));
    }
  }

  const blob = new Blob(chunks as BlobPart[]);
  const objUrl = URL.createObjectURL(blob);
  downloadUrlInBrowser(objUrl, filename);
  setTimeout(() => URL.revokeObjectURL(objUrl), 2000);
  onProgress(100);
}

async function downloadViaNativeFile(
  id: string,
  url: string,
  filename: string,
  platform: PlatformId,
  onProgress: (p: number) => void,
  onDestinationReady: (filePath: string) => void,
): Promise<string> {
  const downloadDir = await getDownloadDir();
  const filePath = await joinPath(downloadDir, filename);
  onDestinationReady(filePath);
  const unlisten = await listen<{ id: string; percent: number }>('native-file-download-progress', event => {
    if (event.payload.id === id) onProgress(event.payload.percent);
  });
  try {
    await invoke('download_url_to_file', {
      id,
      url,
      filePath,
      headers: getDownloadHeaders(platform),
    });
    return filePath;
  } finally {
    unlisten();
  }
}

export async function downloadMedia(
  info: MediaInfo,
  mode: 'video' | 'audio',
  formatId: string,
  options: TwitterRequestOptions = {}
): Promise<void> {
  const format = info.formats[mode].find((f) => f.id === formatId) ?? info.formats[mode][0];
  let dlUrl = format?.url;

  const ext = getDownloadExtension(info.platform, mode, dlUrl);
  const safeName = info.title.replace(/[^a-z0-9]/gi, '_').slice(0, 40);
  const filename = `plasma_${safeName}.${ext}`;

  const downloadId = `${info.platform}_${Date.now()}`;
  
  downloadService.addDownload({
    id: downloadId,
    fileName: filename,
    progress: 0,
    status: 'downloading',
    platform: info.platform as any,
    thumbnailUrl: info.thumbnailUrl
  }, {
    retry: () => downloadMedia(info, mode, formatId, options),
  });

  const onProgress = (p: number) => {
    downloadService.updateDownload(downloadId, { progress: p });
  };

  if (info.platform === 'youtube' && runtimeCapabilities.isAndroid && mode === 'video' && format.hasAudio === false) {
    downloadService.updateDownload(downloadId, {
      status: 'failed',
      error: 'Este formato separa video e audio. Escolha um formato com audio no Android.',
    });
    return;
  }

  if (
    info.platform === 'youtube' &&
    info.originalUrl &&
    runtimeCapabilities.supportsNativeYoutube
  ) {
    const unlisteners: Array<() => void> = [];
    downloadService.setActions(downloadId, {
      cancel: async () => {
        await invoke<boolean>('cancel_native_download', { id: downloadId });
        downloadService.updateDownload(downloadId, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado pelo usuário.',
        });
      },
    });
    try {
      // se for vídeo e não tiver áudio nativo, combina com o melhor áudio
      let ytFormat = formatId;
      if (mode === 'video' && format.hasAudio === false) {
        ytFormat = `${formatId}+bestaudio[ext=m4a]/best`;
      }

      const unlistenProgress = await listen<{id: string, progress: number}>('youtube-download-progress', (e) => {
        if (e.payload.id === downloadId) {
          onProgress(e.payload.progress);
        }
      });
      unlisteners.push(unlistenProgress);

      const unlistenDone = await listen<{id: string}>('youtube-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });
      unlisteners.push(unlistenDone);

      const unlistenError = await listen<{id: string, error: string}>('youtube-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });
      unlisteners.push(unlistenError);

      await invoke('download_youtube_native', { 
        id: downloadId,
        url: info.originalUrl, 
        formatId: ytFormat,
        filename: filename
      });

      return;
    } catch (err: any) {
      if (downloadService.getDownload(downloadId)?.status === 'canceled') return;
      debugWarn('Backend download failed, fallback to direct url:', err);
      // Try to fallback
      try {
        dlUrl = await invoke<string>('get_youtube_stream_url', { 
          url: info.originalUrl, 
          formatId 
        });
      } catch (e) {}
      downloadService.updateDownload(downloadId, { status: 'downloading', error: undefined });
    } finally {
      unlisteners.forEach(unlisten => unlisten());
      downloadService.clearActions(downloadId, ['cancel']);
    }
  }

  if (info.platform === 'twitter' && dlUrl && runtimeCapabilities.supportsNativeTwitter) {
    const unlisteners: Array<() => void> = [];
    downloadService.setActions(downloadId, {
      cancel: async () => {
        await invoke<boolean>('cancel_native_download', { id: downloadId });
        downloadService.updateDownload(downloadId, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado pelo usuário.',
        });
      },
    });
    try {
      const unlistenProgress = await listen<{id: string, progress: number}>('twitter-download-progress', (e) => {
        if (e.payload.id === downloadId) {
          onProgress(e.payload.progress);
        }
      });
      unlisteners.push(unlistenProgress);

      const unlistenDone = await listen<{id: string}>('twitter-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });
      unlisteners.push(unlistenDone);

      const unlistenError = await listen<{id: string, error: string}>('twitter-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });
      unlisteners.push(unlistenError);

      await invoke('download_twitter_native', {
        id: downloadId,
        url: dlUrl,
        filename,
        cookies: cleanOptional(options.twitterCookies),
      });

      return;
    } catch (err: any) {
      if (downloadService.getDownload(downloadId)?.status === 'canceled') return;
      debugWarn('Native Twitter download failed, falling back to blob download:', err);
      downloadService.updateDownload(downloadId, { status: 'downloading', error: undefined });
    } finally {
      unlisteners.forEach(unlisten => unlisten());
      downloadService.clearActions(downloadId, ['cancel']);
    }
  }

  if (
    info.platform === 'tiktok' &&
    info.originalUrl &&
    runtimeCapabilities.isTauri &&
    !runtimeCapabilities.isAndroid
  ) {
    const unlisteners: Array<() => void> = [];
    downloadService.setActions(downloadId, {
      cancel: async () => {
        await invoke<boolean>('cancel_native_download', { id: downloadId });
        downloadService.updateDownload(downloadId, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado pelo usuário.',
        });
      },
    });
    try {
      const unlistenProgress = await listen<{id: string, progress: number}>('tiktok-download-progress', (event) => {
        if (event.payload.id === downloadId) onProgress(event.payload.progress);
      });
      unlisteners.push(unlistenProgress);

      const unlistenDone = await listen<{id: string}>('tiktok-download-done', (event) => {
        if (event.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });
      unlisteners.push(unlistenDone);

      const unlistenError = await listen<{id: string, error: string}>('tiktok-download-error', (event) => {
        if (event.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: event.payload.error });
        }
      });
      unlisteners.push(unlistenError);

      await invoke('download_tiktok_native', {
        id: downloadId,
        url: info.originalUrl,
        formatId,
        filename,
        directUrl: dlUrl,
      });
      return;
    } catch (error) {
      if (downloadService.getDownload(downloadId)?.status === 'canceled') return;
      const message = error instanceof Error ? error.message : String(error);
      debugWarn('Native TikTok download failed:', error);
      downloadService.updateDownload(downloadId, {
        status: 'failed',
        error: message || 'O download nativo do TikTok falhou.',
      });
      return;
    } finally {
      unlisteners.forEach(unlisten => unlisten());
      downloadService.clearActions(downloadId, ['cancel']);
    }
  }

  if (!dlUrl) {
    downloadService.updateDownload(downloadId, {
      status: 'failed',
      error: info.platform === 'youtube'
        ? 'Formato sem URL direta resolvível pelo backend nativo.'
        : 'URL de download não encontrada.'
    });
    return;
  }

  try {
    const abortController = new AbortController();
    downloadService.setActions(downloadId, {
      cancel: async () => {
        if (runtimeCapabilities.isTauri) {
          await invoke<boolean>('cancel_native_download', { id: downloadId });
        } else {
          abortController.abort();
        }
        downloadService.updateDownload(downloadId, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado pelo usuário.',
        });
      },
    });

    if (runtimeCapabilities.isTauri) {
      const resumeHeaders = getDownloadHeaders(info.platform);
      downloadService.updateDownload(downloadId, { sourceUrl: dlUrl, resumeHeaders }, true);
      const filePath = await downloadViaNativeFile(
        downloadId,
        dlUrl,
        filename,
        info.platform,
        onProgress,
        path => downloadService.updateDownload(downloadId, { filePath: path }, true),
      );
      downloadService.updateDownload(downloadId, { status: 'completed', progress: 100, filePath });
    } else {
      await downloadViaBlob(dlUrl, filename, info.platform, onProgress, abortController.signal);
      downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
    }
  } catch (err: any) {
    if (err?.name === 'AbortError') {
      downloadService.updateDownload(downloadId, {
        status: 'canceled',
        progress: 0,
        error: 'Cancelado pelo usuário.',
      });
      return;
    }
    debugWarn("Download failed", err);
    downloadService.updateDownload(downloadId, { status: 'failed', error: err.message });
  } finally {
    downloadService.clearActions(downloadId, ['cancel']);
  }
}

export function isNative(): boolean {
  return runtimeCapabilities.isTauri;
}

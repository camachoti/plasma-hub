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
    case 'tiktok':    return extractTikTok(url);
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

async function downloadViaBlob(
  url: string,
  filename: string,
  onProgress: (p: number) => void
): Promise<void> {
  const res = await tauriFetch(url, { 
    method: 'GET',
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      'Referer': 'https://www.youtube.com/',
      'Origin': 'https://www.youtube.com'
    }
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
  url: string,
  filename: string,
  onProgress: (p: number) => void
): Promise<string> {
  const downloadDir = await getDownloadDir();
  const filePath = await joinPath(downloadDir, filename);

  const res = await tauriFetch(url, {
    method: 'GET',
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      'Referer': 'https://www.youtube.com/',
      'Origin': 'https://www.youtube.com'
    }
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);

  const contentLength = res.headers.get('content-length');
  const total = contentLength ? parseInt(contentLength, 10) : 0;
  const reader = res.body?.getReader();

  await invoke('begin_download_file', { filePath });
  try {
    if (!reader) {
      const buffer = new Uint8Array(await res.arrayBuffer());
      await invoke('append_download_file_chunk', { filePath, data: Array.from(buffer) });
      onProgress(97);
    } else {
      let received = 0;
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        if (!value) continue;
        received += value.length;
        await invoke('append_download_file_chunk', { filePath, data: Array.from(value) });
        onProgress(total > 0 ? Math.min(97, (received / total) * 100) : Math.min(90, received / 20000));
      }
    }

    await invoke('finish_download_file', { filePath });
    onProgress(100);
    return filePath;
  } catch (error) {
    await invoke('abort_download_file', { filePath }).catch(() => {});
    throw error;
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

      const unlistenDone = await listen<{id: string}>('youtube-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });

      const unlistenError = await listen<{id: string, error: string}>('youtube-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });

      await invoke('download_youtube_native', { 
        id: downloadId,
        url: info.originalUrl, 
        formatId: ytFormat,
        filename: filename
      });

      unlistenProgress();
      unlistenDone();
      unlistenError();
      return;
    } catch (err: any) {
      debugWarn('Backend download failed, fallback to direct url:', err);
      // Try to fallback
      try {
        dlUrl = await invoke<string>('get_youtube_stream_url', { 
          url: info.originalUrl, 
          formatId 
        });
      } catch (e) {}
    }
  }

  if (info.platform === 'twitter' && dlUrl && runtimeCapabilities.supportsNativeTwitter) {
    try {
      const unlistenProgress = await listen<{id: string, progress: number}>('twitter-download-progress', (e) => {
        if (e.payload.id === downloadId) {
          onProgress(e.payload.progress);
        }
      });

      const unlistenDone = await listen<{id: string}>('twitter-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });

      const unlistenError = await listen<{id: string, error: string}>('twitter-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });

      await invoke('download_twitter_native', {
        id: downloadId,
        url: dlUrl,
        filename,
        cookies: cleanOptional(options.twitterCookies),
      });

      unlistenProgress();
      unlistenDone();
      unlistenError();
      return;
    } catch (err: any) {
      debugWarn('Native Twitter download failed, falling back to blob download:', err);
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
    if (runtimeCapabilities.isTauri) {
      const filePath = await downloadViaNativeFile(dlUrl, filename, onProgress);
      downloadService.updateDownload(downloadId, { status: 'completed', progress: 100, filePath });
    } else {
      await downloadViaBlob(dlUrl, filename, onProgress);
      downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
    }
  } catch (err: any) {
    debugWarn("Download failed", err);
    downloadService.updateDownload(downloadId, { status: 'failed', error: err.message });
  }
}

export function isNative(): boolean {
  return runtimeCapabilities.isTauri;
}

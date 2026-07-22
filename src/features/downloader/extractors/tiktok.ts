import { runtimeCapabilities } from '../../../shared/platform/runtime';
import type { MediaInfo } from '../types';
import { apiFetch, resolveRedirects } from '../downloaderApi';
import { extractTikTokVideoId, formatCount, formatDuration } from '../downloaderUtils';

interface TikTokAwemeItem {
  desc: string;
  author: { unique_id: string; nickname: string };
  statistics?: { play_count: number; digg_count: number };
  video: {
    duration: number;
    play_addr?: { url_list: string[] };
    download_addr?: { url_list: string[] };
    cover?: { url_list: string[] };
  };
  music?: { play_url?: { url_list: string[] }; title?: string };
}

export async function extractTikTok(url: string): Promise<MediaInfo> {
  if (!runtimeCapabilities.isTauri) {
    return extractTikTokWebFallback(url);
  }

  const resolved = await resolveRedirects(url);
  const videoId = extractTikTokVideoId(resolved) ?? extractTikTokVideoId(url);
  if (!videoId) throw new Error('Cannot parse TikTok URL');

  const apiUrl = `https://api16-normal-c-useast1a.tiktokv.com/aweme/v1/feed/?aweme_id=${videoId}&iid=1234567890&device_id=1234567890&version_name=26.1.3&app_name=trill&channel=googleplay&device_platform=android&device_type=Pixel+4&os_version=11`;

  const res = await apiFetch(apiUrl, {
    headers: {
      'User-Agent': 'TikTok 26.1.3 rv:261303 (iPhone; iOS 14.4.2; en_US) Cronet',
      'Accept': 'application/json',
    },
  });

  if (res.status >= 400) throw new Error(`TikTok API ${res.status}`);

  const item: TikTokAwemeItem | undefined = (res.data as { aweme_list?: TikTokAwemeItem[] })?.aweme_list?.[0];
  if (!item) {
    return extractTikTokPage(url, videoId);
  }

  const videoUrl = item.video.download_addr?.url_list?.[0] ?? item.video.play_addr?.url_list?.[0];
  const audioUrl = item.music?.play_url?.url_list?.[0];
  const thumbUrl = item.video.cover?.url_list?.[0];
  const durSec = item.video.duration / 1000;

  return {
    platform: 'tiktok',
    title: item.desc || 'TikTok Video',
    author: `@${item.author.unique_id}`,
    authorFull: item.author.nickname,
    duration: formatDuration(Math.round(durSec)),
    views: item.statistics ? formatCount(item.statistics.play_count) : undefined,
    thumbHue: 320,
    thumbHue2: 260,
    thumbnailUrl: thumbUrl,
    formats: {
      video: videoUrl ? [
        { id: 'tiktok-hd', label: 'HD', sub: 'MP4 · TikTok', size: `~${(durSec * 0.3).toFixed(1)} MB`, best: true, url: videoUrl },
      ] : [{ id: 'na', label: 'Indisponível', sub: 'URL não encontrada', size: '—' }],
      audio: audioUrl ? [
        { id: 'tiktok-audio', label: `${item.music?.title ?? 'Música'}`, sub: 'MP3 · TikTok Music', size: '—', best: true, url: audioUrl },
      ] : [{ id: 'na', label: 'Sem áudio', sub: 'Faixa não encontrada', size: '—' }],
    },
  };
}

async function extractTikTokPage(_url: string, videoId: string): Promise<MediaInfo> {
  const res = await apiFetch(`https://www.tiktok.com/@user/video/${videoId}`, {
    headers: { 'User-Agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1' },
    rawText: true,
  });

  const match = res.text.match(/<script id="__UNIVERSAL_DATA_FOR_REHYDRATION__"[^>]*>([\s\S]*?)<\/script>/);
  if (!match) throw new Error('Could not parse TikTok page');

  const pageData = JSON.parse(match[1]);
  const item = pageData?.['__DEFAULT_SCOPE__']?.['webapp.video-detail']?.['itemInfo']?.['itemStruct'] as {
    desc: string;
    author: { uniqueId: string; nickname: string };
    stats?: { playCount: number };
    video: { duration: number; downloadAddr?: string; playAddr?: string; cover?: string };
    music?: { playUrl?: string; title?: string };
  } | undefined;

  if (!item) throw new Error('No video data in TikTok page');

  const videoUrl = item.video.downloadAddr ?? item.video.playAddr;
  const durSec = item.video.duration / 1000;

  return {
    platform: 'tiktok',
    title: item.desc || 'TikTok Video',
    author: `@${item.author.uniqueId}`,
    authorFull: item.author.nickname,
    duration: formatDuration(Math.round(durSec)),
    views: item.stats ? formatCount(item.stats.playCount) : undefined,
    thumbHue: 320,
    thumbHue2: 260,
    thumbnailUrl: item.video.cover,
    formats: {
      video: videoUrl ? [
        { id: 'tiktok-hd', label: 'HD', sub: 'MP4 · TikTok', size: `~${(durSec * 0.3).toFixed(1)} MB`, best: true, url: videoUrl },
      ] : [{ id: 'na', label: 'Indisponível', sub: 'URL não encontrada', size: '—' }],
      audio: item.music?.playUrl ? [
        { id: 'tiktok-audio', label: item.music.title ?? 'Música', sub: 'MP3 · TikTok Music', size: '—', best: true, url: item.music.playUrl },
      ] : [{ id: 'na', label: 'Sem áudio', sub: '—', size: '—' }],
    },
  };
}

async function extractTikTokWebFallback(url: string): Promise<MediaInfo> {
  const res = await apiFetch(`https://www.tiktok.com/oembed?url=${encodeURIComponent(url)}`);
  const oembed = res.data as { title?: string; author_name?: string; thumbnail_url?: string } | null;
  return {
    platform: 'tiktok',
    title: oembed?.title ?? 'TikTok Video',
    author: oembed?.author_name ?? 'TikTok',
    duration: '—',
    thumbHue: 320,
    thumbHue2: 260,
    thumbnailUrl: oembed?.thumbnail_url,
    webLimitedPlatform: true,
    formats: {
      video: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
    },
  };
}

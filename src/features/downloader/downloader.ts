import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { downloadUrlInBrowser } from '../../shared/platform/browserDownload';
import { getDownloadDir, joinPath } from '../../shared/platform/files';
import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { debugWarn } from '../../shared/debug/logger';
import type { MediaInfo, FormatOption } from './types';
import { detectPlatform } from './platforms';
import { downloadService } from './DownloadService';
import {
  cleanOptional,
  extractInstagramCode,
  extractRedditPostId,
  extractTikTokVideoId,
  extractTwitterId,
  extractYoutubeId,
  formatBytes,
  formatCount,
  formatDuration,
  getDownloadExtension,
  isTwitterProfileUrl,
} from './downloaderUtils';

export interface TwitterRequestOptions {
  twitterCookies?: string;
}

export interface ApiOptions {
  method?: 'GET' | 'POST';
  headers?: Record<string, string>;
  body?: any;
  rawText?: boolean;
}

export async function apiFetch(url: string, opts: ApiOptions = {}): Promise<any> {
  const fetchOpts: any = {
    method: opts.method ?? 'GET',
    headers: opts.headers,
  };
  if (opts.body !== undefined) {
    fetchOpts.body = typeof opts.body === 'string' ? opts.body : JSON.stringify(opts.body);
  }
  const res = await tauriFetch(url, fetchOpts);
  const text = await res.text();
  let data: any = text;
  if (!opts.rawText) {
    try { data = JSON.parse(text); } catch { /* keep as text */ }
  }
  const headers: Record<string, string> = {};
  res.headers.forEach((v: string, k: string) => { headers[k] = v; });
  return { status: res.status, data, text, headers };
}

// ─── Platform extractors ───────────────────────────────────────────────────────

// ── Reddit ────────────────────────────────────────────────────────────────────
// Reddit's JSON API has CORS headers — works on both web and native.

interface RedditPost {
  title: string;
  author: string;
  score: number;
  media?: { reddit_video?: { fallback_url: string; duration: number; height: number } };
  preview?: { images?: Array<{ source: { url: string } }> };
  url: string;
  is_video: boolean;
}

async function extractReddit(url: string): Promise<MediaInfo> {
  const postId = extractRedditPostId(url);
  if (!postId) throw new Error('Cannot parse Reddit URL');

  const res = await apiFetch(
    `https://www.reddit.com/comments/${postId}.json?raw_json=1`,
    { headers: { Accept: 'application/json' } }
  );
  if (res.status >= 400) throw new Error(`Reddit API ${res.status}`);

  const post: RedditPost = (res.data as [{ data: { children: [{ data: RedditPost }] } }])
    [0]?.data?.children?.[0]?.data;
  if (!post) throw new Error('Post not found');

  const rv = post.media?.reddit_video;
  const thumbRaw = post.preview?.images?.[0]?.source?.url ?? '';
  const thumbnailUrl = thumbRaw.replace(/&amp;/g, '&') || undefined;
  const duration = rv?.duration ? formatDuration(rv.duration) : '—';
  const durSec = rv?.duration ?? 30;

  const videoUrl = rv?.fallback_url?.split('?')[0];
  // Reddit hosts audio separately at DASH_audio.mp4
  const audioUrl = videoUrl ? videoUrl.replace(/DASH_\d+\.mp4$/, 'DASH_audio.mp4') : undefined;

  return {
    platform: 'reddit',
    title: post.title,
    author: `u/${post.author}`,
    duration,
    views: post.score > 1000 ? `${(post.score / 1000).toFixed(1)}K` : String(post.score),
    thumbHue: 14, thumbHue2: 0,
    thumbnailUrl,
    formats: {
      video: rv ? [
        { id: 'best',  label: 'Melhor', sub: 'MP4 · Reddit',   size: `~${(durSec * 0.22).toFixed(1)} MB`, best: true, url: videoUrl },
        { id: 'audio-mp4', label: 'Áudio', sub: 'MP4 · Reddit Audio', size: `~${(durSec * 0.05).toFixed(1)} MB`, url: audioUrl },
      ] : [
        { id: 'link', label: 'Abrir link', sub: 'Imagem ou link externo', size: '—', url: post.url },
      ],
      audio: audioUrl ? [
        { id: 'audio', label: 'Áudio MP4', sub: 'AAC · Reddit', size: `~${(durSec * 0.05).toFixed(1)} MB`, best: true, url: audioUrl },
      ] : [
        { id: 'none', label: 'Sem áudio', sub: 'Vídeo sem faixa de áudio', size: '—' },
      ],
    },
  };
}

// ── YouTube ───────────────────────────────────────────────────────────────────
// Uses the internal Android client API. Returns signed streaming URLs.
// CORS-blocked on web browsers → metadata-only fallback.

interface YtFormat {
  itag: number;
  url?: string;
  signatureCipher?: string;
  mimeType: string;
  quality: string;
  qualityLabel?: string;
  bitrate?: number;
  contentLength?: string;
  width?: number;
  height?: number;
  audioSampleRate?: string;
  audioQuality?: string;
  approxDurationMs?: string;
}

interface YtPlayerResponse {
  videoDetails?: {
    videoId: string;
    title: string;
    author: string;
    lengthSeconds: string;
    viewCount: string;
    thumbnail?: { thumbnails: Array<{ url: string; width: number; height: number }> };
  };
  streamingData?: {
    formats: YtFormat[];
    adaptiveFormats: YtFormat[];
    expiresInSeconds?: string;
  };
  playabilityStatus?: { status: string; reason?: string };
}



async function extractYoutube(url: string): Promise<MediaInfo> {
  const videoId = extractYoutubeId(url);
  if (!videoId) throw new Error('Cannot parse YouTube URL');

  // To avoid bot detection on the API, we extract the ytInitialPlayerResponse directly from the HTML
  const res = await apiFetch(`https://www.youtube.com/watch?v=${videoId}`, {
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
    },
    rawText: true
  });

  const startStr = 'ytInitialPlayerResponse = ';
  const startIdx = res.text.indexOf(startStr);
  if (startIdx === -1) {
    throw new Error('Não foi possível extrair os dados do vídeo do YouTube. Tente novamente mais tarde.');
  }

  let jsonStr = res.text.slice(startIdx + startStr.length);
  const endScriptIdx = jsonStr.indexOf(';</script>');
  if (endScriptIdx !== -1) jsonStr = jsonStr.slice(0, endScriptIdx);
  const endVarIdx = jsonStr.indexOf('};var ');
  if (endVarIdx !== -1) jsonStr = jsonStr.slice(0, endVarIdx + 1);

  let data: YtPlayerResponse;
  try {
    data = JSON.parse(jsonStr) as YtPlayerResponse;
  } catch (err) {
    throw new Error('Falha ao processar os dados do YouTube (JSON inválido).');
  }
  
  if (data.playabilityStatus?.status === 'ERROR' || data.playabilityStatus?.status === 'LOGIN_REQUIRED') {
    throw new Error(data.playabilityStatus.reason ?? 'Vídeo indisponível ou requer login.');
  }

  const details = data.videoDetails;
  const streaming = data.streamingData;

  const muxed = (streaming?.formats ?? []);
  const videoOnly = (streaming?.adaptiveFormats ?? []).filter(
    (f) => f.mimeType.startsWith('video/')
  );

  // Group all video formats (muxed and adaptive)
  const allVideos = [...muxed, ...videoOnly].sort((a, b) => (b.height ?? 0) - (a.height ?? 0));

  const uniqueVideos: FormatOption[] = [];
  const seenQualities = new Set<string>();

  for (let i = 0; i < allVideos.length; i++) {
    const f = allVideos[i];
    const isAdaptive = !muxed.includes(f);
    let label = f.qualityLabel ?? f.quality ?? 'Desconhecido';

    if (!seenQualities.has(label)) {
      seenQualities.add(label);
      uniqueVideos.push({
        id: String(f.itag),
        label,
        sub: `MP4 · ${f.quality}`,
        size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
        best: uniqueVideos.length === 0 && !isAdaptive,
        url: f.url,
        hasAudio: !isAdaptive
      });
    } else if (!isAdaptive) {
      // Prefer muxed (with audio) over adaptive for the same quality label
      const existingIdx = uniqueVideos.findIndex(v => v.label === label);
      if (existingIdx !== -1 && !uniqueVideos[existingIdx].hasAudio) {
        uniqueVideos[existingIdx] = {
          id: String(f.itag),
          label,
          sub: `MP4 · ${f.quality}`,
          size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
          best: existingIdx === 0,
          url: f.url,
          hasAudio: true
        };
      }
    }
  }

  const videoFormats: FormatOption[] = uniqueVideos;

  if (videoFormats.length === 0) {
    videoFormats.push({ id: 'best', label: 'Melhor Qualidade', sub: 'Muxed stream', size: '—' });
  }

  const audioOnly = (streaming?.adaptiveFormats ?? []).filter(
    (f) => f.mimeType.startsWith('audio/')
  );

  const audioFormats: FormatOption[] = audioOnly.slice(0, 3).map((f, i) => ({
    id: String(f.itag),
    label: f.audioQuality === 'AUDIO_QUALITY_HIGH' ? 'Alta qualidade' : f.audioQuality === 'AUDIO_QUALITY_MEDIUM' ? 'Qualidade média' : 'Compacto',
    sub: f.mimeType.split(';')[0].replace('audio/', '').toUpperCase() + (f.audioSampleRate ? ` · ${Math.round(parseInt(f.audioSampleRate) / 1000)}kHz` : ''),
    size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
    best: i === 0,
    url: f.url,
  }));

  if (audioFormats.length === 0) {
    audioFormats.push({ id: 'mp3-mock', label: 'MP3 256kbps', sub: 'Extraído do vídeo', size: '—' });
  }

  const thumb = details?.thumbnail?.thumbnails;
  const thumbnailUrl = thumb ? thumb[thumb.length - 1]?.url : undefined;

  return {
    platform: 'youtube',
    title: details?.title ?? 'YouTube Video',
    author: details?.author ?? 'Unknown',
    duration: details?.lengthSeconds ? formatDuration(parseInt(details.lengthSeconds)) : '—',
    views: details?.viewCount
      ? parseInt(details.viewCount) > 1_000_000
        ? `${(parseInt(details.viewCount) / 1_000_000).toFixed(1)}M`
        : `${(parseInt(details.viewCount) / 1000).toFixed(0)}K`
      : undefined,
    thumbHue: 0, thumbHue2: 20,
    thumbnailUrl,
    originalUrl: url,
    formats: { video: videoFormats, audio: audioFormats },
  };
}



// ── TikTok ────────────────────────────────────────────────────────────────────
// Uses TikTok's internal aweme API. Requires native HTTP (CORS blocked on web).

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

async function extractTikTok(url: string): Promise<MediaInfo> {
  if (!runtimeCapabilities.isTauri) {
    return extractTikTokWebFallback(url);
  }

  // Resolve redirects to get canonical URL with video ID
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
    // Try page-scraping fallback
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
    thumbHue: 320, thumbHue2: 260,
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

  // Extract JSON from __UNIVERSAL_DATA_FOR_REHYDRATION__
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
    thumbHue: 320, thumbHue2: 260,
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
    thumbHue: 320, thumbHue2: 260,
    thumbnailUrl: oembed?.thumbnail_url,
    webLimitedPlatform: true,
    formats: {
      video: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
    },
  };
}

// ── Instagram ─────────────────────────────────────────────────────────────────
// Instagram requires authentication for the full API.
// We extract from the GraphQL endpoint which still works without auth for some posts.

async function extractInstagram(url: string): Promise<MediaInfo> {
  const code = extractInstagramCode(url);
  if (!code) throw new Error('Cannot parse Instagram URL');

  if (!runtimeCapabilities.isTauri) {
    return extractInstagramOembed(url);
  }

  // Try the GraphQL endpoint
  const gqlUrl = `https://www.instagram.com/graphql/query?query_hash=2c4c2e343a8f64c625ba02b2aa12c7f8&variables=${encodeURIComponent(JSON.stringify({ shortcode: code }))}`;
  const res = await apiFetch(gqlUrl, {
    headers: {
      'User-Agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15',
      'Accept': 'application/json',
      'X-IG-App-ID': '936619743392459',
    },
  });

  if (res.status === 200 && res.data?.data?.shortcode_media) {
    const m = res.data.data.shortcode_media as {
      edge_media_to_caption?: { edges: [{ node: { text: string } }] };
      owner?: { username: string; full_name?: string };
      video_duration?: number;
      video_url?: string;
      display_url?: string;
      is_video?: boolean;
      video_view_count?: number;
    };

    const title = m.edge_media_to_caption?.edges?.[0]?.node?.text ?? 'Instagram Post';
    const videoUrl = m.is_video ? m.video_url : undefined;
    const durSec = m.video_duration ?? 30;

    return {
      platform: 'instagram',
      title: title.slice(0, 120),
      author: `@${m.owner?.username ?? 'instagram'}`,
      authorFull: m.owner?.full_name,
      duration: m.is_video ? formatDuration(Math.round(durSec)) : '—',
      views: m.video_view_count ? formatCount(m.video_view_count) : undefined,
      thumbHue: 300, thumbHue2: 240,
      thumbnailUrl: m.display_url,
      formats: {
        video: videoUrl ? [
          { id: 'ig-hd', label: 'HD', sub: 'MP4 · Instagram', size: `~${(durSec * 0.4).toFixed(1)} MB`, best: true, url: videoUrl },
        ] : [{ id: 'na', label: 'Imagem', sub: 'Não é um vídeo', size: '—', url: m.display_url }],
        audio: videoUrl ? [
          { id: 'ig-audio', label: 'Áudio', sub: 'AAC · Instagram', size: '—', url: videoUrl },
        ] : [{ id: 'na', label: 'Sem áudio', sub: '—', size: '—' }],
      },
    };
  }

  // GQL failed — try scraping the embed page
  return extractInstagramEmbed(url, code);
}

async function extractInstagramEmbed(_url: string, code: string): Promise<MediaInfo> {
  const res = await apiFetch(`https://www.instagram.com/p/${code}/embed/captioned`, {
    headers: { 'User-Agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15' },
    rawText: true,
  });

  const videoMatch = res.text.match(/"video_url":"([^"]+)"/);
  const thumbMatch = res.text.match(/"thumbnail_src":"([^"]+)"/);
  const ownerMatch = res.text.match(/"owner":\{"id":"[^"]+","username":"([^"]+)"/);
  const captionMatch = res.text.match(/"edge_media_to_caption":\{"edges":\[\{"node":\{"text":"([^"]+)"/);

  const videoUrl = videoMatch?.[1]?.replace(/\\u0026/g, '&').replace(/\\\//g, '/');
  const thumbUrl = thumbMatch?.[1]?.replace(/\\u0026/g, '&').replace(/\\\//g, '/');

  if (!videoUrl) throw new Error('Could not extract Instagram video URL');

  return {
    platform: 'instagram',
    title: captionMatch?.[1] ?? 'Instagram Video',
    author: `@${ownerMatch?.[1] ?? 'instagram'}`,
    duration: '—',
    thumbHue: 300, thumbHue2: 240,
    thumbnailUrl: thumbUrl,
    formats: {
      video: [{ id: 'ig-hd', label: 'HD', sub: 'MP4 · Instagram', size: '—', best: true, url: videoUrl }],
      audio: [{ id: 'ig-audio', label: 'Áudio', sub: 'AAC · Instagram', size: '—', url: videoUrl }],
    },
  };

}

async function extractInstagramOembed(url: string): Promise<MediaInfo> {
  const res = await apiFetch(`https://www.instagram.com/oembed?url=${encodeURIComponent(url)}&format=json`);
  const oembed = res.data as { title?: string; author_name?: string; thumbnail_url?: string } | null;
  return {
    platform: 'instagram',
    title: oembed?.title ?? 'Instagram Post',
    author: oembed?.author_name ?? 'Instagram',
    duration: '—',
    thumbHue: 300, thumbHue2: 240,
    thumbnailUrl: oembed?.thumbnail_url,
    webLimitedPlatform: true,
    formats: {
      video: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
    },
  };
}

// ── Twitter / X ───────────────────────────────────────────────────────────────
// Uses Twitter/X's public syndication endpoint as the browser fallback.

async function extractTwitter(url: string): Promise<MediaInfo> {
  const tweetId = extractTwitterId(url);
  if (!tweetId) throw new Error('Nao foi possivel identificar o tweet. Cole uma URL /status/... do Twitter/X.');

  return extractTwitterSyndication(url, tweetId);
}

async function extractTwitterSyndication(_url: string, tweetId: string): Promise<MediaInfo> {
  // Compute Twitter syndication token (deterministic from tweet ID)
  const token = ((Number(tweetId) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, '');
  const syndicationUrl = `https://cdn.syndication.twimg.com/tweet-result?id=${tweetId}&token=${token}&lang=en`;
  console.info('[twitter] fetching syndication fallback', { tweetId, token });

  const res = await apiFetch(syndicationUrl);
  const tweet = res.data as {
    text?: string;
    user?: { screen_name?: string; name?: string };
    mediaDetails?: Array<{
      type?: string;
      media_url_https?: string;
      video_info?: { duration_millis?: number; variants?: Array<{ content_type?: string; bitrate?: number; url?: string }> };
    }>;
  } | null;

  if (!tweet) throw new Error('Tweet not found or private');

  const media = tweet.mediaDetails ?? [];
  const videoMedia = media.find((m) => m.type === 'video' || m.type === 'animated_gif');
  const text = (tweet.text ?? '').replace(/https?:\/\/t\.co\/\S+/g, '').trim() || 'Twitter Video';

  let videoFormats: FormatOption[] = [];
  if (videoMedia?.video_info?.variants) {
    const variants = videoMedia.video_info.variants
      .filter((v) => v.content_type === 'video/mp4' && v.url)
      .sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0));
    videoFormats = variants.map((v, i) => ({
      id: `tw-${i}`,
      label: (v.bitrate ?? 0) > 1500000 ? '1080p' : (v.bitrate ?? 0) > 800000 ? '720p' : '360p',
      sub: `MP4 · ${v.content_type}`,
      size: '—',
      best: i === 0,
      url: v.url,
    }));
  }

  return {
    platform: 'twitter',
    title: text.slice(0, 120),
    author: `@${tweet.user?.screen_name ?? 'twitter'}`,
    authorFull: tweet.user?.name,
    duration: videoMedia?.video_info?.duration_millis
      ? formatDuration(Math.round(videoMedia.video_info.duration_millis / 1000))
      : '—',
    thumbHue: 200, thumbHue2: 220,
    thumbnailUrl: videoMedia?.media_url_https ?? media[0]?.media_url_https,
    webLimitedPlatform: videoFormats.length === 0,
    formats: {
      video: videoFormats.length > 0 ? videoFormats : [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: videoFormats.length > 0 ? [{ id: 'tw-audio', label: 'Áudio', sub: 'MP4 · Twitter', size: '—', best: true, url: videoFormats[0]?.url }] : [{ id: 'web-limit', label: '—', sub: '—', size: '—' }],
    },
  };
}

// ─── Utilities ─────────────────────────────────────────────────────────────────

async function resolveRedirects(url: string): Promise<string> {
  try {
    if (runtimeCapabilities.isTauri) {
      // HEAD request to follow redirects
      const res = await apiFetch(url, { method: 'GET', headers: { 'User-Agent': 'Mozilla/5.0' }, rawText: true });
      // Native HTTP follows redirects and may expose the final URL in headers.
      return (res.headers['x-final-url'] ?? res.headers['location'] ?? url) as string;
    }
    const res = await fetch(url, { method: 'HEAD', mode: 'no-cors' });
    return res.url || url;
  } catch {
    return url;
  }
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
    console.error("Download failed", err);
    downloadService.updateDownload(downloadId, { status: 'failed', error: err.message });
  }
}

export function isNative(): boolean {
  return runtimeCapabilities.isTauri;
}

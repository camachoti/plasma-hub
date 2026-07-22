import type { FormatOption, MediaInfo } from '../types';
import { apiFetch } from '../downloaderApi';
import { extractTwitterId, formatDuration } from '../downloaderUtils';

export async function extractTwitter(url: string): Promise<MediaInfo> {
  const tweetId = extractTwitterId(url);
  if (!tweetId) throw new Error('Nao foi possivel identificar o tweet. Cole uma URL /status/... do Twitter/X.');

  return extractTwitterSyndication(tweetId);
}

async function extractTwitterSyndication(tweetId: string): Promise<MediaInfo> {
  const token = ((Number(tweetId) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, '');
  const syndicationUrl = `https://cdn.syndication.twimg.com/tweet-result?id=${tweetId}&token=${token}&lang=en`;

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
    thumbHue: 200,
    thumbHue2: 220,
    thumbnailUrl: videoMedia?.media_url_https ?? media[0]?.media_url_https,
    webLimitedPlatform: videoFormats.length === 0,
    formats: {
      video: videoFormats.length > 0 ? videoFormats : [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: videoFormats.length > 0 ? [{ id: 'tw-audio', label: 'Áudio', sub: 'MP4 · Twitter', size: '—', best: true, url: videoFormats[0]?.url }] : [{ id: 'web-limit', label: '—', sub: '—', size: '—' }],
    },
  };
}

import type { MediaInfo } from '../types';
import { apiFetch } from '../downloaderApi';
import { extractRedditPostId, formatDuration } from '../downloaderUtils';

interface RedditPost {
  title: string;
  author: string;
  score: number;
  media?: { reddit_video?: { fallback_url: string; duration: number; height: number } };
  preview?: { images?: Array<{ source: { url: string } }> };
  url: string;
  is_video: boolean;
}

export async function extractReddit(url: string): Promise<MediaInfo> {
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
  const audioUrl = videoUrl ? videoUrl.replace(/DASH_\d+\.mp4$/, 'DASH_audio.mp4') : undefined;

  return {
    platform: 'reddit',
    title: post.title,
    author: `u/${post.author}`,
    duration,
    views: post.score > 1000 ? `${(post.score / 1000).toFixed(1)}K` : String(post.score),
    thumbHue: 14,
    thumbHue2: 0,
    thumbnailUrl,
    formats: {
      video: rv ? [
        { id: 'best', label: 'Melhor', sub: 'MP4 · Reddit', size: `~${(durSec * 0.22).toFixed(1)} MB`, best: true, url: videoUrl },
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

import { runtimeCapabilities } from '../../../shared/platform/runtime';
import type { MediaInfo } from '../types';
import { apiFetch } from '../downloaderApi';
import { extractInstagramCode, formatCount, formatDuration } from '../downloaderUtils';

export async function extractInstagram(url: string): Promise<MediaInfo> {
  const code = extractInstagramCode(url);
  if (!code) throw new Error('Cannot parse Instagram URL');

  if (!runtimeCapabilities.isTauri) {
    return extractInstagramOembed(url);
  }

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
      thumbHue: 300,
      thumbHue2: 240,
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
    thumbHue: 300,
    thumbHue2: 240,
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
    thumbHue: 300,
    thumbHue2: 240,
    thumbnailUrl: oembed?.thumbnail_url,
    webLimitedPlatform: true,
    formats: {
      video: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
      audio: [{ id: 'web-limit', label: 'App nativo necessário', sub: 'Instale no Android/iOS', size: '—' }],
    },
  };
}

export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export function formatBytes(bytes: number): string {
  if (!bytes) return '—';
  const mb = bytes / 1024 / 1024;
  return mb < 1 ? `${(mb * 1024).toFixed(0)} KB` : `${mb.toFixed(1)} MB`;
}

export function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

export function extractYoutubeId(url: string): string | null {
  for (const p of [
    /(?:youtube\.com\/watch\?v=|youtu\.be\/)([a-zA-Z0-9_-]{11})/,
    /youtube\.com\/(?:embed|shorts|v)\/([a-zA-Z0-9_-]{11})/,
  ]) {
    const m = url.match(p);
    if (m) return m[1];
  }
  return null;
}

export function extractRedditPostId(url: string): string | null {
  const m = url.match(/reddit\.com\/(?:r\/[^/]+\/)?comments\/([a-zA-Z0-9]+)/);
  return m ? m[1] : null;
}

export function extractTikTokVideoId(url: string): string | null {
  const m = url.match(/\/video\/(\d+)/);
  return m ? m[1] : null;
}

export function extractTwitterId(url: string): string | null {
  const m = url.match(/(?:twitter\.com|x\.com)\/(?:\w+\/status|i\/web\/status)\/(\d+)/);
  return m ? m[1] : null;
}

export function isTwitterProfileUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, '');
    if (host !== 'twitter.com' && host !== 'x.com') return false;

    const parts = parsed.pathname.split('/').filter(Boolean);
    return parts.length === 1 && /^[A-Za-z0-9_]{1,15}$/.test(parts[0]);
  } catch {
    return /(?:twitter\.com|x\.com)\/[A-Za-z0-9_]{1,15}\/?$/.test(url);
  }
}

export function cleanOptional(value?: string): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

export function extractInstagramCode(url: string): string | null {
  const m = url.match(/instagram\.com\/(?:p|reel|tv)\/([A-Za-z0-9_-]+)/);
  return m ? m[1] : null;
}

export function getDownloadExtension(platform: string, mode: 'video' | 'audio', url?: string, formatDescription = ''): string {
  if (url) {
    try {
      const parsed = new URL(url);
      const extension = parsed.pathname.toLowerCase().match(/\.([a-z0-9]{2,5})$/)?.[1];
      if (extension && ['jpg', 'jpeg', 'png', 'webp', 'gif', 'mp4', 'mov', 'm4a', 'mp3', 'webm', 'opus', 'aac'].includes(extension)) {
        return extension === 'jpeg' ? 'jpg' : extension;
      }
      const imageFormat = parsed.searchParams.get('format');
      if (imageFormat && ['jpg', 'jpeg', 'png', 'webp'].includes(imageFormat)) return imageFormat === 'jpeg' ? 'jpg' : imageFormat;
    } catch { /* A native extractor may supply an opaque URL. */ }
  }

  if (mode === 'audio') {
    if (/webm|opus/i.test(formatDescription)) return 'webm';
    if (/mp3/i.test(formatDescription)) return 'mp3';
    if (/aac|m4a|mp4/i.test(formatDescription)) return 'm4a';
    return platform === 'youtube' ? 'm4a' : 'mp4';
  }
  if (/webm/i.test(formatDescription)) return 'webm';
  return 'mp4';
}

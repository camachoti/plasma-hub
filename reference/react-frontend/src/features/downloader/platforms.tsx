import type { PlatformId } from './types';

export interface PlatformDef {
  name: string;
  color: string;
  gradient: string;
  icon: (size: number, color: string) => React.ReactElement;
  thumbHue: number;
  thumbHue2: number;
}

export const PLATFORMS: Record<PlatformId, PlatformDef> = {
  instagram: {
    name: 'Instagram',
    color: '#E4405F',
    gradient: 'linear-gradient(135deg, #FEDA77 0%, #F58529 25%, #DD2A7B 50%, #8134AF 75%, #515BD4 100%)',
    thumbHue: 300,
    thumbHue2: 240,
    icon: (s, c) => (
      <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
        <rect x="2.5" y="2.5" width="19" height="19" rx="5.5" stroke={c} strokeWidth="1.8"/>
        <circle cx="12" cy="12" r="4" stroke={c} strokeWidth="1.8"/>
        <circle cx="17.5" cy="6.5" r="1.2" fill={c}/>
      </svg>
    ),
  },
  tiktok: {
    name: 'TikTok',
    color: '#000000',
    gradient: 'linear-gradient(135deg, #25F4EE 0%, #000000 50%, #FE2C55 100%)',
    thumbHue: 320,
    thumbHue2: 260,
    icon: (s, c) => (
      <svg width={s} height={s} viewBox="0 0 24 24" fill={c}>
        <path d="M19.6 6.4a5.4 5.4 0 0 1-3.6-1.4 5.4 5.4 0 0 1-1.6-3.5h-3.5v13.3a2.6 2.6 0 1 1-2.6-2.6c.3 0 .5 0 .8.1V8.7a6.2 6.2 0 0 0-.8 0 6.1 6.1 0 1 0 6.1 6.1V8.4a8.9 8.9 0 0 0 5.2 1.7v-3.7z"/>
      </svg>
    ),
  },
  youtube: {
    name: 'YouTube',
    color: '#FF0033',
    gradient: 'linear-gradient(135deg, #FF0033 0%, #B00020 100%)',
    thumbHue: 0,
    thumbHue2: 20,
    icon: (s, c) => (
      <svg width={s} height={s} viewBox="0 0 24 24" fill={c}>
        <path d="M22.5 6.4a2.7 2.7 0 0 0-1.9-1.9C18.9 4 12 4 12 4s-6.9 0-8.6.5A2.7 2.7 0 0 0 1.5 6.4 28 28 0 0 0 1 12a28 28 0 0 0 .5 5.6 2.7 2.7 0 0 0 1.9 1.9C5.1 20 12 20 12 20s6.9 0 8.6-.5a2.7 2.7 0 0 0 1.9-1.9A28 28 0 0 0 23 12a28 28 0 0 0-.5-5.6zM9.8 15.3V8.7l5.7 3.3-5.7 3.3z"/>
      </svg>
    ),
  },
  twitter: {
    name: 'X',
    color: '#000000',
    gradient: 'linear-gradient(135deg, #000000 0%, #2C2C2C 100%)',
    thumbHue: 200,
    thumbHue2: 240,
    icon: (s, c) => (
      <svg width={s} height={s} viewBox="0 0 24 24" fill={c}>
        <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
      </svg>
    ),
  },
  reddit: {
    name: 'Reddit',
    color: '#FF4500',
    gradient: 'linear-gradient(135deg, #FF4500 0%, #CC3700 100%)',
    thumbHue: 14,
    thumbHue2: 0,
    icon: (s, c) => (
      <svg width={s} height={s} viewBox="0 0 24 24" fill={c}>
        <path d="M22 12.07c0-1.21-.99-2.2-2.2-2.2a2.2 2.2 0 0 0-1.5.6c-1.45-.97-3.36-1.59-5.49-1.66l1.13-3.55 3.07.74a1.55 1.55 0 1 0 .15-1.32l-3.42-.82a.5.5 0 0 0-.6.34l-1.27 4a11.6 11.6 0 0 0-5.5 1.65 2.2 2.2 0 0 0-3.7 1.62c0 .77.4 1.45 1 1.85a4.7 4.7 0 0 0-.06.74c0 3.05 3.55 5.51 7.94 5.51s7.94-2.46 7.94-5.51c0-.25-.02-.5-.06-.74.6-.4 1-1.08 1-1.85zM7 13.5a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0zm8.7 4.13c-1.06.85-2.83 1.27-4.7 1.27s-3.64-.42-4.7-1.27a.5.5 0 1 1 .63-.78c.86.69 2.4 1.05 4.07 1.05s3.21-.36 4.07-1.05a.5.5 0 1 1 .63.78zM15.5 15a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3z"/>
      </svg>
    ),
  },
};

export function detectPlatform(url: string): PlatformId | null {
  const u = url.toLowerCase();
  if (u.includes('tiktok')) return 'tiktok';
  if (u.includes('instagram') || u.includes('instagr.am')) return 'instagram';
  if (u.includes('youtube') || u.includes('youtu.be')) return 'youtube';
  if (u.includes('twitter') || u.includes('x.com')) return 'twitter';
  if (u.includes('reddit')) return 'reddit';
  return null;
}

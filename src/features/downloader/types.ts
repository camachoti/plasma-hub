export type ThemeId = 'ivory' | 'obsidian' | 'plasma';
export type LangId = 'pt' | 'en';
export type PlatformId = 'instagram' | 'tiktok' | 'youtube' | 'twitter' | 'reddit';
export type ViewId = 'empty' | 'analyzing' | 'preview' | 'downloading' | 'done' | 'history' | 'settings';
export type MediaMode = 'video' | 'audio';

export interface Theme {
  name: string;
  bg: string;
  bg2: string;
  surface: string;
  surfaceMuted: string;
  ink: string;
  inkMuted: string;
  inkSubtle: string;
  accent: string;
  accentInk: string;
  accent2: string;
  border: string;
  borderStrong: string;
  success: string;
  danger: string;
  statusBarDark: boolean;
}

export interface FormatOption {
  id: string;
  label: string;
  sub: string;
  size: string;
  best?: boolean;
  url?: string;
  hasAudio?: boolean;
}

export interface HistoryItem {
  id: string;
  platform: PlatformId;
  title: string;
  author: string;
  duration: string;
  fileSize: string;
  format: string;
  when: 'today' | 'yesterday' | 'earlier';
  thumbHue: number;
  thumbHue2: number;
  thumbnailUrl?: string;
}

export interface MediaInfo {
  platform: PlatformId;
  title: string;
  author: string;
  authorFull?: string;
  duration: string;
  views?: string;
  likes?: string;
  thumbHue: number;
  thumbHue2: number;
  thumbnailUrl?: string;
  originalUrl?: string;
  /** True when running on web browser and this platform requires the native app for downloads */
  webLimitedPlatform?: boolean;
  formats: {
    video: FormatOption[];
    audio: FormatOption[];
  };
}

export interface AppStrings {
  appName: string;
  tagline: string;
  pasteHint: string;
  pastePlaceholder: string;
  paste: string;
  analyze: string;
  analyzing: string;
  fetchingMeta: string;
  chooseFormat: string;
  video: string;
  audio: string;
  quality: string;
  download: string;
  downloading: string;
  of: string;
  speed: string;
  eta: string;
  cancel: string;
  done: string;
  saved: string;
  openFile: string;
  downloadAnother: string;
  history: string;
  today: string;
  yesterday: string;
  earlier: string;
  recent: string;
  seeAll: string;
  redownload: string;
  share: string;
  delete: string;
  supportedSites: string;
  pasteFromClipboard: string;
  invalidUrl: string;
  duration: string;
  by: string;
  settings: string;
  onlyAudio: string;
  bestQuality: string;
  smallerSize: string;
  fileSize: string;
  format: string;
  home: string;
  library: string;
  pasteToBegin: string;
  fromAnywhere: string;
  detectedFrom: string;
  noInternetError: string;
  fetchError: string;
  webLimitWarning: string;
}

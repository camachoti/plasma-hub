export type RuntimeKind = "desktop" | "android" | "webview";

export interface RuntimeCapabilities {
  kind: RuntimeKind;
  isTauri: boolean;
  isAndroid: boolean;
  supportsNativeYoutube: boolean;
  supportsNativeTwitter: boolean;
  supportsTdlib: boolean;
  supportsOpenInFolder: boolean;
  supportsSystemDownloads: boolean;
  supportsServiceWorker: boolean;
}

function hasTauriRuntime() {
  return Boolean((window as any).__TAURI_INTERNALS__);
}

function hasAndroidUserAgent() {
  return /Android/i.test(navigator.userAgent);
}

function detectRuntimeKind(): RuntimeKind {
  if (hasTauriRuntime()) return "desktop";
  if (hasAndroidUserAgent()) return "webview";
  return "webview";
}

export function getRuntimeCapabilities(): RuntimeCapabilities {
  const isTauri = hasTauriRuntime();
  const isAndroid = hasAndroidUserAgent();
  const kind: RuntimeKind = isTauri && isAndroid ? "android" : detectRuntimeKind();

  return {
    kind,
    isTauri,
    isAndroid,
    supportsNativeYoutube: isTauri && !isAndroid,
    supportsNativeTwitter: isTauri && !isAndroid,
    supportsTdlib: isTauri && !isAndroid,
    supportsOpenInFolder: isTauri && !isAndroid,
    supportsSystemDownloads: isTauri && !isAndroid,
    supportsServiceWorker: !isTauri && "serviceWorker" in navigator,
  };
}

export const runtimeCapabilities = getRuntimeCapabilities();

export const SHARED_DOWNLOAD_URL_KEY = "plasma_pending_shared_download_url";
export const SHARED_DOWNLOAD_URL_EVENT = "plasma-android-share-url";

export function takePendingSharedDownloadUrl() {
  const url = localStorage.getItem(SHARED_DOWNLOAD_URL_KEY);
  if (url) localStorage.removeItem(SHARED_DOWNLOAD_URL_KEY);
  return url;
}

export function queueSharedDownloadUrl(url: string) {
  localStorage.setItem(SHARED_DOWNLOAD_URL_KEY, url);
}

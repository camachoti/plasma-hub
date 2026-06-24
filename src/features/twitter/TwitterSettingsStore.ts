const TWITTER_COOKIES_STORAGE_KEY = 'plasma_twitter_optional_cookies';
const TWITTER_SETTINGS_EVENT = 'plasma-twitter-settings-changed';

export function getStoredTwitterCookies() {
  try {
    return localStorage.getItem(TWITTER_COOKIES_STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
}

export function setStoredTwitterCookies(cookies: string) {
  try {
    const trimmed = cookies.trim();
    if (trimmed) {
      localStorage.setItem(TWITTER_COOKIES_STORAGE_KEY, cookies);
    } else {
      localStorage.removeItem(TWITTER_COOKIES_STORAGE_KEY);
    }
  } catch {
    // Keep the setting usable for the current session even if storage is unavailable.
  }

  window.dispatchEvent(new CustomEvent(TWITTER_SETTINGS_EVENT));
}

export function onTwitterSettingsChanged(callback: () => void) {
  const sync = () => callback();
  window.addEventListener(TWITTER_SETTINGS_EVENT, sync);
  window.addEventListener('storage', sync);
  return () => {
    window.removeEventListener(TWITTER_SETTINGS_EVENT, sync);
    window.removeEventListener('storage', sync);
  };
}

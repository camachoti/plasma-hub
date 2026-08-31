import { appStorage } from '../../shared/storage/appStorage';
import { invokeCommand } from '../../shared/platform/tauri';
import { runtimeCapabilities } from '../../shared/platform/runtime';

const TWITTER_COOKIES_STORAGE_KEY = 'plasma_twitter_optional_cookies';
const TWITTER_SETTINGS_EVENT = 'plasma-twitter-settings-changed';
let cachedCookies = '';
let loadPromise: Promise<string> | null = null;

export function getStoredTwitterCookies() {
  return cachedCookies || appStorage.get(TWITTER_COOKIES_STORAGE_KEY) || '';
}

export async function loadStoredTwitterCookies() {
  if (!runtimeCapabilities.isTauri) return getStoredTwitterCookies();
  if (!loadPromise) {
    loadPromise = invokeCommand<string>('load_twitter_cookies')
      .then(async nativeCookies => {
        const legacyCookies = appStorage.get(TWITTER_COOKIES_STORAGE_KEY) || '';
        cachedCookies = nativeCookies || legacyCookies;
        if (!nativeCookies && legacyCookies) {
          await invokeCommand('save_twitter_cookies', { cookies: legacyCookies });
        }
        appStorage.remove(TWITTER_COOKIES_STORAGE_KEY);
        return cachedCookies;
      })
      .catch(() => getStoredTwitterCookies());
  }
  return loadPromise;
}

export async function setStoredTwitterCookies(cookies: string) {
  cachedCookies = cookies.trim() ? cookies : '';
  if (runtimeCapabilities.isTauri) {
    await invokeCommand('save_twitter_cookies', { cookies: cachedCookies });
    appStorage.remove(TWITTER_COOKIES_STORAGE_KEY);
  } else if (cachedCookies) {
    appStorage.set(TWITTER_COOKIES_STORAGE_KEY, cachedCookies);
  } else {
    appStorage.remove(TWITTER_COOKIES_STORAGE_KEY);
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

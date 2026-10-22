const DEBUG_STORAGE_KEY = "plasma_debug_logs";

function storageDebugEnabled() {
  try {
    return localStorage.getItem(DEBUG_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function isDebugLoggingEnabled() {
  return Boolean(import.meta.env.DEV || storageDebugEnabled());
}

export function debugLog(...args: unknown[]) {
  if (isDebugLoggingEnabled()) console.log(...args);
}

export function debugWarn(...args: unknown[]) {
  if (isDebugLoggingEnabled()) console.warn(...args);
}

export function enableDebugLogging(enabled = true) {
  localStorage.setItem(DEBUG_STORAGE_KEY, String(enabled));
}

import { invokeCommand } from "../platform/tauri";
import { runtimeCapabilities } from "../platform/runtime";

/**
 * Global preferences that moved from `localStorage` to SQLite.
 *
 * Per-chat caches and timeline anchors stay in `localStorage` until the Rust
 * timeline replaces them; these are the values the Rust shell reads at boot.
 */
const LEGACY_SETTINGS_KEYS = [
  "skip_login",
  "telegram_tdlib_session",
  "plasma_appearance_palette",
  "plasma_appearance_density",
  "plasma_info_panel_open",
  "plasma_chat_media_size",
  "plasma_twitter_optional_cookies",
  "clear_twitter_groups_on_clear_cache",
  "plasma_debug_logs",
] as const;

let inFlight: Promise<void> | null = null;

/**
 * Mirrors the current React preferences into the Rust settings store.
 *
 * Until the Phase 8 cutover, the React UI is authoritative, so this overwrites
 * SQLite on every boot. The call is fire-and-forget: a failure is logged and
 * never blocks or breaks the existing app. Once the React frontend is removed,
 * the Rust shell uses the non-overwriting import instead.
 */
export function migrateLegacySettings(): Promise<void> {
  if (!runtimeCapabilities.isTauri) return Promise.resolve();
  if (!inFlight) {
    inFlight = runMigration().catch(error => {
      // A missing command only happens when an older binary is running with a
      // newer frontend bundle; retrying on the next boot is safe.
      console.warn("[plasma] Falha ao migrar preferências para o SQLite.", error);
    });
  }
  return inFlight;
}

async function runMigration() {
  const entries: Array<[string, string]> = [];

  for (const key of LEGACY_SETTINGS_KEYS) {
    try {
      const value = window.localStorage.getItem(key);
      if (value !== null) entries.push([key, value]);
    } catch {
      // localStorage can be unavailable; keep the values we already have.
    }
  }

  if (entries.length === 0) return;
  await invokeCommand("settings_import_legacy", { entries, overwrite: true });
}

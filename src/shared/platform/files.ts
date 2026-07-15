import { runtimeCapabilities } from "./runtime";

export async function getDownloadDir() {
  if (runtimeCapabilities.isTauri) {
    const { appDataDir, downloadDir, join } = await import("@tauri-apps/api/path");
    try {
      return await downloadDir();
    } catch {
      return join(await appDataDir(), "downloads");
    }
  }

  return "";
}

export async function joinPath(...parts: string[]) {
  if (runtimeCapabilities.isTauri) {
    const { join } = await import("@tauri-apps/api/path");
    return join(...parts);
  }

  return parts.filter(Boolean).join("/");
}

export async function openSystemPath(path: string) {
  if (runtimeCapabilities.isTauri) {
    const { openPath } = await import("@tauri-apps/plugin-opener");
    return openPath(path);
  }

  window.open(path, "_blank", "noopener,noreferrer");
}

export async function revealSystemItem(path: string) {
  if (runtimeCapabilities.isTauri) {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    return revealItemInDir(path);
  }

  return openSystemPath(path);
}

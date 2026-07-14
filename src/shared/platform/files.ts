import { downloadDir, join } from "@tauri-apps/api/path";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

export async function getDownloadDir() {
  return downloadDir();
}

export async function joinPath(...parts: string[]) {
  return join(...parts);
}

export async function openSystemPath(path: string) {
  return openPath(path);
}

export async function revealSystemItem(path: string) {
  return revealItemInDir(path);
}

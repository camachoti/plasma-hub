import { open, save } from "@tauri-apps/plugin-dialog";

export function openDialog(options: Parameters<typeof open>[0]) {
  return open(options);
}

export function saveDialog(options: Parameters<typeof save>[0]) {
  return save(options);
}

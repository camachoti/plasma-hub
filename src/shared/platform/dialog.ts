import { runtimeCapabilities } from "./runtime";

export async function openDialog(options: Record<string, unknown>) {
  if (!runtimeCapabilities.isTauri) return null;

  const { open } = await import("@tauri-apps/plugin-dialog");
  return open(options);
}

export async function saveDialog(options: Record<string, unknown>) {
  if (!runtimeCapabilities.isTauri) return null;

  const { save } = await import("@tauri-apps/plugin-dialog");
  return save(options);
}

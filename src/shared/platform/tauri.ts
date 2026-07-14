import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export { convertFileSrc };

export function invokeCommand<T = unknown>(command: string, args?: Record<string, unknown>) {
  return invoke<T>(command, args);
}

export function listenEvent<T>(
  event: string,
  handler: Parameters<typeof listen<T>>[1],
) {
  return listen<T>(event, handler);
}

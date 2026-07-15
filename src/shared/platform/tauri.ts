import { runtimeCapabilities } from "./runtime";

export async function invokeCommand<T = unknown>(command: string, args?: Record<string, unknown>) {
  if (!runtimeCapabilities.isTauri) {
    throw new Error(`Comando nativo indisponível fora do Tauri: ${command}`);
  }

  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export async function listenEvent<T>(
  event: string,
  handler: (event: { payload: T }) => void,
) {
  if (!runtimeCapabilities.isTauri) {
    throw new Error(`Evento nativo indisponível fora do Tauri: ${event}`);
  }

  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, handler);
}

export function convertFileSrc(filePath: string) {
  if (!runtimeCapabilities.isTauri) return filePath;

  const tauriInternals = (window as any).__TAURI_INTERNALS__;
  if (typeof tauriInternals?.convertFileSrc === "function") {
    return tauriInternals.convertFileSrc(filePath);
  }

  return filePath;
}

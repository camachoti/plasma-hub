import { runtimeCapabilities } from "./runtime";
import { convertFileSrc as tauriConvertFileSrc } from "@tauri-apps/api/core";

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

  try {
    return tauriConvertFileSrc(filePath);
  } catch {
    // Fall through to runtime globals/fallback below.
  }

  const tauriInternals = (window as any).__TAURI_INTERNALS__;
  if (typeof tauriInternals?.convertFileSrc === "function") {
    return tauriInternals.convertFileSrc(filePath);
  }

  const tauriGlobal = (window as any).__TAURI__;
  if (typeof tauriGlobal?.core?.convertFileSrc === "function") {
    return tauriGlobal.core.convertFileSrc(filePath);
  }

  return filePath;
}

import { runtimeCapabilities } from "./runtime";

export async function platformFetch(input: RequestInfo | URL, init?: RequestInit) {
  if (runtimeCapabilities.isTauri) {
    const { fetch: tauriFetch } = await import("@tauri-apps/plugin-http");
    return tauriFetch(input, init);
  }

  return fetch(input, init);
}

import { fetch as tauriFetch } from "@tauri-apps/plugin-http";

export function platformFetch(input: Parameters<typeof tauriFetch>[0], init?: Parameters<typeof tauriFetch>[1]) {
  return tauriFetch(input, init);
}

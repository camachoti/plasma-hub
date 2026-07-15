import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { nodePolyfills } from 'vite-plugin-node-polyfills';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    react(),
    nodePolyfills({
      include: ['buffer', 'process', 'crypto', 'net', 'stream', 'util', 'path', 'os', 'http', 'https'],
      globals: {
        Buffer: true,
        global: true,
        process: true,
      },
    })
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // GramJS and its Node polyfills are intentionally isolated in a lazy Telegram chunk.
    chunkSizeWarningLimit: 1600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (
            id.includes("/telegram/") ||
            id.includes("/big-integer/") ||
            id.includes("/node-stdlib-browser/") ||
            id.includes("/crypto-browserify/") ||
            id.includes("/stream-browserify/") ||
            id.includes("/buffer/") ||
            id.includes("/readable-stream/")
          ) {
            return "telegram-vendor";
          }
          if (id.includes("/@tauri-apps/")) return "tauri-vendor";
          if (id.includes("/react/") || id.includes("/react-dom/") || id.includes("/scheduler/")) return "react-vendor";
        },
      },
    },
  },
}));

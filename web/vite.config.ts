import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// `pnpm dev` talks to a running yaoplm-server (cargo run -p yaoplm-server).
const server = process.env.YAOPLM_SERVER ?? "http://127.0.0.1:7878";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  // Tauri reads the dev server output and needs a fixed port.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: server, ws: true },
      "/health": server,
      // Covers and other pictures, served by yaoplm-server from the roots.
      "/media": server,
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}", "tests/unit/**/*.test.{ts,tsx}"],
    // The i18n test reads the Rust sources through import.meta.glob; a cached
    // transform would miss files added since and check a stale list.
    fsModuleCache: false,
  },
});

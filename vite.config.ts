/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and no screen clearing, so its own logs stay visible.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  build: {
    target: "chrome120",
    sourcemap: false,
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});

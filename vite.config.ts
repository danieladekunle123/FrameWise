/// <reference types="vitest" />
import { defineConfig } from "vite";
import { configDefaults } from "vitest/config";
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
    // Process CSS so the contrast check can read the design tokens (styles.css?raw).
    css: true,
    // e2e/ holds the Playwright tests, which drive the built app (npm run e2e).
    exclude: [...configDefaults.exclude, "e2e/**"],
  },
});

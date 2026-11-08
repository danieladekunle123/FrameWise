import { defineConfig } from "@playwright/test";

// End-to-end tests drive the real, built FrameWise (target/release/framewise.exe) through its
// WebView2 window over the DevTools protocol. Build first: `npx tauri build --no-bundle`, plus
// `cargo build --release -p framewise-checks --example test_scene` for the benchmark test.
export default defineConfig({
  testDir: "e2e",
  timeout: 180_000,
  expect: { timeout: 20_000 },
  workers: 1,
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
});

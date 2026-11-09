// FrameWise end to end: the real app, with a throwaway data folder (FRAMEWISE_DATA_DIR), so your
// own history, benchmarks and profiles are never touched.
//
// What runs: first run and scan; a safe fix applied, shown in History and undone (it changes one
// per-user Windows setting and puts it back; skipped if this PC needs no safe fix); a benchmark of
// the test scene (skipped unless frame capture works without a UAC prompt, i.e. run as admin, and
// the PC has a real graphics adapter; GitHub's runners are admin but have none).
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const EXE = process.env.FRAMEWISE_EXE ?? path.resolve("target/release/framewise.exe");
const SCENE = process.env.FRAMEWISE_TEST_SCENE ?? path.resolve("target/release/examples/test_scene.exe");
const PORT = 9555;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

let app: ChildProcess;
let browser: Browser;
let page: Page;
let dataDir: string;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const invoke = <T>(cmd: string, args?: Record<string, unknown>) => page.evaluate(([c, a]) => (window as any).__TAURI_INTERNALS__.invoke(c, a), [cmd, args] as const) as Promise<T>;

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  expect(existsSync(EXE), `${EXE} not found: run "npx tauri build --no-bundle" first`).toBe(true);
  dataDir = mkdtempSync(path.join(tmpdir(), "framewise-e2e-"));
  app = spawn(EXE, [], {
    env: {
      ...process.env,
      FRAMEWISE_DATA_DIR: dataDir,
      // The window's own storage (settings, "seen the welcome screen") goes in the throwaway folder too.
      WEBVIEW2_USER_DATA_FOLDER: path.join(dataDir, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    },
    stdio: "ignore",
  });
  let up = false;
  for (let i = 0; i < 120 && !up; i++) {
    try {
      up = (await fetch(`http://127.0.0.1:${PORT}/json/version`)).ok;
    } catch {
      /* not up yet */
    }
    if (!up) await sleep(500);
  }
  if (!up) {
    // Say why: the app exited (with any crash report it saved), or its window never started.
    const crashes = path.join(dataDir, "crashes");
    const reports = existsSync(crashes) ? readdirSync(crashes).map((f) => readFileSync(path.join(crashes, f), "utf8")) : [];
    // What's in the data folder (did WebView2 start at all?), the WebView2 processes and their
    // arguments (was the debug port passed?), and the app's windows.
    const ps = (script: string) => {
      try {
        return execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8", timeout: 30_000 }).trim();
      } catch (e) {
        return `(failed: ${e})`;
      }
    };
    const files = ps(`Get-ChildItem -Recurse -Depth 3 '${dataDir}' | Select-Object -First 40 -ExpandProperty FullName`);
    const webviews = ps(`Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match 'webview-exe-name=framewise' -and $_.CommandLine -notmatch '--type=' } | ForEach-Object { $_.ProcessId.ToString() + ' ' + $_.CommandLine }`);
    const windows = ps(`Get-Process framewise -ErrorAction SilentlyContinue | ForEach-Object { $_.Id.ToString() + ' session ' + $_.SessionId + ' window [' + $_.MainWindowTitle + ']' }`);
    const ports = ps(`Get-NetTCPConnection -State Listen -OwningProcess (Get-Process msedgewebview2 -ErrorAction SilentlyContinue).Id -ErrorAction SilentlyContinue | ForEach-Object { $_.LocalAddress + ':' + $_.LocalPort }`);
    throw new Error(
      `FrameWise's window didn't open its debug port within 60 s. App ${app.exitCode === null ? "still running" : `exited with code ${app.exitCode}`}.` +
        (reports.length ? ` Crash reports: ${reports.join(" | ")}` : " No crash report.") +
        " (Running as admin? WebView2 ignores WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS then: use scripts/admin-e2e.ps1.)" +
        `\nApp: ${windows || "(none)"}\nWebView2 browser processes:\n${webviews || "(none)"}\nWebView2 listening on: ${ports || "(nothing)"}\nData folder:\n${files || "(empty)"}`,
    );
  }
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
  const ctx = browser.contexts()[0];
  page = ctx.pages()[0] ?? (await ctx.waitForEvent("page"));
  // The window starts on a blank page; wait for FrameWise itself.
  await page.waitForFunction(() => "__TAURI_INTERNALS__" in window && document.querySelector("#root")?.childElementCount, null, { timeout: 30_000 });
  // Tests run without a restore point, so no fix needs a permission prompt.
  await page.evaluate(() => localStorage.setItem("framewise.restorePoint", "0"));
  await page.reload();
});

test.afterAll(async () => {
  await browser?.close().catch(() => {});
  app?.kill();
  await sleep(1500);
  // WebView2's helper processes can hold a file or two for a moment after the app exits.
  try {
    rmSync(dataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
  } catch (e) {
    console.warn(`Couldn't remove ${dataDir}: ${e}`);
  }
});

test("first run: welcome screen, then a scan with a health score", async () => {
  await expect(page.getByText("The honest gaming optimiser").first()).toBeVisible();
  await page.getByRole("button", { name: "Run my first scan" }).click();
  await expect(page.getByText("out of 100")).toBeVisible({ timeout: 90_000 });
  await page.getByRole("button", { name: /Quick wins/ }).first().click();
  await expect(page.getByRole("tab", { name: /All checks \(16\)/ }).or(page.getByRole("button", { name: /All checks \(16\)/ }))).toBeVisible();
});

test("a safe fix applies, shows in History, and Undo puts it back", async () => {
  type Finding = { check_id: string; title: string; status: string; fix: { safe: boolean; label: string } | null };
  // Works on its own too: scan first if nothing has been scanned yet.
  let scan = await invoke<{ findings: Finding[] } | null>("last_scan");
  if (!scan) {
    scan = await invoke<{ findings: Finding[] }>("run_scan");
    await page.reload();
    const skip = page.getByRole("button", { name: "Skip for now" });
    if (await skip.isVisible().catch(() => false)) await skip.click();
  }
  const f = scan.findings.find((x) => x.status === "issue" && x.fix?.safe);
  test.skip(!f, "This PC doesn't need any safe fix, so there's nothing to apply.");
  await page.getByRole("button", { name: /Quick wins/ }).first().click();
  await page.getByRole("button", { name: f!.title }).first().click();
  await page.getByRole("button", { name: f!.fix!.label }).first().click();
  await expect(page.getByText("1 change made")).toBeVisible({ timeout: 60_000 });

  await page.getByRole("button", { name: /History/ }).first().click();
  const row = page.locator(".change-row, li, tr").filter({ hasText: f!.fix!.label === "Turn on" ? /Game Mode|windowed|scheduling/i : /./ }).first();
  await expect(row).toBeVisible();
  await page.getByRole("button", { name: /^Undo$/ }).first().click();
  await expect(page.getByText("1 change undone")).toBeVisible({ timeout: 60_000 });
});

test("the standby list clears when asked", async () => {
  // Clearing goes through the helper, which needs a UAC prompt unless the tests already run as admin.
  let admin = true;
  try {
    execFileSync("net", ["session"], { stdio: "ignore" });
  } catch {
    admin = false;
  }
  test.skip(!admin, "Clearing the standby list needs admin rights (run the tests from an admin terminal).");
  const r = await invoke<{ before: number | null; after: number | null }>("standby_purge");
  expect(r.before, "standby size before").not.toBeNull();
  expect(r.after, "standby size after").not.toBeNull();
  expect(r.after!).toBeLessThan(r.before!);
  console.log(`Standby list: ${(r.before! / 2 ** 20).toFixed(0)} MB before, ${(r.after! / 2 ** 20).toFixed(0)} MB after.`);
});

test("a benchmark records the test scene", async () => {
  const status = await invoke<{ direct_ok: boolean }>("monitor_status");
  test.skip(!status.direct_ok, "Frame capture needs admin rights or the Performance Log Users group here.");
  test.skip(!existsSync(SCENE), `${SCENE} not built`);
  // PresentMon reads frames from the graphics driver's events; virtual machines with only a basic
  // or virtual display adapter (GitHub's runners) don't produce them for any app.
  const adapters = execFileSync("powershell", ["-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController).Name -join '; '"], { encoding: "utf8" }).trim();
  const realGpu = adapters.split("; ").some((n) => n && !/Basic Display|Basic Render|Hyper-V|Remote Display|VirtualBox|VMware|Parsec|Virtual Display/i.test(n));
  test.skip(!realGpu, `No real graphics adapter here (${adapters || "none"}), so there are no frames to capture.`);
  const scene = spawn(SCENE, ["90", "2000"], { stdio: "ignore" });
  try {
    await page.getByRole("button", { name: /Monitor & benchmark/ }).first().click();
    await page.getByRole("button", { name: "Start monitor" }).click();
    await expect(page.getByText(/Measuring test_scene\.exe|test_scene\.exe/).first(), `the test scene ${scene.exitCode === null ? "is running" : `exited with code ${scene.exitCode}`}; adapters: ${adapters}`).toBeVisible({ timeout: 30_000 });
    const form = page.locator(".bench-form");
    await form.locator("select").nth(0).selectOption("30");
    await form.locator("select").nth(1).selectOption("0");
    await form.locator("select").nth(2).selectOption({ label: "test_scene.exe" });
    await form.locator("input").fill("E2E run");
    await form.getByRole("button", { name: "Record" }).click();
    await expect(page.getByText(/Saved: test_scene\.exe/)).toBeVisible({ timeout: 90_000 });
    await expect(page.locator(".runs-table")).toContainText("E2E run");
    await page.getByRole("button", { name: "Stop monitor" }).click();
  } finally {
    scene.kill();
  }
});

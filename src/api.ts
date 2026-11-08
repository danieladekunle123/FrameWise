import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppInfo,
  PurgeResult,
  StandbyStatus,
  CrashReport,
  IssueDraft,
  Game,
  GamesList,
  Profile,
  ProfileActions,
  RunningApp,
  SessionView,
  WatcherInfo,
  BenchmarkDetail,
  BenchmarkRequest,
  BenchmarkRow,
  CompareResult,
  MonitorStatus,
  OverlayCorner,
  ChangeLog,
  CleanCategoryId,
  CleanupCategory,
  CleanupOutcome,
  FixOutcome,
  FootprintReport,
  LiveStats,
  Myth,
  RecheckResult,
  ScanResult,
  ScoreHistoryPoint,
  StartupEntry,
  StartupPick,
  UndoOutcome,
} from "./types";
import * as mock from "./mock";

/** True inside the Tauri window; false in a plain browser (UI development with mock data). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return mock.handle<T>(cmd, args);
  return invoke<T>(cmd, args);
}

export interface FixOptions {
  restore_point: boolean;
  input?: { startup?: StartupPick[]; enable?: boolean };
}

export const api = {
  runScan: () => call<ScanResult>("run_scan"),
  lastScan: () => call<ScanResult | null>("last_scan"),
  recheck: (checkId: string) => call<RecheckResult>("recheck", { checkId }),
  scoreHistory: () => call<ScoreHistoryPoint[]>("score_history"),
  footprint: () => call<FootprintReport | null>("footprint"),
  liveStats: () => call<LiveStats>("live_stats"),
  myths: () => call<Myth[]>("myths"),
  appInfo: () => call<AppInfo>("app_info"),
  prepareUpdate: () => call<void>("prepare_update"),
  crashReports: () => call<CrashReport[]>("crash_reports"),
  standbyStatus: () => call<StandbyStatus>("standby_status"),
  standbyPurge: () => call<PurgeResult>("standby_purge"),
  crashDismiss: (id: string) => call<void>("crash_dismiss", { id }),
  crashRecord: (message: string) => call<void>("crash_record", { message }),
  crashIssue: (id: string) => call<IssueDraft>("crash_issue", { id }),
  watcherBudgetIssue: (ramMb: number, peakRamMb: number, cpuPct: number, profiles: number, active: boolean) =>
    call<IssueDraft>("watcher_budget_issue", { ramMb, peakRamMb, cpuPct, profiles, active }),
  systemAccent: () => call<string | null>("system_accent"),
  openExternal: (uri: string) => call<void>("open_external", { uri }),
  // Phase 2
  applyFix: (checkId: string, options: FixOptions) => call<FixOutcome>("apply_fix", { checkId, options }),
  fixAllSafe: (options: FixOptions) => call<FixOutcome>("fix_all_safe", { options }),
  undoChanges: (ids: number[], force = false) => call<UndoOutcome>("undo_changes", { ids, force }),
  undoAll: (force = false) => call<UndoOutcome>("undo_all", { force }),
  confirmChanges: (ids: number[]) => call<void>("confirm_changes", { ids }),
  changeLog: () => call<ChangeLog>("change_log"),
  startupEntries: () => call<StartupEntry[]>("startup_entries"),
  cleanupScan: (elevated: boolean) => call<CleanupCategory[]>("cleanup_scan", { elevated }),
  cleanupRun: (categories: CleanCategoryId[], permanent: boolean) => call<CleanupOutcome>("cleanup_run", { categories, permanent }),
  exportReport: () => call<string>("export_report"),
  // Phase 3
  monitorStatus: () => call<MonitorStatus>("monitor_status"),
  monitorStart: () => call<MonitorStatus>("monitor_start"),
  monitorStop: () => call<void>("monitor_stop"),
  monitorPin: (pid: number | null) => call<void>("monitor_pin", { pid }),
  monitorAllowWithoutPrompt: () => call<void>("monitor_allow_without_prompt"),
  overlaySet: (visible: boolean, corner?: OverlayCorner) => call<void>("overlay_set", { visible, corner }),
  benchmarkStart: (request: BenchmarkRequest) => call<void>("benchmark_start", { request }),
  benchmarkCancel: () => call<void>("benchmark_cancel"),
  benchmarksList: () => call<BenchmarkRow[]>("benchmarks_list"),
  benchmarkDetail: (id: number) => call<BenchmarkDetail>("benchmark_detail", { id }),
  benchmarkRename: (id: number, label: string) => call<void>("benchmark_rename", { id, label }),
  benchmarkDelete: (id: number) => call<void>("benchmark_delete", { id }),
  benchmarkCompare: (before: number[], after: number[]) => call<CompareResult>("benchmark_compare", { before, after }),
  // Phase 4
  gamesList: () => call<GamesList>("games_list"),
  gameAdd: (path: string) => call<Game>("game_add", { path }),
  gameRemove: (id: string) => call<void>("game_remove", { id }),
  profileSave: (game: Game, enabled: boolean, actions: ProfileActions) => call<Profile>("profile_save", { game, enabled, actions }),
  runningApps: () => call<RunningApp[]>("running_apps"),
  watcherStatus: () => call<WatcherInfo>("watcher_status"),
  watcherEnable: (on: boolean) => call<WatcherInfo>("watcher_enable", { on }),
  watcherPause: (paused: boolean) => call<void>("watcher_pause", { paused }),
  profileSessions: (limit = 20) => call<SessionView[]>("profile_sessions", { limit }),
};

/** Subscribes to a backend event; returns an unsubscribe function. */
export function onEvent<T>(event: string, cb: (payload: T) => void): () => void {
  if (!inTauri) return mock.onEvent(event, cb as (p: unknown) => void);
  let stop: (() => void) | null = null;
  let cancelled = false;
  listen<T>(event, (e) => cb(e.payload)).then((un) => {
    if (cancelled) un();
    else stop = un;
  });
  return () => {
    cancelled = true;
    stop?.();
  };
}

export function errorCode(e: unknown): string | null {
  return typeof e === "object" && e !== null && "code" in e ? String((e as { code: unknown }).code) : null;
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (typeof e === "object" && e !== null && "message" in e) return String((e as { message: unknown }).message);
  return "Something went wrong.";
}

export function formatBytes(b: number): string {
  const mb = b / 1048576;
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  if (mb >= 10) return `${mb.toFixed(0)} MB`;
  if (mb >= 0.1) return `${mb.toFixed(1)} MB`;
  return b > 0 ? "< 0.1 MB" : "0 MB";
}

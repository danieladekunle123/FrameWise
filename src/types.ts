// Mirrors the Rust types in crates/framewise-checks/src/model.rs and src-tauri/src/*.rs.

export type Impact = "high" | "medium" | "low_medium" | "low";
export type Status = "issue" | "ok" | "unknown" | "not_applicable";
export type Action = "fix" | "guide" | "choose";

export interface Link {
  label: string;
  url: string;
}

export interface Choice {
  label: string;
  gains: string;
  costs: string;
}

export interface Guide {
  steps: string[];
  settings_uri: string | null;
  links: Link[];
  choices: Choice[];
}

export interface Finding {
  check_id: string;
  title: string;
  status: Status;
  impact: Impact;
  action: Action;
  summary: string;
  explanation: string;
  fix_changes: string;
  detected: string | null;
  expected: string | null;
  details: string[];
  guide: Guide;
  one_click_phase: number | null;
  fix: FixInfo | null;
}

export type FixKind = "apply" | "pick_startup" | "choose" | "cleanup";

export interface FixInfo {
  kind: FixKind;
  label: string;
  safe: boolean;
  needs_admin: boolean;
  needs_restart: boolean;
  confirm: string | null;
}

export interface ScanResult {
  id: number;
  started_at: number;
  duration_ms: number;
  score: number;
  app_version: string;
  findings: Finding[];
  collection_errors: string[];
}

export interface RecheckResult {
  finding: Finding;
  score: number;
}

export interface ScoreHistoryPoint {
  started_at: number;
  score: number;
}

export interface ProcessFootprint {
  role: string;
  name: string;
  count: number;
  ram_mb: number;
  cpu_pct: number;
}

export interface FootprintReport {
  ram_mb: number;
  cpu_pct: number;
  processes: ProcessFootprint[];
  history: { t: number; ram_mb: number; cpu_pct: number }[];
  budget_ram_mb: number;
  budget_cpu_pct: number;
  over_budget: boolean;
}

export interface LiveStats {
  cpu_pct: number;
  ram_used_gb: number;
  ram_total_gb: number;
}

export interface Myth {
  id: string;
  claim: string;
  verdict: string;
  explanation: string;
  links: Link[];
  advanced_exception: boolean;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  db_ok: boolean;
  portable: boolean;
}

// ------------------------------------------------------------------ phase 2: changes, undo, cleanup

/** Errors from change commands carry a machine-readable code. */
export interface ApiError {
  code: string;
  message: string;
}

export interface ChangeRow {
  id: number;
  check_id: string;
  title: string;
  old_label: string | null;
  new_label: string | null;
  applied_at: number;
  source: "manual" | "fix_all" | "cleanup" | string;
  reverted_at: number | null;
  needs_restart: boolean;
  restart_pending: boolean;
  needs_admin: boolean;
  undoable: boolean;
  detail: string | null;
}

export interface RestorePointRow {
  id: number;
  created_at: number;
  sequence: number | null;
  description: string;
  reused: boolean;
}

export interface ChangeLog {
  changes: ChangeRow[];
  restore_points: RestorePointRow[];
  restore_point_this_session: boolean;
}

export interface FixOutcome {
  changes: ChangeRow[];
  already_set: string[];
  errors: { check_id: string; code: string; message: string }[];
  restore_point: RestorePointRow | null;
  findings: Finding[];
  score: number | null;
  confirm_ids: number[];
  confirm_seconds: number;
}

export interface UndoOutcome {
  reverted: number[];
  errors: [number, ApiError][];
  findings: Finding[];
  score: number | null;
}

export type StartupSource = "user_run" | "machine_run" | "machine_run32" | "user_folder" | "machine_folder";

export interface StartupEntry {
  name: string;
  location: string;
  enabled: boolean;
  source: StartupSource | null;
  value_name: string;
  command: string;
}

export interface StartupPick {
  source: StartupSource;
  value_name: string;
}

export type CleanCategoryId =
  | "user_temp"
  | "windows_temp"
  | "crash_dumps"
  | "system_crash_dumps"
  | "update_downloads"
  | "driver_packages"
  | "shader_cache";

export interface DriverPackage {
  inf: string;
  provider: string;
  version: string;
  date: string;
  bytes: number;
  in_use: boolean;
  kept_for_rollback: boolean;
}

export interface CleanupCategory {
  id: CleanCategoryId;
  title: string;
  description: string;
  needs_admin: boolean;
  recyclable: boolean;
  default_selected: boolean;
  scan: {
    bytes: number;
    items: number;
    recent_items: number;
    drivers: DriverPackage[];
    needs_permission: boolean;
    error: string | null;
  };
}

export interface CleanResult {
  id: CleanCategoryId;
  freed_bytes: number;
  removed_items: number;
  skipped_items: number;
  recycled: boolean;
  error: string | null;
}

export interface CleanupOutcome {
  results: CleanResult[];
  freed_bytes: number;
  recycled_bytes: number;
}

// ------------------------------------------------------------------ phase 3: monitor and benchmark

export interface SensorSample {
  cpu_pct: number | null;
  cpu_perf_pct: number | null;
  cpu_cap_pct: number | null;
  gpu_pct: number | null;
  gpu_temp_c: number | null;
  gpu_clock_mhz: number | null;
  gpu_power_w: number | null;
  gpu_thermal_throttle: boolean | null;
  gpu_power_limited: boolean | null;
  ram_used_bytes: number | null;
  ram_total_bytes: number | null;
  vram_used_bytes: number | null;
  vram_total_bytes: number | null;
}

export type ThrottleKind = "gpu_thermal" | "cpu";

export interface MonitorTick {
  app: string | null;
  pid: number | null;
  fps: number | null;
  low_1: number | null;
  frame_ms: number | null;
  frames: number[];
  sensors: SensorSample;
  throttling: ThrottleKind[];
  /** [exe, pid, frames in the last second] */
  presenting: [string, number, number][];
}

export interface MonitorStatus {
  running: boolean;
  starting: boolean;
  mode: "direct" | "helper" | null;
  direct_ok: boolean;
  benchmarking: boolean;
  pinned: number | null;
  overlay: boolean;
  nvml: boolean;
  hotkey: string;
}

export interface FrameStats {
  frames: number;
  duration_s: number;
  avg_fps: number;
  low_1: number;
  low_01: number;
  avg_ms: number;
  p50_ms: number;
  p99_ms: number;
  p999_ms: number;
  max_ms: number;
  stutters: number;
}

export interface BenchmarkRow {
  id: number;
  app: string;
  label: string;
  recorded_at: number;
  duration_s: number;
  frames: number;
  avg_fps: number;
  low_1: number;
  low_01: number;
  stats: FrameStats;
  has_frames: boolean;
}

export interface BenchmarkRequest {
  pid: number | null;
  duration_s: number;
  delay_s: number;
  label: string;
}

export interface BenchmarkProgress {
  phase: "countdown" | "recording" | "saving";
  seconds_left: number;
  app: string | null;
  fps: number | null;
}

export interface BenchmarkDetail {
  run: BenchmarkRow;
  graph: number[];
}

export type Verdict = "better" | "worse" | "within_variation" | "need_more_runs";

export interface MetricComparison {
  metric: "avg_fps" | "low_1" | "low_01";
  before: number;
  after: number;
  diff: number;
  diff_pct: number;
  variation: number;
  verdict: Verdict;
}

export interface CompareResult {
  comparison: { before_runs: number; after_runs: number; metrics: MetricComparison[]; verdict: Verdict };
  before_spread_pct: number | null;
  after_spread_pct: number | null;
}

export type OverlayCorner = "top_left" | "top_right" | "bottom_left" | "bottom_right";

// ------------------------------------------------------------------ phase 4: game profiles

export type GameSource = "steam" | "epic" | "battle_net" | "xbox" | "roblox" | "manual";

export interface Game {
  id: string;
  name: string;
  source: GameSource;
  install_dir: string | null;
  exe: string | null;
}

export interface ProfileActions {
  power_plan: string | null;
  pause: string[];
  close: string[];
}

export interface Profile {
  game_id: string;
  enabled: boolean;
  actions: ProfileActions;
}

export interface GameRow {
  game: Game;
  source_label: string;
  profile: Profile | null;
}

export interface GamesList {
  games: GameRow[];
  /** [GUID, name] */
  plans: [string, string][];
  current_plan: string | null;
}

export interface RunningApp {
  exe: string;
  path: string | null;
  label: string | null;
  known_overlay: boolean;
}

export interface WatcherStatus {
  pid: number;
  started_at: number;
  updated_at: number;
  ram_mb: number;
  cpu_pct: number;
  peak_ram_mb: number;
  paused: boolean;
  active_game: string | null;
  profiles: number;
  over_budget: boolean;
}

export interface WatcherInfo {
  enabled: boolean;
  running: boolean;
  autostart: boolean;
  paused: boolean;
  status: WatcherStatus | null;
  budget_ram_mb: number;
  budget_cpu_pct: number;
}

export interface SessionView {
  id: number;
  game_id: string;
  game_name: string;
  started_at: number;
  ended_at: number | null;
  restored: boolean;
  power_plan: string | null;
  paused: string[];
  closed: string[];
  not_closed: string[];
}

// ------------------------------------------------------------------ crash reports

export interface CrashReport {
  id: string;
  at: number;
  version: string;
  process: "app" | "watcher" | "helper" | "interface" | string;
  message: string;
  location: string | null;
}

export interface IssueDraft {
  title: string;
  body: string;
  url: string;
}

export interface StandbyStatus {
  standby_bytes: number | null;
  ram_total_bytes: number;
  may_help: boolean;
}

export interface PurgeResult {
  before: number | null;
  after: number | null;
}

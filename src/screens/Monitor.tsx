import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  BarChart3,
  Circle,
  Cpu,
  Gauge,
  Keyboard,
  Layers,
  MemoryStick,
  MonitorPlay,
  Pencil,
  Play,
  Shield,
  Square,
  Thermometer,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react";
import { api, errorText, formatBytes, onEvent } from "../api";
import { Badge, Switch, timeAgo } from "../components/ui";
import { useFixes } from "../fixes";
import type { BenchmarkDetail, BenchmarkProgress, BenchmarkRow, CompareResult, MetricComparison, MonitorStatus, MonitorTick, OverlayCorner, Verdict } from "../types";

const GRAPH_FRAMES = 1500;
const fmt = (v: number | null | undefined, digits = 0) => (v == null || !isFinite(v) ? "–" : v.toFixed(digits));
const pct = (v: number | null | undefined) => (v == null ? "–" : `${Math.round(v)}%`);

export function MonitorScreen() {
  const fixes = useFixes();
  const [status, setStatus] = useState<MonitorStatus | null>(null);
  const [tick, setTick] = useState<MonitorTick | null>(null);
  const [frames, setFrames] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [corner, setCorner] = useState<OverlayCorner>("top_left");
  const [allowed, setAllowed] = useState(false);

  const refresh = useCallback(() => api.monitorStatus().then(setStatus).catch(() => {}), []);

  useEffect(() => {
    refresh();
    const offs = [
      onEvent<MonitorTick>("monitor-tick", (t) => {
        setTick(t);
        if (t.frames.length) setFrames((f) => [...f, ...t.frames].slice(-GRAPH_FRAMES));
      }),
      onEvent<string | null>("monitor-stopped", (reason) => {
        setTick(null);
        setFrames([]);
        if (reason) setError(`The monitor stopped: ${reason}`);
        refresh();
      }),
    ];
    return () => offs.forEach((o) => o());
  }, [refresh]);

  const start = async () => {
    setError(null);
    try {
      const s = await fixes.runTask(() => api.monitorStart());
      if (s) setStatus(s);
    } catch (e) {
      setError(errorText(e));
    }
  };
  const stop = async () => {
    await api.monitorStop();
    setTimeout(refresh, 600);
  };
  const setOverlay = async (on: boolean, c = corner) => {
    try {
      await api.overlaySet(on, c);
      refresh();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const allow = async () => {
    setError(null);
    try {
      await fixes.runTask(() => api.monitorAllowWithoutPrompt());
      setAllowed(true);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const running = status?.running ?? false;
  const s = tick?.sensors;

  return (
    <div className="page">
      <div className="card card-pad upcoming-hero">
        <span className="icon">
          <Activity size={24} />
        </span>
        <div className="grow" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <h2 style={{ fontSize: 18 }}>Monitor and benchmark</h2>
          <p className="muted">
            Measure what changes actually do. Frame times come from Intel's PresentMon, which runs only while the monitor
            is on. Benchmarks record a repeatable scene so you can compare before and after a fix.
          </p>
          <div className="head-actions" style={{ marginTop: 4 }}>
            {running ? (
              <button className="btn" onClick={stop} disabled={status?.benchmarking}>
                <Square size={14} /> Stop monitor
              </button>
            ) : (
              <button className="btn btn-primary" onClick={start} disabled={fixes.busy || status?.starting}>
                {!status?.direct_ok && <Shield size={14} />}
                <Play size={14} /> Start monitor
              </button>
            )}
            <span className="hint">
              {running
                ? status?.mode === "helper"
                  ? "Running through the FrameWise helper (Windows asked for permission once)."
                  : "Running."
                : status?.direct_ok
                  ? "Off. Starts without a permission prompt."
                  : "Off. Windows asks for permission each time it starts."}
            </span>
          </div>
          {status && !status.direct_ok && !allowed && (
            <p className="hint">
              Frame timing reads a Windows trace that needs admin rights.{" "}
              <button className="btn-link" onClick={allow} disabled={fixes.busy}>
                Allow monitoring without a prompt
              </button>{" "}
              adds you to Windows' "Performance Log Users" group (one prompt now; takes effect after you sign out and back in).
            </p>
          )}
          {allowed && <p className="hint">Done. Sign out of Windows and back in, and the monitor will start without a prompt.</p>}
        </div>
      </div>

      {error && (
        <div className="banner error-box" role="alert">
          <TriangleAlert size={18} />
          <div className="grow">{error}</div>
          <button className="btn btn-ghost btn-sm" onClick={() => setError(null)} aria-label="Dismiss">
            <X size={14} />
          </button>
        </div>
      )}

      {running && (
        <section className="card card-pad live-section" aria-label="Live monitor">
          <div className="section-head">
            <h2 className="section-title">
              <MonitorPlay size={16} /> {tick?.app ? `Measuring ${tick.app}` : "Waiting for a game to draw frames…"}
            </h2>
            <div className="head-actions">
              <label className="hint" htmlFor="pin">
                App
              </label>
              <select
                id="pin"
                className="select"
                value={status?.pinned ?? ""}
                onChange={(e) => {
                  const pid = e.target.value ? Number(e.target.value) : null;
                  api.monitorPin(pid).then(refresh);
                }}
              >
                <option value="">The app in front</option>
                {(tick?.presenting ?? []).map(([app, pid, n]) => (
                  <option key={pid} value={pid}>
                    {app} · {n} FPS
                  </option>
                ))}
              </select>
            </div>
          </div>

          {(tick?.throttling ?? []).length > 0 && (
            <div className="banner" style={{ ["--c" as string]: "var(--warning)" }} role="status">
              <Thermometer size={18} />
              <div className="grow">
                {tick!.throttling.includes("gpu_thermal") && <p><strong>The GPU is slowing itself down because it's too hot.</strong> Check its fans and the case airflow.</p>}
                {tick!.throttling.includes("cpu") && (
                  <p>
                    <strong>The CPU is running well below its normal speed under load.</strong> That usually means it's too hot or held by a power limit.
                  </p>
                )}
              </div>
            </div>
          )}

          <div className="tiles live-tiles">
            <Tile label="FPS" value={fmt(tick?.fps)} sub={`1% low ${fmt(tick?.low_1)}`} live icon={Gauge} />
            <Tile label="Frame time" value={`${fmt(tick?.frame_ms, 1)} ms`} sub={`${frames.length ? fmt(Math.max(...frames.slice(-300)), 1) : "–"} ms slowest, last 2 s`} icon={Activity} />
            <Tile
              label="GPU"
              value={pct(s?.gpu_pct)}
              sub={[s?.gpu_temp_c != null && `${fmt(s.gpu_temp_c)} °C`, s?.gpu_clock_mhz != null && `${s.gpu_clock_mhz} MHz`, s?.gpu_power_w != null && `${fmt(s.gpu_power_w)} W`].filter(Boolean).join(" · ") || (status?.nvml ? "–" : "Temperature: NVIDIA GPUs only")}
              icon={Layers}
            />
            <Tile label="CPU" value={pct(s?.cpu_pct)} sub={s?.cpu_perf_pct != null ? `${Math.round(s.cpu_perf_pct)}% of base speed` : "–"} icon={Cpu} />
            <Tile label="RAM" value={s?.ram_used_bytes != null ? formatBytes(s.ram_used_bytes) : "–"} sub={s?.ram_total_bytes ? `of ${formatBytes(s.ram_total_bytes)}` : ""} icon={MemoryStick} />
            <Tile label="VRAM" value={s?.vram_used_bytes != null ? formatBytes(s.vram_used_bytes) : "–"} sub={s?.vram_total_bytes ? `of ${formatBytes(s.vram_total_bytes)}` : "Dedicated GPU memory"} icon={MemoryStick} />
          </div>

          <div>
            <div className="hint" style={{ marginBottom: 4 }}>
              Frame time, last {Math.round(frames.reduce((a, b) => a + b, 0) / 1000)} s. Lower is better, spikes are stutters; dashed lines mark 144 and 60 FPS.
            </div>
            <FrameGraph values={frames} height={120} label="Live frame-time graph" />
          </div>

          <div className="overlay-row">
            <Switch checked={status?.overlay ?? false} onChange={(v) => setOverlay(v)} label="In-game overlay" />
            <span className="grow">
              <strong>In-game overlay</strong>
              <span className="hint"> · FPS and temperatures on top of games in borderless or windowed mode. Exclusive full-screen games draw over it.</span>
            </span>
            <select
              className="select"
              value={corner}
              aria-label="Overlay position"
              onChange={(e) => {
                const c = e.target.value as OverlayCorner;
                setCorner(c);
                if (status?.overlay) setOverlay(true, c);
              }}
            >
              <option value="top_left">Top left</option>
              <option value="top_right">Top right</option>
              <option value="bottom_left">Bottom left</option>
              <option value="bottom_right">Bottom right</option>
            </select>
          </div>
        </section>
      )}

      <Benchmarks status={status} onStatus={refresh} presenting={tick?.presenting ?? []} />
    </div>
  );
}

function Tile({ label, value, sub, live, icon: Icon }: { label: string; value: string; sub?: string; live?: boolean; icon: typeof Activity }) {
  return (
    <div className="card tile">
      <span className="label">
        <Icon size={14} /> {label}
      </span>
      <span className={`value num ${live ? "live" : ""}`}>{value}</span>
      {sub !== undefined && <span className="sub">{sub}</span>}
    </div>
  );
}

/** Frame times in ms; the y axis starts at 0 and gridlines mark 60 and 144 FPS when in range. */
export function FrameGraph({ values, height = 120, label, compare }: { values: number[]; height?: number; label: string; compare?: number[] }) {
  const width = 600;
  const all = compare ? [...values, ...compare] : values;
  if (values.length < 2) {
    return (
      <div className="frame-graph empty-graph hint" style={{ height }}>
        No frames yet
      </div>
    );
  }
  const sorted = [...all].sort((a, b) => a - b);
  const p99 = sorted[Math.floor(sorted.length * 0.99)] ?? 16;
  const top = Math.max(p99 * 1.6, Math.min(sorted[sorted.length - 1] * 1.05, p99 * 4), 8.4);
  const y = (v: number) => height - 2 - (Math.min(v, top) / top) * (height - 6);
  const path = (v: number[]) => {
    const step = width / Math.max(v.length - 1, 1);
    return v.map((ms, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${y(ms).toFixed(1)}`).join(" ");
  };
  const lines = [
    { ms: 1000 / 60, text: "60 FPS" },
    { ms: 1000 / 144, text: "144 FPS" },
  ].filter((l) => l.ms < top);
  return (
    <svg className="frame-graph" width="100%" height={height} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label={label}>
      {lines.map((l) => (
        <g key={l.text}>
          <line x1="0" x2={width} y1={y(l.ms)} y2={y(l.ms)} stroke="var(--text-2)" strokeDasharray="4 4" strokeWidth="1" opacity="0.5" vectorEffect="non-scaling-stroke" />
        </g>
      ))}
      {compare && compare.length > 1 && <path d={path(compare)} fill="none" stroke="var(--text-2)" strokeWidth="1.5" opacity="0.7" vectorEffect="non-scaling-stroke" />}
      <path d={path(values)} fill="none" stroke="var(--live)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
    </svg>
  );
}

// ------------------------------------------------------------------ benchmarks

function Benchmarks({ status, onStatus, presenting }: { status: MonitorStatus | null; onStatus: () => void; presenting: [string, number, number][] }) {
  const fixes = useFixes();
  const [runs, setRuns] = useState<BenchmarkRow[]>([]);
  const [label, setLabel] = useState("");
  const [duration, setDuration] = useState(60);
  const [delay, setDelay] = useState(5);
  const [pid, setPid] = useState<number | null>(null);
  const [progress, setProgress] = useState<BenchmarkProgress | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [before, setBefore] = useState<Set<number>>(new Set());
  const [after, setAfter] = useState<Set<number>>(new Set());
  const [open, setOpen] = useState<BenchmarkDetail | null>(null);

  const load = useCallback(() => api.benchmarksList().then(setRuns).catch(() => setRuns([])), []);
  useEffect(() => {
    load();
    const offs = [
      onEvent<BenchmarkProgress>("benchmark-progress", setProgress),
      onEvent<BenchmarkRow>("benchmark-done", (run) => {
        setProgress(null);
        setMessage({ ok: true, text: `Saved: ${run.app}, ${run.avg_fps.toFixed(1)} FPS average, 1% low ${run.low_1.toFixed(1)}.` });
        load();
        onStatus();
      }),
      onEvent<string>("benchmark-failed", (text) => {
        setProgress(null);
        setMessage({ ok: false, text });
        onStatus();
      }),
    ];
    return () => offs.forEach((o) => o());
  }, [load, onStatus]);

  const record = async () => {
    setMessage(null);
    try {
      await fixes.runTask(() => api.benchmarkStart({ pid, duration_s: duration, delay_s: delay, label: label.trim() }));
      setProgress({ phase: delay > 0 ? "countdown" : "recording", seconds_left: delay || duration, app: null, fps: null });
      onStatus();
    } catch (e) {
      setMessage({ ok: false, text: errorText(e) });
    }
  };

  // Functional updates, so quick clicks in a row never work from a stale selection.
  const toggle = (side: "before" | "after", id: number) => {
    const [setSide, setOther] = side === "before" ? [setBefore, setAfter] : [setAfter, setBefore];
    setSide((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
    // A run is on one side at most.
    setOther((s) => {
      if (!s.has(id)) return s;
      const n = new Set(s);
      n.delete(id);
      return n;
    });
  };

  const recording = progress !== null || (status?.benchmarking ?? false);

  return (
    <>
      <section className="card card-pad" aria-label="Record a benchmark">
        <div className="section-head">
          <h2 className="section-title">
            <BarChart3 size={16} /> Record a benchmark
          </h2>
        </div>
        <p className="hint" style={{ marginBottom: 12 }}>
          Play the same scene each time (a built-in benchmark, or the same route through the same area), with nothing else running. Record at
          least two runs before a change and two after: a difference only counts if it's bigger than the variation between your runs.
        </p>
        {!recording ? (
          <div className="bench-form">
            <label>
              <span className="hint">Label</span>
              <input className="input" value={label} maxLength={80} placeholder="e.g. Before: Game Mode off" onChange={(e) => setLabel(e.target.value)} />
            </label>
            <label>
              <span className="hint">Length</span>
              <select className="select" value={duration} onChange={(e) => setDuration(Number(e.target.value))}>
                <option value={30}>30 s</option>
                <option value={60}>60 s</option>
                <option value={120}>2 min</option>
              </select>
            </label>
            <label>
              <span className="hint">Start after</span>
              <select className="select" value={delay} onChange={(e) => setDelay(Number(e.target.value))}>
                <option value={0}>Now</option>
                <option value={5}>5 s</option>
                <option value={10}>10 s</option>
                <option value={20}>20 s</option>
              </select>
            </label>
            <label>
              <span className="hint">App</span>
              <select className="select" value={pid ?? ""} onChange={(e) => setPid(e.target.value ? Number(e.target.value) : null)}>
                <option value="">The app in front</option>
                {presenting.map(([app, p]) => (
                  <option key={p} value={p}>
                    {app}
                  </option>
                ))}
              </select>
            </label>
            <button className="btn btn-primary" onClick={record} disabled={fixes.busy}>
              <Circle size={14} fill="currentColor" /> Record
            </button>
          </div>
        ) : (
          <div className="bench-progress" role="status">
            <div className="grow">
              <strong>
                {progress?.phase === "countdown"
                  ? `Switch to your game: recording starts in ${Math.ceil(progress.seconds_left)} s`
                  : progress?.phase === "saving"
                    ? "Saving…"
                    : `Recording ${progress?.app ?? ""}: ${Math.ceil(progress?.seconds_left ?? duration)} s left`}
              </strong>
              {progress?.phase === "recording" && (
                <div className="progress-bar" aria-hidden>
                  <span style={{ width: `${Math.min(100, (1 - (progress.seconds_left ?? 0) / duration) * 100)}%` }} />
                </div>
              )}
              {progress?.fps != null && <span className="hint">{progress.fps.toFixed(1)} FPS so far</span>}
            </div>
            <button className="btn" onClick={() => api.benchmarkCancel()}>
              Cancel
            </button>
          </div>
        )}
        <p className="hint" style={{ marginTop: 10, display: "flex", alignItems: "center", gap: 6 }}>
          <Keyboard size={14} /> While the monitor is running, press <kbd>{status?.hotkey ?? "Ctrl+Shift+F10"}</kbd> in your game to record 60 s of it
          (press again to cancel).
        </p>
        {message && (
          <div className="banner" style={{ ["--c" as string]: message.ok ? "var(--success)" : "var(--warning)", marginTop: 12 }} role="status">
            <div className="grow">{message.text}</div>
            <button className="btn btn-ghost btn-sm" onClick={() => setMessage(null)} aria-label="Dismiss">
              <X size={14} />
            </button>
          </div>
        )}
      </section>

      <section className="card" aria-label="Benchmark runs">
        <div className="card-pad section-head" style={{ paddingBottom: 6 }}>
          <h2 className="section-title">Runs</h2>
          <span className="hint">Mark runs as Before or After to compare them.</span>
        </div>
        {runs.length === 0 ? (
          <div className="empty">
            <BarChart3 size={24} />
            <p className="muted">No benchmarks yet. Record two runs before you change anything.</p>
          </div>
        ) : (
          <table className="fp-table runs-table num">
            <thead>
              <tr>
                <th>Compare</th>
                <th className="left">Run</th>
                <th>Average FPS</th>
                <th>1% low</th>
                <th>0.1% low</th>
                <th>Length</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {runs.map((r) => (
                <RunRow
                  key={r.id}
                  run={r}
                  isBefore={before.has(r.id)}
                  isAfter={after.has(r.id)}
                  onBefore={() => toggle("before", r.id)}
                  onAfter={() => toggle("after", r.id)}
                  onOpen={() => api.benchmarkDetail(r.id).then(setOpen).catch(() => {})}
                  onChanged={load}
                />
              ))}
            </tbody>
          </table>
        )}
        {(before.size > 0 || after.size > 0) && <Comparison before={[...before]} after={[...after]} runs={runs} />}
      </section>

      {open && <RunDetail detail={open} onClose={() => setOpen(null)} />}
    </>
  );
}

function RunRow({
  run,
  isBefore,
  isAfter,
  onBefore,
  onAfter,
  onOpen,
  onChanged,
}: {
  run: BenchmarkRow;
  isBefore: boolean;
  isAfter: boolean;
  onBefore: () => void;
  onAfter: () => void;
  onOpen: () => void;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [label, setLabel] = useState(run.label);
  const save = async () => {
    await api.benchmarkRename(run.id, label);
    setEditing(false);
    onChanged();
  };
  return (
    <tr>
      <td>
        <span className="pair" role="group" aria-label={`Compare ${run.label || run.app}`}>
          <button className={`pill ${isBefore ? "on" : ""}`} aria-pressed={isBefore} onClick={onBefore}>
            Before
          </button>
          <button className={`pill ${isAfter ? "on after" : ""}`} aria-pressed={isAfter} onClick={onAfter}>
            After
          </button>
        </span>
      </td>
      <td className="run-name">
        {editing ? (
          <input
            className="input"
            autoFocus
            value={label}
            maxLength={80}
            onChange={(e) => setLabel(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") save();
              if (e.key === "Escape") setEditing(false);
            }}
            onBlur={save}
            aria-label="Run label"
          />
        ) : (
          <button className="btn-link" onClick={onOpen} title="Show the frame-time graph">
            {run.label || "Untitled run"}
          </button>
        )}
        <span className="hint">
          {run.app} · {timeAgo(run.recorded_at)}
        </span>
      </td>
      <td>{run.avg_fps.toFixed(1)}</td>
      <td>{run.low_1.toFixed(1)}</td>
      <td>{run.low_01.toFixed(1)}</td>
      <td>{Math.round(run.duration_s)} s</td>
      <td className="row-actions">
        <button className="btn btn-ghost btn-sm" onClick={() => setEditing(true)} aria-label="Rename run">
          <Pencil size={14} />
        </button>
        <button
          className="btn btn-ghost btn-sm"
          onClick={async () => {
            await api.benchmarkDelete(run.id);
            onChanged();
          }}
          aria-label="Delete run"
        >
          <Trash2 size={14} />
        </button>
      </td>
    </tr>
  );
}

const METRIC_LABEL: Record<MetricComparison["metric"], string> = { avg_fps: "Average FPS", low_1: "1% low", low_01: "0.1% low" };

function verdictText(v: Verdict, m: MetricComparison): string {
  const d = `${m.diff >= 0 ? "+" : ""}${m.diff.toFixed(1)} FPS (${m.diff_pct >= 0 ? "+" : ""}${m.diff_pct.toFixed(1)}%)`;
  switch (v) {
    case "better":
      return `${d}: bigger than the ${m.variation.toFixed(1)} FPS your runs vary by, so it's a real improvement.`;
    case "worse":
      return `${d}: bigger than the ${m.variation.toFixed(1)} FPS your runs vary by, so it's really worse.`;
    case "within_variation":
      return `${d}: within the ${m.variation.toFixed(1)} FPS your runs vary by, so no measurable difference.`;
    default:
      return `${d}. Record at least two runs on each side to know whether that's more than normal variation.`;
  }
}

function Comparison({ before, after, runs }: { before: number[]; after: number[]; runs: BenchmarkRow[] }) {
  const [result, setResult] = useState<CompareResult | null>(null);
  const key = `${before.join(",")}|${after.join(",")}`;
  useEffect(() => {
    if (before.length === 0 || after.length === 0) {
      setResult(null);
      return;
    }
    api.benchmarkCompare(before, after).then(setResult).catch(() => setResult(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  const apps = useMemo(() => new Set(runs.filter((r) => before.includes(r.id) || after.includes(r.id)).map((r) => r.app)), [runs, before, after]);
  if (before.length === 0 || after.length === 0) {
    return (
      <div className="card-pad compare hint">
        {before.length} before · {after.length} after. Pick at least one run on each side.
      </div>
    );
  }
  if (!result) return null;
  const head = result.comparison.metrics[0];
  const cls = { better: "c-good", worse: "c-critical", within_variation: "neutral", need_more_runs: "neutral" }[result.comparison.verdict];
  return (
    <div className="card-pad compare" aria-label="Comparison">
      <div className="compare-head">
        <Badge cls={cls}>
          {{ better: "Improvement", worse: "Worse", within_variation: "No measurable difference", need_more_runs: "Need more runs" }[result.comparison.verdict]}
        </Badge>
        <strong>{verdictText(result.comparison.verdict, head)}</strong>
      </div>
      {apps.size > 1 && <p className="hint warn-text">These runs are from different apps, so the comparison doesn't mean much.</p>}
      <table className="fp-table num">
        <thead>
          <tr>
            <th>Metric</th>
            <th>Before ({result.comparison.before_runs})</th>
            <th>After ({result.comparison.after_runs})</th>
            <th>Change</th>
            <th>Spread between runs</th>
          </tr>
        </thead>
        <tbody>
          {result.comparison.metrics.map((m) => (
            <tr key={m.metric}>
              <td>{METRIC_LABEL[m.metric]}</td>
              <td>{m.before.toFixed(1)}</td>
              <td>{m.after.toFixed(1)}</td>
              <td className={m.verdict === "better" ? "good-text" : m.verdict === "worse" ? "bad-text" : undefined}>
                {m.diff >= 0 ? "+" : ""}
                {m.diff.toFixed(1)} ({m.diff_pct >= 0 ? "+" : ""}
                {m.diff_pct.toFixed(1)}%)
              </td>
              <td>{m.verdict === "need_more_runs" ? "–" : m.variation.toFixed(1)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="hint">
        {result.before_spread_pct != null && `Before runs vary by ${result.before_spread_pct.toFixed(1)}%. `}
        {result.after_spread_pct != null && `After runs vary by ${result.after_spread_pct.toFixed(1)}%. `}
        Runs within about 3% of each other are a solid baseline.
      </p>
    </div>
  );
}

function RunDetail({ detail, onClose }: { detail: BenchmarkDetail; onClose: () => void }) {
  const r = detail.run;
  const ref = useRef<HTMLDivElement>(null);
  // Braces matter: newer browsers return a Promise from scrollIntoView, which React would
  // otherwise treat as the effect's cleanup function.
  useEffect(() => {
    ref.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [detail]);
  return (
    <section className="card card-pad" ref={ref} aria-label="Run detail">
      <div className="section-head">
        <h2 className="section-title">{r.label || "Untitled run"}</h2>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close run detail">
          <X size={14} />
        </button>
      </div>
      <p className="hint">
        {r.app} · {new Date(r.recorded_at * 1000).toLocaleString()} · {r.frames} frames in {r.duration_s.toFixed(1)} s · {r.stats.stutters} stutters (frames over twice the
        typical time)
      </p>
      <div className="tiles" style={{ margin: "12px 0" }}>
        <Tile label="Average FPS" value={r.avg_fps.toFixed(1)} live icon={Gauge} />
        <Tile label="1% low" value={r.low_1.toFixed(1)} sub="Average of the slowest 1% of frames" icon={Gauge} />
        <Tile label="0.1% low" value={r.low_01.toFixed(1)} sub="Average of the slowest 0.1%" icon={Gauge} />
        <Tile label="Slowest frame" value={`${r.stats.max_ms.toFixed(1)} ms`} sub={`Typical ${r.stats.p50_ms.toFixed(1)} ms`} icon={Activity} />
      </div>
      {detail.graph.length > 1 ? (
        <FrameGraph values={detail.graph} height={160} label="Frame times across the run" />
      ) : (
        <p className="hint">The frame-time file for this run was pruned (FrameWise keeps the newest 50); its statistics are kept.</p>
      )}
    </section>
  );
}

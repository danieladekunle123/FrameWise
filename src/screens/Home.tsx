import { CircuitBoard, Cpu, Gauge, Lightbulb, MemoryStick, Monitor, RefreshCw, ShieldCheck, Wrench, X } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { IssueCard } from "../components/IssueCard";
import { useFixes } from "../fixes";
import { IMPACT_RANK, ScoreRing, scoreWord, timeAgo } from "../components/ui";
import type { Finding, LiveStats, ScanResult } from "../types";
import { usePoll } from "../usePoll";

interface Props {
  scan: ScanResult | null;
  scanning: boolean;
  error: string | null;
  onScan: () => void;
  onOpenCheck: (checkId: string) => void;
  onRecheck: (checkId: string) => Promise<void>;
  restorePoint: boolean;
  onOpenBenchmark?: () => void;
}

export function topIssues(scan: ScanResult | null, n = 3): Finding[] {
  if (!scan) return [];
  return scan.findings
    .filter((f) => f.status === "issue" && f.action !== "choose")
    .sort((a, b) => IMPACT_RANK[a.impact] - IMPACT_RANK[b.impact])
    .slice(0, n);
}

const BASELINE_KEY = "framewise.baselineTipDismissed";

export function Home({ scan, scanning, error, onScan, onOpenCheck, onRecheck, restorePoint, onOpenBenchmark }: Props) {
  const live = usePoll<LiveStats>(api.liveStats, 2000);
  const fixes = useFixes();
  const safe = scan?.findings.filter((f) => f.status === "issue" && f.fix?.safe) ?? [];
  const issues = scan?.findings.filter((f) => f.status === "issue" && f.action !== "choose") ?? [];
  const top = topIssues(scan);
  const [tipDismissed, setTipDismissed] = useState(() => {
    try {
      return localStorage.getItem(BASELINE_KEY) === "1";
    } catch {
      return false;
    }
  });
  const dismissTip = () => {
    setTipDismissed(true);
    try {
      localStorage.setItem(BASELINE_KEY, "1");
    } catch {
      /* ignore */
    }
  };

  const display = scan?.findings.find((f) => f.check_id === "refresh_rate");
  const gpu = scan?.findings.find((f) => f.check_id === "gpu_driver");
  const driverAge = gpu?.detected?.match(/\((\d+) days old\)/)?.[1];

  return (
    <div className="page">
      {error && (
        <div className="banner error-box" role="alert">
          <X size={18} />
          <div className="grow">
            <strong>The scan didn't finish.</strong>
            <p className="hint">{error}</p>
          </div>
        </div>
      )}

      <div className="home-grid">
        <section className="card score-card" aria-label="Health score">
          <ScoreRing score={scan?.score ?? null} />
          <div>
            <h2 style={{ fontSize: 18 }}>{scan ? scoreWord(scan.score) : "Ready to scan"}</h2>
            <p className="muted" style={{ marginTop: 4 }}>
              {scan
                ? issues.length === 0
                  ? "Your PC is already in good shape. Nothing here needs fixing."
                  : `${issues.length} thing${issues.length === 1 ? "" : "s"} could make your games run better.`
                : "A scan only reads settings. It never changes anything."}
            </p>
          </div>
          <div className="score-actions">
            <button className="btn btn-primary" onClick={onScan} disabled={scanning}>
              <RefreshCw size={16} className={scanning ? "spin" : undefined} />
              {scanning ? "Scanning…" : scan ? "Scan again" : "Scan my PC"}
            </button>
            <button
              className="btn"
              disabled={!scan || safe.length === 0 || fixes.busy || scanning}
              onClick={() => scan && fixes.fixAllSafe(scan.findings)}
              title={scan && safe.length === 0 ? "No low-risk fixes left to apply" : undefined}
            >
              <Wrench size={16} /> Fix all safe{safe.length > 0 ? ` (${safe.length})` : ""}
            </button>
            <p className="hint">
              {scan
                ? `Last scan ${timeAgo(scan.started_at)} · took ${(scan.duration_ms / 1000).toFixed(1)} s. `
                : ""}
              {restorePoint ? "Changes start with a restore point, and each one can be undone." : "Each change can be undone from History."}
            </p>
          </div>
        </section>

        <div className="right-col">
          <div className="tiles">
            <div className="card tile">
              <span className="label">
                <Cpu size={14} /> CPU load
              </span>
              <span className="value live num">{live ? `${live.cpu_pct.toFixed(0)}%` : "–"}</span>
              <span className="sub">All cores, live</span>
            </div>
            <div className="card tile">
              <span className="label">
                <MemoryStick size={14} /> RAM in use
              </span>
              <span className="value live num">{live ? `${live.ram_used_gb.toFixed(1)} GB` : "–"}</span>
              <span className="sub">{live ? `of ${live.ram_total_gb.toFixed(0)} GB, live` : "live"}</span>
            </div>
            <div className="card tile">
              <span className="label">
                <Monitor size={14} /> Main display
              </span>
              <span className="value num">{display?.detected ?? "–"}</span>
              <span className="sub" title={display?.details[0]}>
                {display ? (display.status === "issue" ? `Supports ${display.expected}` : "At its maximum rate") : "Scan to check"}
              </span>
            </div>
            <div className="card tile">
              <span className="label">
                <CircuitBoard size={14} /> GPU driver
              </span>
              <span className="value num" style={{ fontSize: 20, paddingTop: 4 }}>
                {driverAge ? `${driverAge} days old` : gpu?.status === "issue" ? "Missing" : "–"}
              </span>
              <span className="sub" title={gpu?.details[0]}>
                {gpu?.details[0]?.split(" · ")[0] ?? "Scan to check"}
              </span>
            </div>
          </div>

          {scan && !tipDismissed && (
            <div className="banner">
              <Lightbulb size={18} />
              <div className="grow">
                <strong>Before you change anything, record a baseline.</strong>
                <p className="hint">
                  Benchmark two runs of a game you play often now, and two more after each fix. FrameWise shows whether the
                  difference is bigger than normal run-to-run variation.
                </p>
                {onOpenBenchmark && (
                  <button className="btn btn-sm" style={{ marginTop: 8 }} onClick={onOpenBenchmark}>
                    Record a benchmark
                  </button>
                )}
              </div>
              <button className="btn btn-ghost btn-sm" onClick={dismissTip} aria-label="Dismiss tip">
                <X size={16} />
              </button>
            </div>
          )}

          <section className="issue-list" aria-label="Top quick wins">
            <div className="section-head">
              <h2 className="section-title">
                <Gauge size={16} color="var(--accent)" /> Top quick wins
              </h2>
              {scan && issues.length > 0 && (
                <button className="btn-link" onClick={() => onOpenCheck("")}>
                  See all {scan.findings.length} checks
                </button>
              )}
            </div>
            {!scan && (
              <div className="card empty">
                <Gauge size={28} />
                <p className="muted">Run a scan to see what's holding your games back.</p>
              </div>
            )}
            {scan && top.length === 0 && (
              <div className="card empty">
                <ShieldCheck size={28} color="var(--success)" />
                <strong>No quick wins left</strong>
                <p className="muted">Every check that FrameWise can measure is in good shape.</p>
              </div>
            )}
            {top.map((f) => (
              <IssueCard key={f.check_id} finding={f} open={false} onToggle={() => onOpenCheck(f.check_id)} onRecheck={onRecheck} />
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}

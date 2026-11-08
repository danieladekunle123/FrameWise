import { useEffect, useRef, useState } from "react";
import { Copy, Feather, X } from "lucide-react";
import { api } from "../api";
import type { FootprintReport } from "../types";
import { usePoll } from "../usePoll";
import { Sparkline } from "./ui";

export function fmtMb(mb: number) {
  return mb >= 100 ? `${mb.toFixed(0)} MB` : `${mb.toFixed(1)} MB`;
}

export function fmtPct(p: number) {
  return `${p < 10 ? p.toFixed(1) : p.toFixed(0)}%`;
}

/** Permanent header indicator: FrameWise's own RAM and CPU across all its processes. */
export function FootprintIndicator() {
  const report = usePoll<FootprintReport | null>(api.footprint, 2000);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={ref}>
      <button
        className={`footprint-btn ${report?.over_budget ? "over" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        aria-haspopup="dialog"
        title="FrameWise's own memory and CPU use"
      >
        <span className="dot" aria-hidden="true" />
        FrameWise:
        {report ? (
          <span className="num">
            <strong>{fmtMb(report.ram_mb)}</strong> RAM · <strong>{fmtPct(report.cpu_pct)}</strong> CPU
          </span>
        ) : (
          <span>measuring…</span>
        )}
      </button>
      {open && <FootprintPanel report={report ?? null} onClose={() => setOpen(false)} />}
    </div>
  );
}

function FootprintPanel({ report, onClose }: { report: FootprintReport | null; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    if (!report) return;
    const text = [
      `FrameWise footprint report`,
      `Total: ${fmtMb(report.ram_mb)} RAM, ${fmtPct(report.cpu_pct)} CPU (budget ${report.budget_ram_mb} MB, ${report.budget_cpu_pct}%)`,
      ...report.processes.map((p) => `${p.role} (${p.name} ×${p.count}): ${fmtMb(p.ram_mb)}, ${fmtPct(p.cpu_pct)}`),
    ].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      /* clipboard unavailable */
    }
  };

  return (
    <div className="card popover" role="dialog" aria-label="FrameWise footprint">
      <div className="section-head">
        <h2 className="section-title">
          <Feather size={16} color="var(--accent)" /> FrameWise's own footprint
        </h2>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close">
          <X size={16} />
        </button>
      </div>
      {!report ? (
        <p className="muted">Measuring…</p>
      ) : (
        <>
          <table className="fp-table num">
            <thead>
              <tr>
                <th>Process</th>
                <th>RAM</th>
                <th>CPU</th>
              </tr>
            </thead>
            <tbody>
              {report.processes.map((p) => (
                <tr key={p.role}>
                  <td>
                    {p.role}
                    {p.count > 1 && <span className="muted"> ×{p.count}</span>}
                  </td>
                  <td>{fmtMb(p.ram_mb)}</td>
                  <td>{fmtPct(p.cpu_pct)}</td>
                </tr>
              ))}
              <tr className="future">
                <td>Background watcher</td>
                <td colSpan={2}>Not running</td>
              </tr>
              <tr className="future">
                <td>Admin helper</td>
                <td colSpan={2}>Not running</td>
              </tr>
              <tr className="future">
                <td>Monitor and overlay</td>
                <td colSpan={2}>Off</td>
              </tr>
            </tbody>
          </table>
          <div>
            <div className="subhead">RAM, last 10 minutes</div>
            <Sparkline values={report.history.map((h) => h.ram_mb)} limit={report.budget_ram_mb} label="RAM use over the last 10 minutes" />
          </div>
          <div>
            <div className="subhead">CPU, last 10 minutes</div>
            <Sparkline values={report.history.map((h) => h.cpu_pct)} max={report.budget_cpu_pct} label="CPU use over the last 10 minutes" />
          </div>
          <p className="hint">
            Measured the way Task Manager does (private working set), every 2 seconds while this window is open.
            Budget while open: under {report.budget_ram_mb} MB and {report.budget_cpu_pct}% CPU. Closing the window ends
            FrameWise completely; nothing runs in the background in this version.
          </p>
          {report.over_budget && (
            <div className="banner" style={{ ["--c" as string]: "var(--warning)" }}>
              <div className="grow">
                <strong>FrameWise is over its budget.</strong>
                <p className="hint">That's a bug. Copy these numbers and include them in a report.</p>
              </div>
              <button className="btn btn-sm" onClick={copy}>
                <Copy size={14} /> {copied ? "Copied" : "Copy details"}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

import type { ReactNode } from "react";
import {
  CircleCheck,
  CircleHelp,
  CircleMinus,
  Scale,
  TriangleAlert,
  Zap,
  type LucideIcon,
} from "lucide-react";
import type { Finding, Impact } from "../types";

export const IMPACT_RANK: Record<Impact, number> = { high: 0, medium: 1, low_medium: 2, low: 3 };

export const IMPACT_LABEL: Record<Impact, string> = {
  high: "High impact",
  medium: "Medium impact",
  low_medium: "Low–medium impact",
  low: "Low impact",
};

const IMPACT_CLASS: Record<Impact, string> = {
  high: "c-high",
  medium: "c-medium",
  low_medium: "c-low",
  low: "c-low",
};

export interface StatusMeta {
  label: string;
  icon: LucideIcon;
  cls: string;
}

/** Every status has an icon and a word, never colour alone. */
export function statusMeta(f: Finding): StatusMeta {
  switch (f.status) {
    case "issue":
      if (f.action === "choose") return { label: "Your call", icon: Scale, cls: "c-accent" };
      return { label: "Needs attention", icon: TriangleAlert, cls: IMPACT_CLASS[f.impact] };
    case "ok":
      return { label: "Good", icon: CircleCheck, cls: "c-ok" };
    case "unknown":
      return { label: "Check manually", icon: CircleHelp, cls: "c-low" };
    case "not_applicable":
      return { label: "Doesn't apply", icon: CircleMinus, cls: "c-low" };
  }
}

export function StatusIcon({ finding }: { finding: Finding }) {
  const m = statusMeta(finding);
  const Icon = m.icon;
  return (
    <span className={`status-icon ${m.cls}`} aria-hidden="true">
      <Icon size={18} strokeWidth={2.2} />
    </span>
  );
}

export function StatusBadge({ finding }: { finding: Finding }) {
  const m = statusMeta(finding);
  const Icon = m.icon;
  return (
    <span className={`badge ${m.cls}`}>
      <Icon size={13} strokeWidth={2.4} />
      {m.label}
    </span>
  );
}

export function ImpactBadge({ impact }: { impact: Impact }) {
  const cls = impact === "low" || impact === "low_medium" ? "neutral" : IMPACT_CLASS[impact];
  return (
    <span className={`badge ${cls}`}>
      <Zap size={12} strokeWidth={2.4} />
      {IMPACT_LABEL[impact]}
    </span>
  );
}

export function Badge({ cls = "neutral", icon: Icon, children }: { cls?: string; icon?: LucideIcon; children: ReactNode }) {
  return (
    <span className={`badge ${cls}`}>
      {Icon && <Icon size={12} strokeWidth={2.4} />}
      {children}
    </span>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="switch"
      onClick={() => onChange(!checked)}
    />
  );
}

interface SparklineProps {
  values: number[];
  width?: number;
  height?: number;
  color?: string;
  max?: number;
  /** Draws a dashed reference line (for example a budget). */
  limit?: number;
  label: string;
}

export function Sparkline({ values, width = 360, height = 56, color = "var(--live)", max, limit, label }: SparklineProps) {
  if (values.length < 2) {
    return (
      <div className="hint" style={{ height, display: "grid", placeItems: "center" }}>
        Collecting samples…
      </div>
    );
  }
  const top = Math.max(max ?? 0, limit ?? 0, ...values) * 1.1 || 1;
  const step = width / (values.length - 1);
  const y = (v: number) => height - 2 - (v / top) * (height - 4);
  const d = values.map((v, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  return (
    <svg width="100%" height={height} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label={label}>
      {limit !== undefined && (
        <line x1="0" x2={width} y1={y(limit)} y2={y(limit)} stroke="var(--text-2)" strokeDasharray="4 4" strokeWidth="1" opacity="0.6" />
      )}
      <path d={`${d} L${width},${height} L0,${height} Z`} fill={color} opacity="0.12" />
      <path d={d} fill="none" stroke={color} strokeWidth="2" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
    </svg>
  );
}

export function scoreColor(score: number): string {
  if (score < 50) return "var(--critical)";
  if (score < 75) return "var(--warning)";
  return "var(--success)";
}

export function scoreWord(score: number): string {
  if (score >= 90) return "Excellent";
  if (score >= 75) return "Good shape";
  if (score >= 50) return "Room to improve";
  return "Needs attention";
}

export function ScoreRing({ score }: { score: number | null }) {
  const r = 84;
  const c = 2 * Math.PI * r;
  const pct = score === null ? 0 : score / 100;
  const color = score === null ? "var(--border)" : scoreColor(score);
  return (
    <div className="score-ring" role="img" aria-label={score === null ? "No scan yet" : `Health score ${score} out of 100`}>
      <svg width="196" height="196" viewBox="0 0 196 196">
        <circle cx="98" cy="98" r={r} fill="none" stroke="var(--surface-2)" strokeWidth="14" />
        <circle
          className="arc"
          cx="98"
          cy="98"
          r={r}
          fill="none"
          stroke={color}
          strokeWidth="14"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - pct)}
        />
      </svg>
      <div className="value">
        <span className="n num">{score ?? "–"}</span>
        <span className="of">{score === null ? "not scanned" : "out of 100"}</span>
      </div>
    </div>
  );
}

export function timeAgo(unixSecs: number): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - unixSecs));
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(unixSecs * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

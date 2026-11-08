import { useState } from "react";
import { ChevronDown, ExternalLink, Minus, Plus, Power, RefreshCw, Settings2, Shield, Undo2, Wrench } from "lucide-react";
import { api, errorText } from "../api";
import { useFixes } from "../fixes";
import type { Finding } from "../types";
import { Badge, ImpactBadge, StatusBadge, StatusIcon, timeAgo } from "./ui";

const FIX_ARRIVES: Record<number, string> = {
  3: "One-click fixes for this arrive with the live monitor update.",
  4: "FrameWise will do this automatically with game profiles.",
};

interface Props {
  finding: Finding;
  open: boolean;
  onToggle: () => void;
  onRecheck: (checkId: string) => Promise<void>;
}

export function IssueCard({ finding: f, open, onToggle, onRecheck }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fixes = useFixes();
  const active = fixes.activeFor(f.check_id);
  const restart = active.some((c) => c.restart_pending);
  const showImpact = f.status === "issue" || f.status === "unknown";
  const bodyId = `issue-${f.check_id}`;

  const recheck = async () => {
    setBusy(true);
    setError(null);
    try {
      await onRecheck(f.check_id);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const openUri = async (uri: string) => {
    setError(null);
    try {
      await api.openExternal(uri);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <article
      className={`card issue is-${f.impact} status-${f.status}`}
      data-open={open}
      data-check={f.check_id}
    >
      <button className="issue-head" onClick={onToggle} aria-expanded={open} aria-controls={bodyId}>
        <StatusIcon finding={f} />
        <div className="issue-title">
          <h3>{f.title}</h3>
          <p>{f.summary}</p>
        </div>
        <div className="issue-badges">
          {restart && (
            <Badge cls="c-medium" icon={Power}>
              Restart to finish
            </Badge>
          )}
          {showImpact && <ImpactBadge impact={f.impact} />}
          <StatusBadge finding={f} />
        </div>
        <ChevronDown className="chev" size={18} />
      </button>

      {open && (
        <div className="issue-body" id={bodyId}>
          <p>{f.explanation}</p>

          {(f.detected || f.expected) && (
            <dl className="kv">
              <div>
                <dt>On this PC</dt>
                <dd className="num">{f.detected ?? "Unknown"}</dd>
              </div>
              <div>
                <dt>Recommended</dt>
                <dd className="num">{f.expected ?? "—"}</dd>
              </div>
            </dl>
          )}

          {f.details.length > 0 && (
            <ul className="details">
              {f.details.map((d, i) => (
                <li key={i}>{d}</li>
              ))}
            </ul>
          )}

          {f.action === "choose" && f.guide.choices.length > 0 && (
            <div>
              <div className="subhead">Your options</div>
              <div className="choices">
                {f.guide.choices.map((c) => (
                  <div className="choice" key={c.label}>
                    <h4>{c.label}</h4>
                    <div className="row">
                      <Plus size={14} color="var(--success)" aria-label="Gain" />
                      <span>{c.gains}</span>
                    </div>
                    <div className="row">
                      <Minus size={14} color="var(--critical)" aria-label="Cost" />
                      <span>{c.costs}</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {f.status === "issue" && f.action === "fix" && (
            <div>
              <div className="subhead">What the fix changes</div>
              <p>{f.fix_changes}</p>
            </div>
          )}

          <div>
            <div className="subhead">{f.status === "ok" ? "How to check or change it" : "Steps"}</div>
            <ol className="steps">
              {f.guide.steps.map((s, i) => (
                <li key={i}>{s}</li>
              ))}
            </ol>
          </div>

          {active.length > 0 && (
            <div className="changed-box">
              <div className="subhead">Changed by FrameWise</div>
              {active.map((c) => (
                <div className="changed-row" key={c.id}>
                  <span className="grow">
                    {c.title}: {c.old_label} → {c.new_label}
                    <span className="hint"> · {timeAgo(c.applied_at)}</span>
                    {c.restart_pending && <span className="hint"> · takes effect after a restart</span>}
                  </span>
                  <button className="btn btn-sm" onClick={() => fixes.undo([c.id])} disabled={fixes.busy}>
                    <Undo2 size={14} /> Undo
                  </button>
                </div>
              ))}
            </div>
          )}

          {f.status === "issue" && !f.fix && f.action === "fix" && f.one_click_phase && FIX_ARRIVES[f.one_click_phase] && (
            <div className="fix-note">
              <span>{FIX_ARRIVES[f.one_click_phase]} Until then, the steps above do the same thing by hand.</span>
            </div>
          )}

          <div className="action-row">
            {f.fix && (
              <button
                className={`btn btn-sm ${f.fix.kind === "choose" ? "" : "btn-primary"}`}
                onClick={() => fixes.fix(f)}
                disabled={fixes.busy}
                title={f.fix.needs_admin ? "Windows will ask for permission" : undefined}
              >
                {f.fix.needs_admin ? <Shield size={14} /> : <Wrench size={14} />}
                {f.fix.label}
              </button>
            )}
            {f.guide.settings_uri && (
              <button className="btn btn-sm" onClick={() => openUri(f.guide.settings_uri!)}>
                <Settings2 size={14} />
                {f.guide.settings_uri.startsWith("windowsdefender") ? "Open Windows Security" : "Open Windows setting"}
              </button>
            )}
            {f.guide.links.map((l) => (
              <button className="btn btn-sm" key={l.url} onClick={() => openUri(l.url)}>
                <ExternalLink size={14} />
                {l.label}
              </button>
            ))}
            <button className="btn btn-sm btn-ghost" onClick={recheck} disabled={busy}>
              <RefreshCw size={14} className={busy ? "spin" : undefined} />
              {busy ? "Rechecking…" : "Recheck"}
            </button>
          </div>
          {f.fix && (f.fix.needs_restart || f.fix.needs_admin) && (
            <p className="hint">
              {[f.fix.needs_admin && "Windows asks for permission because this setting is for every user on the PC.", f.fix.needs_restart && "It takes effect after a restart."]
                .filter(Boolean)
                .join(" ")}
            </p>
          )}
          {error && (
            <p role="alert" className="error-text">
              {error}
            </p>
          )}
        </div>
      )}
    </article>
  );
}

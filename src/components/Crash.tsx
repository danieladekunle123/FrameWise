import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from "react";
import { Bug, ExternalLink, Trash2, X } from "lucide-react";
import { api, errorText } from "../api";
import { Modal } from "./Modal";
import { timeAgo } from "./ui";
import type { CrashReport, IssueDraft } from "../types";

// Crash reports never leave the PC on their own. FrameWise shows the exact text, and "Open on
// GitHub" opens a pre-filled issue in the browser: it's only sent if you submit it there.

/** Shows a report's exact text and lets the user open it as a GitHub issue. */
export function IssueModal({ load, title, onClose, onDelete }: { load: () => Promise<IssueDraft>; title: string; onClose: () => void; onDelete?: () => void }) {
  const [draft, setDraft] = useState<IssueDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    load().then(setDraft).catch((e) => setError(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <Modal
      title={title}
      icon={<Bug size={20} color="var(--accent)" />}
      onClose={onClose}
      wide
      actions={
        <>
          {onDelete && (
            <button className="btn" onClick={onDelete}>
              <Trash2 size={14} /> Delete report
            </button>
          )}
          <span className="grow" />
          <button className="btn" onClick={onClose} data-autofocus>
            Close
          </button>
          <button className="btn btn-primary" disabled={!draft} onClick={() => draft && api.openExternal(draft.url).then(onClose)}>
            <ExternalLink size={14} /> Open on GitHub
          </button>
        </>
      }
    >
      <p className="hint">
        This is everything the report contains. "Open on GitHub" opens a new issue with this text in your browser; nothing is sent unless you
        submit it there, and you can edit it first.
      </p>
      {error && <p className="error-text">{error}</p>}
      {draft && (
        <pre className="report-text">
          <strong>{draft.title}</strong>
          {"\n\n"}
          {draft.body}
        </pre>
      )}
    </Modal>
  );
}

/** On launch: if FrameWise (or its watcher or helper) crashed since last time, say so. */
export function CrashNotice() {
  const [reports, setReports] = useState<CrashReport[]>([]);
  const [open, setOpen] = useState<CrashReport | null>(null);
  const load = () => api.crashReports().then(setReports).catch(() => {});
  useEffect(() => {
    load();
  }, []);
  const latest = reports[0];
  if (!latest) return null;
  const what = { app: "FrameWise", watcher: "The game-profile watcher", helper: "The FrameWise helper", interface: "FrameWise's window" }[latest.process] ?? "FrameWise";
  const dismissAll = async () => {
    for (const r of reports) await api.crashDismiss(r.id);
    setReports([]);
  };
  return (
    <>
      <div className="update-banner crash-banner" role="status">
        <Bug size={16} />
        <span className="grow">
          <strong>{what} closed unexpectedly</strong>
          <span className="hint">
            {" "}
            {timeAgo(latest.at)}
            {reports.length > 1 ? ` · ${reports.length} reports` : ""}. A short report was saved on this PC.
          </span>
        </span>
        <button className="btn btn-sm" onClick={() => setOpen(latest)}>
          Review report
        </button>
        <button className="btn btn-sm btn-ghost" onClick={dismissAll} aria-label="Dismiss and delete the reports">
          <X size={14} />
        </button>
      </div>
      {open && (
        <IssueModal
          title="Crash report"
          load={() => api.crashIssue(open.id)}
          onClose={() => setOpen(null)}
          onDelete={async () => {
            await api.crashDismiss(open.id);
            setOpen(null);
            load();
          }}
        />
      )}
    </>
  );
}

/** Catches errors in the interface: records a scrubbed report and offers a reload. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const where = info.componentStack?.split("\n").find((l) => l.trim())?.trim() ?? "";
    api.crashRecord(`${error.name}: ${error.message}${where ? ` (${where})` : ""}`).catch(() => {});
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="crash-screen">
        <Bug size={32} />
        <h2>Something went wrong in this window</h2>
        <p className="muted">
          Your PC wasn't changed by this. A short report was saved on this PC; FrameWise offers to show it when it reopens.
        </p>
        <button className="btn btn-primary" onClick={() => window.location.reload()}>
          Reload FrameWise
        </button>
      </div>
    );
  }
}

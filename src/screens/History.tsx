import { useEffect, useState } from "react";
import { CircleCheck, FileDown, History as HistoryIcon, LifeBuoy, Power, Shield, Undo2 } from "lucide-react";
import { api, errorText } from "../api";
import { Badge, Sparkline, scoreWord, timeAgo } from "../components/ui";
import { useFixes } from "../fixes";
import type { ChangeRow, ScoreHistoryPoint } from "../types";

const SOURCE: Record<string, string> = {
  manual: "Fix",
  fix_all: "Fix all safe",
  cleanup: "Cleanup",
};

function when(t: number) {
  return new Date(t * 1000).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

export function History() {
  const fixes = useFixes();
  const [scans, setScans] = useState<ScoreHistoryPoint[] | null>(null);
  useEffect(() => {
    api.scoreHistory().then(setScans).catch(() => setScans([]));
    fixes.refreshLog();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const log = fixes.log;
  const changes = log?.changes ?? [];
  const undoable = changes.filter((c) => c.undoable);

  const exportReport = async () => {
    try {
      const path = await api.exportReport();
      fixes.notify({ kind: "ok", title: "Report saved", lines: [path] });
    } catch (e) {
      fixes.notify({ kind: "error", title: "Couldn't save the report", lines: [errorText(e)] });
    }
  };

  return (
    <div className="page">
      <section className="card">
        <div className="card-pad section-head">
          <h2 className="section-title">
            <Undo2 size={16} color="var(--accent)" /> Changes
          </h2>
          <div className="head-actions">
            <button className="btn btn-sm" onClick={exportReport} disabled={fixes.busy}>
              <FileDown size={14} /> Export report
            </button>
            {undoable.length > 0 && (
              <button className="btn btn-sm" onClick={fixes.undoAll} disabled={fixes.busy}>
                <Undo2 size={14} /> Undo everything ({undoable.length})
              </button>
            )}
          </div>
        </div>
        {changes.length === 0 ? (
          <div className="empty" style={{ paddingTop: 4 }}>
            <CircleCheck size={28} color="var(--success)" />
            <strong>FrameWise hasn't changed anything on this PC</strong>
            <p className="muted" style={{ maxWidth: 480 }}>
              Every change will be listed here with the time, its old and new value and an Undo button.
            </p>
          </div>
        ) : (
          <ul className="change-list">
            {changes.map((c) => (
              <ChangeItem key={c.id} c={c} onUndo={() => fixes.undo([c.id])} busy={fixes.busy} />
            ))}
          </ul>
        )}
      </section>

      {log && log.restore_points.length > 0 && (
        <section className="card card-pad" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <h2 className="section-title">
            <LifeBuoy size={16} color="var(--accent)" /> Restore points
          </h2>
          <p className="hint">
            If something goes badly wrong, Windows can roll the whole system back to one of these: search the Start menu for
            "Create a restore point", then choose System Restore.
          </p>
          <table className="fp-table num">
            <thead>
              <tr>
                <th>When</th>
                <th>Windows number</th>
                <th>Note</th>
              </tr>
            </thead>
            <tbody>
              {log.restore_points.map((r) => (
                <tr key={r.id}>
                  <td>{when(r.created_at)}</td>
                  <td>#{r.sequence ?? "?"}</td>
                  <td>{r.reused ? `Used "${r.description}", made by Windows in the 24 hours before` : r.description}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      <section className="card card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <h2 className="section-title">
          <HistoryIcon size={16} color="var(--accent)" /> Scans
        </h2>
        {scans === null ? (
          <p className="muted">Loading…</p>
        ) : scans.length === 0 ? (
          <p className="muted">No scans yet.</p>
        ) : (
          <>
            {scans.length > 1 && (
              <Sparkline values={scans.map((s) => s.score)} max={100} color="var(--accent)" height={64} label="Health score over recent scans" />
            )}
            <table className="fp-table num">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Score</th>
                  <th>Result</th>
                </tr>
              </thead>
              <tbody>
                {[...scans].reverse().map((s) => (
                  <tr key={s.started_at}>
                    <td>
                      {new Date(s.started_at * 1000).toLocaleString()} <span className="muted">· {timeAgo(s.started_at)}</span>
                    </td>
                    <td>{s.score}</td>
                    <td>{scoreWord(s.score)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </section>
    </div>
  );
}

function ChangeItem({ c, onUndo, busy }: { c: ChangeRow; onUndo: () => void; busy: boolean }) {
  const what = c.old_label && c.new_label ? `${c.old_label} → ${c.new_label}` : c.detail ?? "";
  return (
    <li className={c.reverted_at ? "is-reverted" : undefined}>
      <div className="grow">
        <div className="change-title">
          <strong>{c.title}</strong>
          <Badge>{SOURCE[c.source] ?? c.source}</Badge>
          {c.needs_admin && <Badge icon={Shield}>All users</Badge>}
          {c.restart_pending && (
            <Badge cls="c-medium" icon={Power}>
              Restart to finish
            </Badge>
          )}
        </div>
        <span className="hint">
          {what} · {when(c.applied_at)}
          {c.reverted_at && ` · undone ${when(c.reverted_at)}`}
          {c.source === "cleanup" && c.detail?.startsWith("Moved") && " · restore from the Recycle Bin if needed"}
        </span>
      </div>
      {c.undoable && (
        <button className="btn btn-sm" onClick={onUndo} disabled={busy}>
          <Undo2 size={14} /> Undo
        </button>
      )}
      {c.reverted_at && <span className="hint">Undone</span>}
    </li>
  );
}

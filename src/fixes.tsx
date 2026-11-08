// Fix controller: runs fixes, Fix all safe and Undo through the backend, and owns the dialogs
// around them (restore point problems, confirmations, the refresh-rate countdown, the startup
// picker), the progress overlay and the result toast.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { CircleCheck, Monitor, RotateCcw, Shield, ShieldAlert, TriangleAlert, Undo2, Wrench } from "lucide-react";
import { api, errorCode, errorText, onEvent, type FixOptions } from "./api";
import { Modal, ProgressOverlay } from "./components/Modal";
import type { ChangeLog, ChangeRow, Finding, FixOutcome, StartupEntry, StartupPick, UndoOutcome } from "./types";

const RP_KEY = "framewise.restorePoint";

export function useRestorePointPref(): [boolean, (v: boolean) => void] {
  const [on, setOn] = useState(() => {
    try {
      return localStorage.getItem(RP_KEY) !== "0";
    } catch {
      return true;
    }
  });
  const set = (v: boolean) => {
    setOn(v);
    try {
      localStorage.setItem(RP_KEY, v ? "1" : "0");
    } catch {
      /* ignore */
    }
  };
  return [on, set];
}

interface Toast {
  kind: "ok" | "warn" | "error";
  title: string;
  lines: string[];
  undoIds?: number[];
}

interface FixContext {
  log: ChangeLog | null;
  refreshLog: () => Promise<void>;
  /** Changes still in effect for a check, newest first. */
  activeFor: (checkId: string) => ChangeRow[];
  restartPending: ChangeRow[];
  busy: boolean;
  fix: (f: Finding) => void;
  fixAllSafe: (findings: Finding[]) => void;
  undo: (ids: number[]) => void;
  undoAll: () => void;
  notify: (t: Toast) => void;
  runTask: <T>(work: () => Promise<T>) => Promise<T | undefined>;
}

const Ctx = createContext<FixContext | null>(null);

export function useFixes(): FixContext {
  const c = useContext(Ctx);
  if (!c) throw new Error("useFixes outside FixProvider");
  return c;
}

type Dialog =
  | { kind: "restore"; code: string; message: string; retry: (skipRestorePoint: boolean) => void }
  | { kind: "confirm"; title: string; body: ReactNode; confirmLabel: string; danger?: boolean; onConfirm: () => void }
  | { kind: "startup"; finding: Finding }
  | { kind: "refresh"; ids: number[]; seconds: number }
  | { kind: "changed"; message: string; ids: number[] };

interface Props {
  children: ReactNode;
  restorePoint: boolean;
  onFindings: (findings: Finding[], score: number | null) => void;
  onOpenCleanup: () => void;
}

function summarise(out: FixOutcome): Toast {
  const lines: string[] = [];
  for (const c of out.changes) lines.push(`${c.title}: ${c.old_label ?? "?"} → ${c.new_label ?? "?"}`);
  for (const a of out.already_set) lines.push(`${a} was already set.`);
  for (const e of out.errors) lines.push(e.message);
  if (out.restore_point) {
    lines.unshift(
      out.restore_point.reused
        ? "Windows already made a restore point in the last 24 hours, so that one covers these changes."
        : "A Windows restore point was created first.",
    );
  }
  if (out.changes.some((c) => c.needs_restart)) lines.push("Restart Windows to finish.");
  const n = out.changes.length;
  if (n === 0 && out.errors.length === 0) return { kind: "ok", title: "Nothing needed changing", lines };
  if (out.errors.length > 0 && n === 0) return { kind: "error", title: "Nothing was changed", lines };
  return {
    kind: out.errors.length ? "warn" : "ok",
    title: n === 1 ? "1 change made" : `${n} changes made`,
    lines,
    undoIds: out.changes.map((c) => c.id),
  };
}

export function FixProvider({ children, restorePoint, onFindings, onOpenCleanup }: Props) {
  const [log, setLog] = useState<ChangeLog | null>(null);
  const [busyText, setBusyText] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const toastTimer = useRef<number>();

  const refreshLog = useCallback(async () => {
    try {
      setLog(await api.changeLog());
    } catch {
      /* the History screen shows the error */
    }
  }, []);

  useEffect(() => {
    refreshLog();
    const offProgress = onEvent<string>("task-progress", (t) => setBusyText((b) => (b === null ? b : t || "Working…")));
    const offReverted = onEvent<number[]>("changes-auto-reverted", (ids) => {
      setDialog((d) => (d?.kind === "refresh" ? null : d));
      if (ids.length) {
        setToast({ kind: "warn", title: "Refresh rate put back", lines: ["It wasn't confirmed within 15 seconds, so the previous rate was restored."] });
        api.recheck("refresh_rate").then((r) => onFindings([r.finding], r.score)).catch(() => undefined);
      }
      refreshLog();
    });
    return () => {
      offProgress();
      offReverted();
    };
  }, [refreshLog, onFindings]);

  const notify = useCallback((t: Toast) => {
    window.clearTimeout(toastTimer.current);
    setToast(t);
    toastTimer.current = window.setTimeout(() => setToast(null), t.kind === "ok" ? 9000 : 15000);
  }, []);

  /** Runs one backend task with the progress overlay. */
  const runTask = useCallback(
    async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
      setBusyText("Working…");
      try {
        return await work();
      } finally {
        setBusyText(null);
        refreshLog();
      }
    },
    [refreshLog],
  );

  const handleOutcome = useCallback(
    (out: FixOutcome) => {
      onFindings(out.findings, out.score);
      if (out.confirm_ids.length) {
        setDialog({ kind: "refresh", ids: out.confirm_ids, seconds: out.confirm_seconds });
      }
      notify(summarise(out));
    },
    [onFindings, notify],
  );

  /** Applies a fix; on a restore point problem, asks whether to continue without one. */
  const apply = useCallback(
    async (call: (opts: FixOptions) => Promise<FixOutcome>, skipRestorePoint = false) => {
      try {
        const out = await runTask(() => call({ restore_point: restorePoint && !skipRestorePoint }));
        if (out) handleOutcome(out);
      } catch (e) {
        const code = errorCode(e) ?? "error";
        if (["restore_point_off", "restore_point_failed", "elevation_cancelled", "elevation_failed", "helper_failed"].includes(code) && restorePoint && !skipRestorePoint) {
          setDialog({ kind: "restore", code, message: errorText(e), retry: (skip) => apply(call, skip) });
        } else {
          notify({ kind: "error", title: "Nothing was changed", lines: [errorText(e)] });
        }
      }
    },
    [restorePoint, runTask, handleOutcome, notify],
  );

  const fix = useCallback(
    (f: Finding) => {
      const info = f.fix;
      if (!info) return;
      const run = (input?: FixOptions["input"]) => apply((opts) => api.applyFix(f.check_id, { ...opts, input }));
      switch (info.kind) {
        case "cleanup":
          onOpenCleanup();
          return;
        case "pick_startup":
          setDialog({ kind: "startup", finding: f });
          return;
        case "choose":
          setDialog({
            kind: "confirm",
            title: `${info.label}?`,
            body: (
              <>
                <p>{info.confirm}</p>
                {info.needs_admin && <p className="hint">Windows will ask for permission.</p>}
              </>
            ),
            confirmLabel: info.label,
            danger: true,
            onConfirm: () => run({ enable: false }),
          });
          return;
        default:
          run();
      }
    },
    [apply, onOpenCleanup],
  );

  const fixAllSafe = useCallback(
    (findings: Finding[]) => {
      const safe = findings.filter((f) => f.status === "issue" && f.fix?.safe);
      if (safe.length === 0) return;
      setDialog({
        kind: "confirm",
        title: "Fix all safe issues?",
        body: (
          <>
            <p>FrameWise will change these settings:</p>
            <ul className="details">
              {safe.map((f) => (
                <li key={f.check_id}>
                  {f.title}: {f.fix?.label.toLowerCase()}
                </li>
              ))}
            </ul>
            <p className="hint">
              {restorePoint
                ? "A Windows restore point is made first (Windows asks for permission for that). Every change can be undone from History."
                : "Every change can be undone from History. Restore points are turned off in Settings."}{" "}
              Memory Integrity and other security settings are never included.
            </p>
          </>
        ),
        confirmLabel: `Fix ${safe.length} ${safe.length === 1 ? "issue" : "issues"}`,
        onConfirm: () => apply((opts) => api.fixAllSafe(opts)),
      });
    },
    [apply, restorePoint],
  );

  const undoWith = useCallback(
    async (call: (force: boolean) => Promise<UndoOutcome>, ids: number[], force = false) => {
      try {
        const out = await runTask(() => call(force));
        if (!out) return;
        onFindings(out.findings, out.score);
        const changed = out.errors.filter(([, e]) => e.code === "changed_since");
        const other = out.errors.filter(([, e]) => e.code !== "changed_since");
        if (changed.length) {
          setDialog({ kind: "changed", message: changed.map(([, e]) => e.message).join(" "), ids: changed.map(([id]) => id) });
        }
        const n = out.reverted.length;
        notify({
          kind: other.length ? "warn" : "ok",
          title: n === 0 ? "Nothing was undone" : n === 1 ? "1 change undone" : `${n} changes undone`,
          lines: other.map(([, e]) => e.message),
        });
      } catch (e) {
        notify({ kind: "error", title: "Couldn't undo", lines: [errorText(e)] });
      }
      void ids;
    },
    [runTask, onFindings, notify],
  );

  const undo = useCallback((ids: number[]) => undoWith((force) => api.undoChanges(ids, force), ids), [undoWith]);

  const undoAll = useCallback(() => {
    setDialog({
      kind: "confirm",
      title: "Undo every change?",
      body: <p>FrameWise puts every setting it changed back to its previous value, newest first. Cleanup can't be undone here; restore files from the Recycle Bin.</p>,
      confirmLabel: "Undo everything",
      onConfirm: () => undoWith((force) => api.undoAll(force), []),
    });
  }, [undoWith]);

  const value = useMemo<FixContext>(() => {
    const changes = log?.changes ?? [];
    return {
      log,
      refreshLog,
      activeFor: (id) => changes.filter((c) => c.check_id === id && c.undoable),
      restartPending: changes.filter((c) => c.restart_pending),
      busy: busyText !== null,
      fix,
      fixAllSafe,
      undo,
      undoAll,
      notify,
      runTask,
    };
  }, [log, refreshLog, busyText, fix, fixAllSafe, undo, undoAll, notify, runTask]);

  const close = () => setDialog(null);

  return (
    <Ctx.Provider value={value}>
      {children}

      {dialog?.kind === "restore" && (
        <Modal
          title={dialog.code === "restore_point_off" ? "System Restore is off" : "No restore point was made"}
          icon={<ShieldAlert size={20} color="var(--warning)" />}
          onClose={close}
          actions={
            <>
              <button className="btn" onClick={close}>
                Cancel
              </button>
              {dialog.code !== "restore_point_off" && (
                <button
                  className="btn"
                  onClick={() => {
                    close();
                    dialog.retry(false);
                  }}
                >
                  <RotateCcw size={14} /> Try again
                </button>
              )}
              <button
                className="btn btn-primary"
                onClick={() => {
                  close();
                  dialog.retry(true);
                }}
              >
                Continue without one
              </button>
            </>
          }
        >
          <p>{dialog.message}</p>
          <p className="hint">
            Nothing has changed yet. Without a restore point, FrameWise's own Undo still records and reverses every change.
            {dialog.code === "restore_point_off" && " You can turn System Restore on in Control Panel > System > System Protection."}
          </p>
        </Modal>
      )}

      {dialog?.kind === "confirm" && (
        <Modal
          title={dialog.title}
          icon={dialog.danger ? <TriangleAlert size={20} color="var(--warning)" /> : <Wrench size={20} color="var(--accent)" />}
          onClose={close}
          actions={
            <>
              <button className="btn" onClick={close} data-autofocus={dialog.danger ? true : undefined}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                onClick={() => {
                  close();
                  dialog.onConfirm();
                }}
              >
                {dialog.confirmLabel}
              </button>
            </>
          }
        >
          {dialog.body}
        </Modal>
      )}

      {dialog?.kind === "changed" && (
        <Modal
          title="Changed outside FrameWise"
          icon={<TriangleAlert size={20} color="var(--warning)" />}
          onClose={close}
          actions={
            <>
              <button className="btn" onClick={close}>
                Leave it
              </button>
              <button
                className="btn btn-primary"
                onClick={() => {
                  close();
                  undoWith((force) => api.undoChanges(dialog.ids, force), dialog.ids, true);
                }}
              >
                Undo anyway
              </button>
            </>
          }
        >
          <p>{dialog.message}</p>
        </Modal>
      )}

      {dialog?.kind === "refresh" && (
        <RefreshConfirm
          ids={dialog.ids}
          seconds={dialog.seconds}
          onKeep={() => {
            api.confirmChanges(dialog.ids).catch(() => undefined);
            close();
          }}
          onRevert={() => {
            close();
            undo(dialog.ids);
          }}
        />
      )}

      {dialog?.kind === "startup" && (
        <StartupPicker
          onClose={close}
          onDisable={(picks) => {
            close();
            apply((opts) => api.applyFix("startup_bloat", { ...opts, input: { startup: picks } }));
          }}
        />
      )}

      {busyText !== null && <ProgressOverlay text={busyText} />}

      {toast && (
        <div className={`toast card t-${toast.kind}`} role="status" aria-live="polite">
          {toast.kind === "ok" ? (
            <CircleCheck size={18} color="var(--success)" />
          ) : (
            <TriangleAlert size={18} color={toast.kind === "warn" ? "var(--warning)" : "var(--critical)"} />
          )}
          <div className="grow">
            <strong>{toast.title}</strong>
            {toast.lines.map((l, i) => (
              <p key={i} className="hint">
                {l}
              </p>
            ))}
          </div>
          {toast.undoIds && toast.undoIds.length > 0 && (
            <button
              className="btn btn-sm"
              onClick={() => {
                setToast(null);
                undo(toast.undoIds!);
              }}
            >
              <Undo2 size={14} /> Undo
            </button>
          )}
          <button className="btn btn-ghost btn-sm" onClick={() => setToast(null)} aria-label="Dismiss">
            ×
          </button>
        </div>
      )}
    </Ctx.Provider>
  );
}

function RefreshConfirm({ ids, seconds, onKeep, onRevert }: { ids: number[]; seconds: number; onKeep: () => void; onRevert: () => void }) {
  const [left, setLeft] = useState(seconds);
  useEffect(() => {
    const t = window.setInterval(() => setLeft((s) => Math.max(0, s - 1)), 1000);
    return () => window.clearInterval(t);
  }, []);
  return (
    <Modal
      title="Keep this refresh rate?"
      icon={<Monitor size={20} color="var(--accent)" />}
      actions={
        <>
          <button className="btn" onClick={onRevert}>
            Put it back
          </button>
          <button className="btn btn-primary" onClick={onKeep} data-autofocus>
            Keep
          </button>
        </>
      }
    >
      <p>
        If the picture looks right, choose Keep. Otherwise FrameWise puts the previous rate back in{" "}
        <strong className="num">{left}</strong> seconds{ids.length > 1 ? ` on all ${ids.length} displays` : ""}, in case the
        monitor can't show it.
      </p>
    </Modal>
  );
}

function StartupPicker({ onClose, onDisable }: { onClose: () => void; onDisable: (picks: StartupPick[]) => void }) {
  const [entries, setEntries] = useState<StartupEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  useEffect(() => {
    api.startupEntries().then(setEntries).catch((e) => setError(errorText(e)));
  }, []);
  const key = (e: StartupEntry) => `${e.source}|${e.value_name}`;
  const enabled = (entries ?? []).filter((e) => e.enabled && e.source);
  const picks: StartupPick[] = enabled.filter((e) => picked.has(key(e))).map((e) => ({ source: e.source!, value_name: e.value_name }));
  const needsAdmin = enabled.some((e) => picked.has(key(e)) && e.source!.startsWith("machine"));
  const toggle = (k: string) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });
  return (
    <Modal
      title="Choose apps to stop starting with Windows"
      wide
      onClose={onClose}
      actions={
        <>
          <span className="hint grow">
            {needsAdmin ? "Some of these are for all users, so Windows will ask for permission." : "The apps still work when you open them. Undo re-enables them."}
          </span>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={picks.length === 0} onClick={() => onDisable(picks)}>
            Disable {picks.length || ""} {picks.length === 1 ? "app" : "apps"}
          </button>
        </>
      }
    >
      <p className="hint">
        Same as Task Manager's Startup apps: nothing is uninstalled or deleted. Keep security software, audio and touchpad
        drivers, and anything you rely on in the tray.
      </p>
      {error && <p className="error-text">{error}</p>}
      {!entries && !error && <p className="muted">Loading…</p>}
      {entries && (
        <ul className="pick-list">
          {enabled.map((e) => (
            <li key={key(e)}>
              <label>
                <input type="checkbox" checked={picked.has(key(e))} onChange={() => toggle(key(e))} />
                <span className="grow">
                  <strong>{e.name}</strong>
                  <span className="hint" title={e.command}>
                    {e.location}
                    {e.command ? ` · ${e.command}` : ""}
                  </span>
                </span>
                {e.source!.startsWith("machine") && (
                  <span className="badge neutral" title="Windows asks for permission">
                    <Shield size={12} /> All users
                  </span>
                )}
              </label>
            </li>
          ))}
          {enabled.length === 0 && <li className="muted">No startup apps are enabled.</li>}
        </ul>
      )}
      {entries && entries.some((e) => !e.enabled) && (
        <p className="hint">{entries.filter((e) => !e.enabled).length} more are already disabled.</p>
      )}
    </Modal>
  );
}

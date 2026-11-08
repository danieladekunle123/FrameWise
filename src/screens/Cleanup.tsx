import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, CircleCheck, Recycle, RefreshCw, Shield, Trash2, TriangleAlert } from "lucide-react";
import { api, errorText, formatBytes } from "../api";
import { Modal } from "../components/Modal";
import { Badge } from "../components/ui";
import { useFixes } from "../fixes";
import type { CleanCategoryId, CleanupCategory, CleanupOutcome } from "../types";

export function Cleanup() {
  const fixes = useFixes();
  const [cats, setCats] = useState<CleanupCategory[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const [selected, setSelected] = useState<Set<CleanCategoryId>>(new Set());
  const [permanent, setPermanent] = useState(false);
  const [openDrivers, setOpenDrivers] = useState(false);
  const [result, setResult] = useState<CleanupOutcome | null>(null);
  const [confirming, setConfirming] = useState(false);

  const load = useCallback(
    async (elevated: boolean) => {
      setError(null);
      try {
        const next = elevated
          ? await fixes.runTask(() => api.cleanupScan(true))
          : await (async () => {
              setMeasuring(true);
              try {
                return await api.cleanupScan(false);
              } finally {
                setMeasuring(false);
              }
            })();
        if (!next) return;
        setCats(next);
        // Only measured categories can be selected: one that still needs permission to measure is
        // never ticked (its checkbox is disabled, so a tick there couldn't be undone).
        const selectable = (c: CleanupCategory) => c.scan.bytes > 0 && !c.scan.needs_permission;
        setSelected((prev) => {
          if (prev.size > 0 || elevated) return new Set([...prev].filter((id) => next.find((c) => c.id === id && selectable(c))));
          return new Set(next.filter((c) => c.default_selected && selectable(c)).map((c) => c.id));
        });
      } catch (e) {
        setError(errorText(e));
      }
    },
    [fixes],
  );

  useEffect(() => {
    load(false);
    // Measure once on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const chosen = useMemo(() => (cats ?? []).filter((c) => selected.has(c.id) && !c.scan.needs_permission), [cats, selected]);
  const total = chosen.reduce((a, c) => a + c.scan.bytes, 0);
  const needsPermission = (cats ?? []).some((c) => c.scan.needs_permission);
  const anyAdmin = chosen.some((c) => c.needs_admin);

  const toggle = (id: CleanCategoryId) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const clean = async () => {
    setConfirming(false);
    setResult(null);
    try {
      const out = await fixes.runTask(() => api.cleanupRun(chosen.map((c) => c.id), permanent));
      if (out) setResult(out);
    } catch (e) {
      setError(errorText(e));
    }
    load(false);
  };

  return (
    <div className="page">
      <div className="card card-pad upcoming-hero">
        <span className="icon">
          <Trash2 size={24} />
        </span>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <h2 style={{ fontSize: 18 }}>Cleanup</h2>
          <p className="muted">
            Junk that takes real space, measured before anything is removed. Your own files go to the Recycle Bin; Windows'
            own folders are deleted permanently, and say so. Nothing that's been changed in the last day is touched.
          </p>
        </div>
      </div>

      {result && <ResultBanner result={result} cats={cats ?? []} onClose={() => setResult(null)} />}
      {error && (
        <div className="banner error-box" role="alert">
          <TriangleAlert size={18} />
          <div className="grow">{error}</div>
        </div>
      )}

      <section className="card">
        <div className="card-pad section-head" style={{ paddingBottom: 6 }}>
          <h2 className="section-title">What can be removed</h2>
          <div className="head-actions">
            {needsPermission && (
              <button className="btn btn-sm" onClick={() => load(true)} disabled={fixes.busy || measuring}>
                <Shield size={14} /> Measure with permission
              </button>
            )}
            <button className="btn btn-sm btn-ghost" onClick={() => load(false)} disabled={fixes.busy || measuring}>
              <RefreshCw size={14} className={measuring ? "spin" : undefined} /> {measuring ? "Measuring…" : "Measure again"}
            </button>
          </div>
        </div>

        {!cats && (
          <div className="empty">
            <RefreshCw size={24} className="spin" />
            <p className="muted">Measuring. Temp folders with many files can take a few seconds…</p>
          </div>
        )}

        {cats && (
          <ul className="clean-list">
            {cats.map((c) => {
              const empty = c.scan.bytes === 0 && !c.scan.needs_permission;
              return (
                <li key={c.id} className={empty ? "is-empty" : undefined}>
                  <label>
                    <input
                      type="checkbox"
                      checked={selected.has(c.id)}
                      disabled={empty || c.scan.needs_permission || fixes.busy}
                      onChange={() => toggle(c.id)}
                    />
                    <span className="grow">
                      <strong>{c.title}</strong>
                      <span className="hint">{c.description}</span>
                      <span className="clean-badges">
                        {c.recyclable && !permanent ? (
                          <Badge icon={Recycle}>Recycle Bin</Badge>
                        ) : (
                          <Badge cls="c-medium" icon={TriangleAlert}>
                            Deleted permanently
                          </Badge>
                        )}
                        {c.needs_admin && <Badge icon={Shield}>Asks permission</Badge>}
                        {c.scan.recent_items > 0 && (
                          <span className="hint">
                            {c.scan.recent_items} recent item{c.scan.recent_items === 1 ? "" : "s"} kept
                          </span>
                        )}
                      </span>
                    </span>
                    <span className="clean-size num">
                      {c.scan.error ? (
                        <span className="hint">Couldn't measure</span>
                      ) : c.scan.needs_permission ? (
                        <span className="hint">Needs permission to measure</span>
                      ) : (
                        <>
                          {formatBytes(c.scan.bytes)}
                          <span className="hint">
                            {c.scan.items} item{c.scan.items === 1 ? "" : "s"}
                          </span>
                        </>
                      )}
                    </span>
                  </label>
                  {c.id === "driver_packages" && c.scan.drivers.length > 0 && (
                    <div className="drivers">
                      <button className="btn-link" onClick={() => setOpenDrivers((o) => !o)} aria-expanded={openDrivers}>
                        <ChevronDown size={14} style={{ transform: openDrivers ? "rotate(180deg)" : undefined }} />
                        {c.scan.drivers.length} graphics driver package{c.scan.drivers.length === 1 ? "" : "s"} in the driver store
                      </button>
                      {openDrivers && (
                        <table className="fp-table num">
                          <thead>
                            <tr>
                              <th>Package</th>
                              <th>Version</th>
                              <th>Date</th>
                              <th>Size</th>
                              <th>What happens</th>
                            </tr>
                          </thead>
                          <tbody>
                            {c.scan.drivers.map((d) => (
                              <tr key={d.inf}>
                                <td>
                                  {d.provider} <span className="hint">{d.inf}</span>
                                </td>
                                <td>{d.version}</td>
                                <td>{d.date}</td>
                                <td>{formatBytes(d.bytes)}</td>
                                <td>{d.in_use ? "Kept: in use" : d.kept_for_rollback ? "Kept: for Roll Back Driver" : "Removed"}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      )}
                    </div>
                  )}
                  {c.scan.error && <p className="error-text">{c.scan.error}</p>}
                </li>
              );
            })}
          </ul>
        )}

        {cats && (
          <div className="clean-foot">
            <label className="check-row">
              <input type="checkbox" checked={permanent} onChange={(e) => setPermanent(e.target.checked)} disabled={fixes.busy} />
              <span>
                Skip the Recycle Bin
                <span className="hint"> · frees the space now, but files can't be restored</span>
              </span>
            </label>
            <div className="grow" />
            <button className="btn btn-primary" disabled={chosen.length === 0 || total === 0 || fixes.busy} onClick={() => setConfirming(true)}>
              <Trash2 size={16} /> Clean selected{total > 0 ? ` (${formatBytes(total)})` : ""}
            </button>
          </div>
        )}
      </section>

      {confirming && (
        <Modal
          title={`Clean ${formatBytes(total)}?`}
          icon={<Trash2 size={20} color="var(--accent)" />}
          onClose={() => setConfirming(false)}
          actions={
            <>
              <button className="btn" onClick={() => setConfirming(false)} data-autofocus>
                Cancel
              </button>
              <button className="btn btn-primary" onClick={clean}>
                Clean
              </button>
            </>
          }
        >
              <ul className="details">
                {chosen.map((c) => (
                  <li key={c.id}>
                    {c.title}: {formatBytes(c.scan.bytes)} · {c.recyclable && !permanent ? "to the Recycle Bin" : "deleted permanently"}
                  </li>
                ))}
              </ul>
              {chosen.some((c) => !c.recyclable || permanent) && (
                <p className="hint">Permanently deleted items can't be restored. Windows recreates anything it still needs.</p>
              )}
              {anyAdmin && <p className="hint">Windows will ask for permission for its own folders.</p>}
        </Modal>
      )}
    </div>
  );
}

function ResultBanner({ result, cats, onClose }: { result: CleanupOutcome; cats: CleanupCategory[]; onClose: () => void }) {
  const errors = result.results.filter((r) => r.error);
  const skipped = result.results.reduce((a, r) => a + r.skipped_items, 0);
  const title = (id: string) => cats.find((c) => c.id === id)?.title ?? id;
  return (
    <div className={`banner ${errors.length && !result.freed_bytes && !result.recycled_bytes ? "error-box" : ""}`} style={{ ["--c" as string]: errors.length ? "var(--warning)" : "var(--success)" }} role="status">
      <CircleCheck size={18} />
      <div className="grow">
        <strong>
          {[result.freed_bytes > 0 && `Freed ${formatBytes(result.freed_bytes)}`, result.recycled_bytes > 0 && `moved ${formatBytes(result.recycled_bytes)} to the Recycle Bin`]
            .filter(Boolean)
            .join(", ")
            .replace(/^m/, "M") || "Nothing was removed"}
        </strong>
        {result.recycled_bytes > 0 && <p className="hint">Empty the Recycle Bin to free that space, or restore anything you need from it.</p>}
        {skipped > 0 && <p className="hint">{skipped} items were in use and left alone.</p>}
        {errors.map((r) => (
          <p className="hint" key={r.id}>
            {title(r.id)}: {r.error}
          </p>
        ))}
      </div>
      <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Dismiss">
        ×
      </button>
    </div>
  );
}

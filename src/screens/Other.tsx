import { useCallback, useEffect, useState } from "react";
import {
  BookOpen,
  ExternalLink,
  FlaskConical,
  LifeBuoy,
  LockKeyhole,
  Shield,
  ShieldCheck,
} from "lucide-react";
import { api, errorText, formatBytes } from "../api";
import { useFixes } from "../fixes";
import { Badge, Switch } from "../components/ui";
import type { AppInfo, Myth, StandbyStatus } from "../types";
import type { ThemePref } from "../theme";
import { UpdatesSettings } from "../updates";

// ------------------------------------------------------------------ standby list (advanced)

function StandbyPanel() {
  const fixes = useFixes();
  const [st, setSt] = useState<StandbyStatus | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.standbyStatus().then(setSt).catch(() => {});
  }, []);
  const purge = async () => {
    setError(null);
    setResult(null);
    try {
      const r = await fixes.runTask(() => api.standbyPurge());
      if (!r) return;
      setResult(
        r.before != null && r.after != null
          ? `Cleared: the standby list went from ${formatBytes(r.before)} to ${formatBytes(r.after)}. Windows refills it as you use files; benchmark your game now and compare.`
          : "Cleared. Benchmark your game now and compare with a run from before.",
      );
      api.standbyStatus().then(setSt).catch(() => {});
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div className="standby-panel">
      <p className="hint">
        <strong>On this PC:</strong> {st ? formatBytes(st.ram_total_bytes) : "…"} of RAM
        {st?.standby_bytes != null ? `, ${formatBytes(st.standby_bytes)} on the standby list right now` : ""}.{" "}
        {st && (st.may_help ? "That's in the range where clearing has sometimes helped." : "With more than 16 GB, clearing isn't expected to help here.")}
      </p>
      <ol className="hint standby-steps">
        <li>Record two benchmarks of a game (Monitor &amp; benchmark).</li>
        <li>Clear the standby list, then record two more.</li>
        <li>Keep doing it only if the comparison says "Improvement". Windows refills the list over time, so it's a one-off, not a fix.</li>
      </ol>
      <div className="standby-actions">
        <button className="btn btn-sm" onClick={purge} disabled={fixes.busy}>
          <Shield size={14} /> Clear the standby list now
        </button>
        <span className="hint">Asks Windows for permission once. Nothing else changes.</span>
      </div>
      {result && <p className="hint good-text">{result}</p>}
      {error && <p className="error-text">{error}</p>}
    </div>
  );
}

// ------------------------------------------------------------------ myths

export function Myths() {
  const [myths, setMyths] = useState<Myth[]>([]);
  useEffect(() => {
    api.myths().then(setMyths).catch(() => setMyths([]));
  }, []);
  return (
    <div className="page">
      <div className="card card-pad upcoming-hero">
        <span className="icon">
          <BookOpen size={24} />
        </span>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <h2 style={{ fontSize: 18 }}>What FrameWise deliberately doesn't do</h2>
          <p className="muted">
            Many optimisers sell satisfying numbers from tweaks that do nothing or make things worse. FrameWise only
            changes what measurably helps. Here's what it leaves alone, and why.
          </p>
        </div>
      </div>
      {myths.map((m) => (
        <article key={m.id} className={`card myth ${m.advanced_exception ? "exception" : ""}`}>
          <span className="claim">{m.claim}</span>
          <h3>
            {m.advanced_exception ? <FlaskConical size={18} color="var(--warning)" /> : <ShieldCheck size={18} color="var(--success)" />}
            {m.verdict}
          </h3>
          {m.advanced_exception && (
            <div>
              <Badge cls="c-medium" icon={FlaskConical}>
                Advanced · off by default
              </Badge>
            </div>
          )}
          <p>{m.explanation}</p>
          {m.id === "standby_list" && <StandbyPanel />}
          {m.links.length > 0 && (
            <div className="link-row">
              {m.links.map((l) => (
                <button key={l.url} className="btn-link" onClick={() => api.openExternal(l.url).catch(() => undefined)}>
                  <ExternalLink size={13} /> {l.label}
                </button>
              ))}
            </div>
          )}
        </article>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ settings

interface SettingsProps {
  themePref: ThemePref;
  setThemePref: (t: ThemePref) => void;
  accentEnabled: boolean;
  setAccentEnabled: (v: boolean) => void;
  accent: string | null;
  restorePoint: boolean;
  setRestorePoint: (v: boolean) => void;
  onShowWelcome: () => void;
}

export function Settings({ themePref, setThemePref, accentEnabled, setAccentEnabled, accent, restorePoint, setRestorePoint, onShowWelcome }: SettingsProps) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const load = useCallback(() => api.appInfo().then(setInfo).catch(() => setInfo(null)), []);
  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="page">
      <section className="card">
        <div className="card-pad" style={{ paddingBottom: 4 }}>
          <h2 className="section-title">Appearance</h2>
        </div>
        <div className="setting-row">
          <div>
            <strong>Theme</strong>
            <p className="hint">System follows your Windows light or dark setting.</p>
          </div>
          <div className="tabs" role="group" aria-label="Theme">
            {(["system", "light", "dark"] as ThemePref[]).map((t) => (
              <button key={t} aria-pressed={themePref === t} onClick={() => setThemePref(t)}>
                {t[0].toUpperCase() + t.slice(1)}
              </button>
            ))}
          </div>
        </div>
        <div className="setting-row">
          <div>
            <strong>Use Windows accent colour</strong>
            <p className="hint">
              Replaces violet with your Windows accent colour
              {accent && (
                <>
                  {" "}
                  (<span style={{ display: "inline-block", width: 10, height: 10, borderRadius: 3, background: accent, verticalAlign: "middle" }} /> {accent})
                </>
              )}
              .
            </p>
          </div>
          <Switch checked={accentEnabled} onChange={setAccentEnabled} label="Use Windows accent colour" />
        </div>
      </section>

      <section className="card">
        <div className="card-pad" style={{ paddingBottom: 4 }}>
          <h2 className="section-title">
            <LifeBuoy size={16} color="var(--accent)" /> Changes
          </h2>
        </div>
        <div className="setting-row">
          <div>
            <strong>Create a restore point before the first change</strong>
            <p className="hint">
              Once per session, before FrameWise changes anything, Windows makes a System Restore point (Windows asks for
              permission). FrameWise's own Undo works either way.
            </p>
          </div>
          <Switch checked={restorePoint} onChange={setRestorePoint} label="Create a restore point before the first change" />
        </div>
        <div className="setting-row">
          <div>
            <strong>Permission prompts</strong>
            <p className="hint">
              FrameWise never runs as administrator. Settings that affect every user (graphics scheduling, Memory Integrity,
              all-users startup apps, Windows' own folders in Cleanup) are changed by a small helper that Windows asks you
              to allow, once per task. It only accepts FrameWise's fixed list of changes and closes when the task is done.
            </p>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-pad" style={{ paddingBottom: 4 }}>
          <h2 className="section-title">
            <LockKeyhole size={16} color="var(--accent)" /> Privacy
          </h2>
        </div>
        <div className="setting-row">
          <div>
            <strong>Everything stays on this PC</strong>
            <p className="hint">
              No account, no telemetry, no ads. Scans and settings are stored locally
              {info ? ` in ${info.data_dir}` : ""}. The network is used for two things: the ping test, which opens a connection
              to a public DNS server and sends no data, and the update check below.
            </p>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-pad" style={{ paddingBottom: 4 }}>
          <h2 className="section-title">Updates</h2>
        </div>
        <UpdatesSettings info={info} />
      </section>

      <section className="card">
        <div className="card-pad" style={{ paddingBottom: 4 }}>
          <h2 className="section-title">About</h2>
        </div>
        <div className="setting-row">
          <div>
            <strong>FrameWise {info?.version ?? ""}</strong>
            <p className="hint">
              Free, part of the PaperForge and Cutroom toolkit. {info && !info.db_ok && "The local database couldn't be opened, so scans aren't being saved."}
            </p>
          </div>
          <button className="btn btn-sm" onClick={onShowWelcome}>
            Show welcome screen
          </button>
        </div>
      </section>
    </div>
  );
}

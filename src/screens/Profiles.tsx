import { useCallback, useEffect, useMemo, useState } from "react";
import { Gamepad2, Pause, Pencil, Play, Plus, RefreshCw, Search, Trash2, TriangleAlert, X } from "lucide-react";
import { api, errorText } from "../api";
import { Modal } from "../components/Modal";
import { Badge, Switch, timeAgo } from "../components/ui";
import type { GameRow, GamesList, ProfileActions, RunningApp, SessionView, WatcherInfo } from "../types";

const EMPTY: ProfileActions = { power_plan: null, pause: [], close: [] };

function planName(list: GamesList | null, guid: string | null): string | null {
  if (!guid) return null;
  return list?.plans.find(([g]) => g === guid)?.[1] ?? "Custom plan";
}

function summary(list: GamesList | null, a: ProfileActions): string {
  const parts: string[] = [];
  const plan = planName(list, a.power_plan);
  if (plan) parts.push(plan);
  if (a.pause.length) parts.push(`pauses ${a.pause.map(niceExe).join(", ")}`);
  if (a.close.length) parts.push(`closes ${a.close.map(niceExe).join(", ")}`);
  return parts.join(" · ") || "No changes yet";
}

function niceExe(exe: string): string {
  return exe.replace(/\.exe$/i, "");
}

export function Profiles() {
  const [list, setList] = useState<GamesList | null>(null);
  const [watcher, setWatcher] = useState<WatcherInfo | null>(null);
  const [sessions, setSessions] = useState<SessionView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<GameRow | null>(null);
  const [adding, setAdding] = useState(false);
  const [portable, setPortable] = useState(false);

  useEffect(() => {
    api.appInfo().then((i) => setPortable(i.portable)).catch(() => {});
  }, []);

  const load = useCallback(async () => {
    try {
      const [l, w, s] = await Promise.all([api.gamesList(), api.watcherStatus(), api.profileSessions(10)]);
      setList(l);
      setWatcher(w);
      setSessions(s);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    load();
    // The watcher updates its status about once a minute; check now and then while this is open.
    const t = setInterval(() => {
      api.watcherStatus().then(setWatcher).catch(() => {});
      api.profileSessions(10).then(setSessions).catch(() => {});
    }, 5000);
    return () => clearInterval(t);
  }, [load]);

  const enable = async (on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setWatcher(await api.watcherEnable(on));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const pause = async (p: boolean) => {
    await api.watcherPause(p);
    setTimeout(() => api.watcherStatus().then(setWatcher), 400);
  };
  const toggle = async (row: GameRow, on: boolean) => {
    const actions = row.profile?.actions ?? EMPTY;
    if (on && !hasActions(actions)) {
      setEditing(row);
      return;
    }
    await api.profileSave(row.game, on, actions);
    load();
  };

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const all = (list?.games ?? []).filter((r) => !q || r.game.name.toLowerCase().includes(q));
    // Games with a profile first, then by name.
    return [...all].sort((a, b) => Number(!!b.profile?.enabled) - Number(!!a.profile?.enabled) || a.game.name.localeCompare(b.game.name));
  }, [list, query]);

  const st = watcher?.status;
  const on = watcher?.enabled ?? false;

  return (
    <div className="page">
      <div className="card card-pad upcoming-hero">
        <span className="icon">
          <Gamepad2 size={24} />
        </span>
        <div className="grow" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <h2 style={{ fontSize: 18 }}>Game profiles</h2>
          <p className="muted">
            Temporary changes for a game: a different power plan, background apps paused or closed. They apply when the game starts and are
            put back when it exits, even if the PC restarts mid-game. A small watcher does this so FrameWise itself can stay closed. It never
            touches the game.
          </p>
          {portable && (
            <p className="hint warn-text">
              This is the portable build, which never runs anything in the background, so profiles are off. Install FrameWise to use them.
            </p>
          )}
          <div className="watcher-row" hidden={portable}>
            <Switch checked={on} onChange={enable} label="Profiles on" />
            <strong>{on ? "Profiles on" : "Profiles off"}</strong>
            <span className="hint">
              {busy
                ? "…"
                : on && watcher?.running && st
                  ? `Watcher running · ${st.ram_mb.toFixed(1)} MB RAM · ${st.cpu_pct.toFixed(2)}% CPU (budget ${watcher.budget_ram_mb} MB, ${watcher.budget_cpu_pct}%) · starts with Windows`
                  : on && watcher?.running
                    ? "Watcher running · starts with Windows"
                    : on
                      ? "The watcher isn't running."
                      : "Turn on to start the watcher now and when you sign in."}
            </span>
            <span className="grow" />
            {on && watcher?.running && (
              <button className="btn btn-sm" onClick={() => pause(!watcher.paused)}>
                {watcher.paused ? <Play size={14} /> : <Pause size={14} />} {watcher.paused ? "Resume profiles" : "Pause profiles"}
              </button>
            )}
          </div>
          {on && !watcher?.running && !busy && (
            <p className="hint warn-text">
              The watcher stopped (it was quit from its tray icon, or crashed).{" "}
              <button className="btn-link" onClick={() => enable(true)}>
                Start it again
              </button>
            </p>
          )}
          {st?.active_game && (
            <Badge cls="c-good" icon={Play}>
              {`Profile on now: ${st.active_game}`}
            </Badge>
          )}
          {st?.over_budget && <p className="hint warn-text">The watcher is using more than its budget. Please report this.</p>}
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

      <section className="card" aria-label="Games">
        <div className="card-pad section-head" style={{ paddingBottom: 6 }}>
          <h2 className="section-title">Games</h2>
          <div className="head-actions">
            <label className="search">
              <Search size={14} />
              <input className="input" placeholder="Find a game" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Find a game" />
            </label>
            <button className="btn btn-sm" onClick={() => setAdding(true)}>
              <Plus size={14} /> Add a game
            </button>
            <button className="btn btn-sm btn-ghost" onClick={load} aria-label="Look for games again">
              <RefreshCw size={14} />
            </button>
          </div>
        </div>
        {!list ? (
          <div className="empty">
            <RefreshCw size={24} className="spin" />
            <p className="muted">Looking through Steam, Epic, Battle.net and the Xbox app…</p>
          </div>
        ) : rows.length === 0 ? (
          <div className="empty">
            <Gamepad2 size={24} />
            <p className="muted">{query ? "No game matches." : "No games found. Add one by its .exe."}</p>
          </div>
        ) : (
          <ul className="game-list">
            {rows.map((r) => (
              <li key={r.game.id}>
                <Switch checked={!!r.profile?.enabled} onChange={(v) => toggle(r, v)} label={`Profile for ${r.game.name}`} />
                <span className="grow">
                  <strong>{r.game.name}</strong>
                  <span className="hint">
                    {r.source_label} · {r.profile && hasActions(r.profile.actions) ? summary(list, r.profile.actions) : "No profile"}
                  </span>
                </span>
                <button className="btn btn-sm btn-ghost" onClick={() => setEditing(r)}>
                  <Pencil size={14} /> {r.profile && hasActions(r.profile.actions) ? "Edit" : "Set up"}
                </button>
                {r.game.source === "manual" && (
                  <button
                    className="btn btn-sm btn-ghost"
                    aria-label={`Remove ${r.game.name}`}
                    onClick={async () => {
                      await api.gameRemove(r.game.id);
                      load();
                    }}
                  >
                    <Trash2 size={14} />
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="card card-pad" aria-label="Recent profile sessions">
        <h2 className="section-title" style={{ marginBottom: 8 }}>
          Recent sessions
        </h2>
        {sessions.length === 0 ? (
          <p className="hint">Nothing yet. When a game with a profile runs, what changed and when it was put back shows here.</p>
        ) : (
          <ul className="session-list">
            {sessions.map((s) => (
              <li key={s.id}>
                <strong>{s.game_name}</strong>
                <span className="hint">
                  {timeAgo(s.started_at)}
                  {s.ended_at ? ` · ${Math.max(1, Math.round((s.ended_at - s.started_at) / 60))} min` : " · running now"}
                  {s.power_plan && ` · ${s.power_plan}`}
                  {s.paused.length > 0 && ` · paused ${s.paused.map(niceExe).join(", ")}`}
                  {s.closed.length > 0 && ` · closed ${s.closed.map(niceExe).join(", ")}`}
                  {s.not_closed.length > 0 && ` · ${s.not_closed.map(niceExe).join(", ")} stayed open`}
                </span>
                {s.restored ? <Badge cls="c-good">Put back</Badge> : <Badge cls="c-live">Active</Badge>}
              </li>
            ))}
          </ul>
        )}
      </section>

      {editing && list && (
        <ProfileEditor
          row={editing}
          list={list}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            load();
          }}
        />
      )}
      {adding && (
        <AddGame
          onClose={() => setAdding(false)}
          onAdded={(row) => {
            setAdding(false);
            load().then(() => setEditing(row));
          }}
        />
      )}
    </div>
  );
}

function hasActions(a: ProfileActions): boolean {
  return !!a.power_plan || a.pause.length > 0 || a.close.length > 0;
}

type Choice = "leave" | "pause" | "close";

function ProfileEditor({ row, list, onClose, onSaved }: { row: GameRow; list: GamesList; onClose: () => void; onSaved: () => void }) {
  const start = row.profile?.actions ?? EMPTY;
  const [plan, setPlan] = useState<string>(start.power_plan ?? "");
  const [choices, setChoices] = useState<Record<string, Choice>>(() => {
    const c: Record<string, Choice> = {};
    start.pause.forEach((e) => (c[e] = "pause"));
    start.close.forEach((e) => (c[e] = "close"));
    return c;
  });
  const [apps, setApps] = useState<RunningApp[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.runningApps().then(setApps).catch(() => setApps([]));
  }, []);

  // Running apps plus anything already in the profile that isn't running now.
  const shown: RunningApp[] = useMemo(() => {
    const out = [...(apps ?? [])];
    for (const exe of Object.keys(choices)) {
      if (!out.find((a) => a.exe === exe)) out.push({ exe, path: null, label: null, known_overlay: false });
    }
    return out;
  }, [apps, choices]);

  const save = async (enabled: boolean) => {
    const actions: ProfileActions = {
      power_plan: plan || null,
      pause: Object.entries(choices).filter(([, c]) => c === "pause").map(([e]) => e),
      close: Object.entries(choices).filter(([, c]) => c === "close").map(([e]) => e),
    };
    try {
      await api.profileSave(row.game, enabled && hasActions(actions), actions);
      onSaved();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <Modal
      title={`Profile for ${row.game.name}`}
      icon={<Gamepad2 size={20} color="var(--accent)" />}
      onClose={onClose}
      wide
      actions={
        <>
          <button className="btn" onClick={onClose} data-autofocus>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => save(true)}>
            Save and turn on
          </button>
        </>
      }
    >
      <p className="hint">
        Applied when {row.game.name} starts, put back when it exits.{" "}
        {row.game.install_dir ? `Any program in its folder counts as the game.` : row.game.exe ? `Matched by ${row.game.exe}.` : ""}
      </p>
      <label className="field">
        <span>Power plan while playing</span>
        <select className="select" value={plan} onChange={(e) => setPlan(e.target.value)}>
          <option value="">Don't change</option>
          {list.plans.map(([g, n]) => (
            <option key={g} value={g}>
              {n}
              {g === list.current_plan ? " (current)" : ""}
            </option>
          ))}
        </select>
      </label>
      <div className="field">
        <span>Background apps</span>
        <span className="hint">
          Pause stops an app using any CPU until the game exits (good for launchers and helpers; don't pause voice chat you use while
          playing). Close asks it to close and reopens it afterwards; apps that only minimise to the tray stay open. Windows, security
          software and anti-cheat are never listed.
        </span>
        {!apps ? (
          <p className="hint">Looking at what's running…</p>
        ) : (
          <ul className="app-choices">
            {shown.map((a) => (
              <li key={a.exe}>
                <span className="grow">
                  <strong>{a.label ?? niceExe(a.exe)}</strong>
                  <span className="hint">
                    {a.exe}
                    {a.known_overlay ? " · overlay or launcher" : ""}
                    {!a.path ? " · not running now" : ""}
                  </span>
                </span>
                <span className="pair" role="group" aria-label={`What to do with ${a.exe}`}>
                  {(["leave", "pause", "close"] as Choice[]).map((c) => (
                    <button
                      key={c}
                      className={`pill ${(choices[a.exe] ?? "leave") === c ? "on" : ""}`}
                      aria-pressed={(choices[a.exe] ?? "leave") === c}
                      onClick={() =>
                        setChoices((prev) => {
                          const n = { ...prev };
                          if (c === "leave") delete n[a.exe];
                          else n[a.exe] = c;
                          return n;
                        })
                      }
                    >
                      {{ leave: "Leave", pause: "Pause", close: "Close" }[c]}
                    </button>
                  ))}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      {error && <p className="error-text">{error}</p>}
    </Modal>
  );
}

function AddGame({ onClose, onAdded }: { onClose: () => void; onAdded: (row: GameRow) => void }) {
  const [path, setPath] = useState("");
  const [apps, setApps] = useState<RunningApp[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.runningApps().then((a) => setApps(a.filter((x) => !x.known_overlay && x.path))).catch(() => {});
  }, []);
  const add = async (p: string) => {
    setError(null);
    try {
      const game = await api.gameAdd(p);
      onAdded({ game, source_label: "Added by you", profile: null });
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <Modal
      title="Add a game"
      icon={<Plus size={20} color="var(--accent)" />}
      onClose={onClose}
      actions={
        <>
          <button className="btn" onClick={onClose} data-autofocus>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!path.trim()} onClick={() => add(path)}>
            Add
          </button>
        </>
      }
    >
      <label className="field">
        <span>The game's .exe</span>
        <input className="input" value={path} placeholder="C:\Games\MyGame\MyGame.exe" onChange={(e) => setPath(e.target.value)} />
        <span className="hint">Tip: right-click the game in File Explorer, choose "Copy as path", and paste it here.</span>
      </label>
      {apps.length > 0 && (
        <div className="field">
          <span>Or pick it while it's running</span>
          <ul className="app-choices">
            {apps.slice(0, 12).map((a) => (
              <li key={a.exe}>
                <span className="grow">
                  <strong>{niceExe(a.exe)}</strong>
                  <span className="hint" title={a.path ?? ""}>
                    {a.path}
                  </span>
                </span>
                <button className="btn btn-sm" onClick={() => add(a.path!)}>
                  Add
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {error && <p className="error-text">{error}</p>}
    </Modal>
  );
}

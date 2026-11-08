import { useCallback, useEffect, useState } from "react";
import { Download, RefreshCw, X } from "lucide-react";
import { api, errorText, inTauri } from "./api";
import { Switch } from "./components/ui";
import type { AppInfo } from "./types";

// Updates come from GitHub Releases through Tauri's updater: it checks on launch, verifies the
// update's signature against the public key built into FrameWise, and asks before installing.

const PREF = "framewise.autoUpdate";

function autoCheckOn(): boolean {
  try {
    return localStorage.getItem(PREF) !== "0";
  } catch {
    return true;
  }
}

interface Available {
  version: string;
  notes: string | null;
  install: (onProgress: (pct: number | null) => void) => Promise<void>;
}

/** Checks for an update. Resolves to null when there is none, or when it can't be checked here. */
async function checkForUpdate(): Promise<Available | null> {
  if (!inTauri) return null;
  const info: AppInfo = await api.appInfo();
  if (info.portable) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    notes: update.body ?? null,
    install: async (onProgress) => {
      const { relaunch } = await import("@tauri-apps/plugin-process");
      // The watcher holds framewise.exe open; stop it first (the app restarts it afterwards).
      await api.prepareUpdate();
      let total = 0;
      let done = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        if (e.event === "Progress") {
          done += e.data.chunkLength;
          onProgress(total ? Math.round((done / total) * 100) : null);
        }
      });
      await relaunch();
    },
  };
}

/** Banner shown at the top of the window when an update is available (checked once per launch). */
export function UpdateBanner() {
  const [update, setUpdate] = useState<Available | null>(null);
  const [hidden, setHidden] = useState(false);
  const [progress, setProgress] = useState<number | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!autoCheckOn()) return;
    // A failed check (offline, GitHub unreachable) is silent: Settings has "Check now".
    checkForUpdate().then(setUpdate).catch(() => {});
  }, []);

  if (!update || hidden) return null;
  const installing = progress !== undefined;
  return (
    <div className="update-banner" role="status">
      <Download size={16} />
      <span className="grow">
        <strong>FrameWise {update.version} is available.</strong>
        {error ? <span className="hint"> {error}</span> : installing ? <span className="hint"> Downloading{progress != null ? ` · ${progress}%` : "…"}</span> : null}
      </span>
      {!installing && (
        <>
          <button
            className="btn btn-sm btn-primary"
            onClick={async () => {
              setError(null);
              setProgress(null);
              try {
                await update.install(setProgress);
              } catch (e) {
                setProgress(undefined);
                setError(`The update didn't install: ${errorText(e)}`);
              }
            }}
          >
            Install and restart
          </button>
          <button className="btn btn-sm btn-ghost" onClick={() => setHidden(true)} aria-label="Not now">
            <X size={14} />
          </button>
        </>
      )}
    </div>
  );
}

/** The Updates section in Settings. */
export function UpdatesSettings({ info }: { info: AppInfo | null }) {
  const [auto, setAuto] = useState(autoCheckOn);
  const [state, setState] = useState<{ busy: boolean; text: string | null }>({ busy: false, text: null });

  const toggle = (v: boolean) => {
    setAuto(v);
    try {
      localStorage.setItem(PREF, v ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  const checkNow = useCallback(async () => {
    setState({ busy: true, text: null });
    try {
      const u = await checkForUpdate();
      setState({ busy: false, text: u ? `FrameWise ${u.version} is available. Restart FrameWise to see the install button.` : "FrameWise is up to date." });
    } catch (e) {
      setState({ busy: false, text: `Couldn't check for updates: ${errorText(e)}` });
    }
  }, []);

  if (info?.portable) {
    return (
      <div className="setting-row">
        <div>
          <strong>Portable build</strong>
          <p className="hint">The portable build doesn't update itself. Download the newest zip from the FrameWise releases page.</p>
        </div>
      </div>
    );
  }
  return (
    <>
      <div className="setting-row">
        <div>
          <strong>Check for updates when FrameWise starts</strong>
          <p className="hint">
            Asks GitHub Releases whether there's a newer version and sends nothing about this PC. Updates are signed and only install when
            you click Install.
          </p>
        </div>
        <Switch checked={auto} onChange={toggle} label="Check for updates when FrameWise starts" />
      </div>
      <div className="setting-row">
        <div>
          <strong>Check now</strong>
          {state.text && <p className="hint">{state.text}</p>}
        </div>
        <button className="btn btn-sm" onClick={checkNow} disabled={state.busy || !inTauri}>
          <RefreshCw size={14} className={state.busy ? "spin" : undefined} /> Check for updates
        </button>
      </div>
    </>
  );
}

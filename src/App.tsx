import { useCallback, useEffect, useState } from "react";
import {
  Activity,
  BookOpen,
  Gamepad2,
  History as HistoryIcon,
  House,
  Power,
  Settings as SettingsIcon,
  Trash2,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { api, errorText } from "./api";
import { FootprintIndicator } from "./components/Footprint";
import { Home } from "./screens/Home";
import { QuickWins } from "./screens/QuickWins";
import { MonitorScreen } from "./screens/Monitor";
import { Myths, Settings } from "./screens/Other";
import { Profiles } from "./screens/Profiles";
import { UpdateBanner } from "./updates";
import { CrashNotice } from "./components/Crash";
import { Cleanup } from "./screens/Cleanup";
import { History } from "./screens/History";
import { Welcome } from "./screens/Welcome";
import { useTheme, useWindowsAccent } from "./theme";
import type { Finding, ScanResult } from "./types";
import { FixProvider, useFixes, useRestorePointPref } from "./fixes";
import { LogoHorizontal } from "./components/Logo";

type Screen = "home" | "quickwins" | "profiles" | "monitor" | "cleanup" | "history" | "settings" | "myths";

const NAV: { id: Screen; label: string; icon: LucideIcon; soon?: boolean }[] = [
  { id: "home", label: "Home", icon: House },
  { id: "quickwins", label: "Quick wins", icon: Zap },
  { id: "profiles", label: "Game profiles", icon: Gamepad2 },
  { id: "monitor", label: "Monitor & benchmark", icon: Activity },
  { id: "cleanup", label: "Cleanup", icon: Trash2 },
  { id: "history", label: "History", icon: HistoryIcon },
];

const TITLES: Record<Screen, string> = {
  home: "Home",
  quickwins: "Quick wins",
  profiles: "Game profiles",
  monitor: "Monitor & benchmark",
  cleanup: "Cleanup",
  history: "History",
  settings: "Settings",
  myths: "What we don't do",
};

const WELCOME_KEY = "framewise.welcomeSeen";

function readFlag(key: string) {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, on: boolean) {
  try {
    if (on) localStorage.setItem(key, "1");
    else localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

export default function App() {
  const theme = useTheme();
  const [accent, setAccent] = useState<string | null>(null);
  const accentPref = useWindowsAccent(accent);

  const [screen, setScreen] = useState<Screen>("home");
  const [focusCheck, setFocusCheck] = useState<string | null>(null);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [welcomeSeen, setWelcomeSeen] = useState(() => readFlag(WELCOME_KEY));
  const [restorePoint, setRestorePoint] = useRestorePointPref();

  useEffect(() => {
    api
      .lastScan()
      .then((s) => setScan(s))
      .catch(() => undefined)
      .finally(() => setLoaded(true));
    api.systemAccent().then(setAccent).catch(() => undefined);
  }, []);

  const runScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      setScan(await api.runScan());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const recheck = useCallback(async (checkId: string) => {
    const r = await api.recheck(checkId);
    setScan((s) =>
      s ? { ...s, score: r.score, findings: s.findings.map((f) => (f.check_id === checkId ? r.finding : f)) } : s,
    );
  }, []);

  /** Merges rechecked findings (after a fix or undo) into the current scan. */
  const applyFindings = useCallback((findings: Finding[], score: number | null) => {
    if (findings.length === 0 && score === null) return;
    setScan((s) =>
      s
        ? {
            ...s,
            score: score ?? s.score,
            findings: s.findings.map((f) => findings.find((n) => n.check_id === f.check_id) ?? f),
          }
        : s,
    );
  }, []);

  const go = (s: Screen) => {
    setFocusCheck(null);
    setScreen(s);
  };

  const openCheck = (checkId: string) => {
    setFocusCheck(checkId || null);
    setScreen("quickwins");
  };

  const finishWelcome = (scanNow: boolean) => {
    setWelcomeSeen(true);
    writeFlag(WELCOME_KEY, true);
    if (scanNow) runScan();
  };

  const issueCount = scan?.findings.filter((f) => f.status === "issue" && f.action !== "choose").length ?? 0;
  const showWelcome = loaded && !welcomeSeen;

  return (
    <FixProvider restorePoint={restorePoint} onFindings={applyFindings} onOpenCleanup={() => go("cleanup")}>
    <div className="app">
      <nav className="sidebar" aria-label="Main">
        <div className="brand">
          <LogoHorizontal />
        </div>
        {NAV.map((n) => (
          <button key={n.id} className="nav-item" aria-current={screen === n.id ? "page" : undefined} onClick={() => go(n.id)}>
            <n.icon size={18} />
            {n.label}
            {n.id === "quickwins" && issueCount > 0 && <span className="nav-count num" aria-label={`${issueCount} issues`}>{issueCount}</span>}
            {n.soon && <span className="soon">Soon</span>}
          </button>
        ))}
        <div className="spacer" />
        <hr />
        <button className="nav-item" aria-current={screen === "myths" ? "page" : undefined} onClick={() => go("myths")}>
          <BookOpen size={18} /> What we don't do
        </button>
        <button className="nav-item" aria-current={screen === "settings" ? "page" : undefined} onClick={() => go("settings")}>
          <SettingsIcon size={18} /> Settings
        </button>
      </nav>

      <div className="main">
        <header className="header">
          <h1>{TITLES[screen]}</h1>
          <div className="grow" />
          <RestartBadge onClick={() => go("history")} />
          <FootprintIndicator />
        </header>
        <CrashNotice />
        <UpdateBanner />
        <main className="content" key={screen}>
          {screen === "home" && (
            <Home scan={scan} scanning={scanning} error={error} onScan={runScan} onOpenCheck={openCheck} onRecheck={recheck} restorePoint={restorePoint} onOpenBenchmark={() => go("monitor")} />
          )}
          {screen === "quickwins" && <QuickWins scan={scan} focusCheck={focusCheck} onScan={runScan} onRecheck={recheck} />}
          {screen === "profiles" && <Profiles />}
          {screen === "monitor" && <MonitorScreen />}
          {screen === "cleanup" && <Cleanup />}
          {screen === "history" && <History key={scan?.id ?? 0} />}
          {screen === "myths" && <Myths />}
          {screen === "settings" && (
            <Settings
              themePref={theme.pref}
              setThemePref={theme.setPref}
              accentEnabled={accentPref.enabled}
              setAccentEnabled={accentPref.setEnabled}
              accent={accent}
              restorePoint={restorePoint}
              setRestorePoint={setRestorePoint}
              onShowWelcome={() => {
                writeFlag(WELCOME_KEY, false);
                setWelcomeSeen(false);
              }}
            />
          )}
        </main>
      </div>

      {showWelcome && <Welcome scanning={scanning} onStart={() => finishWelcome(true)} onSkip={() => finishWelcome(false)} />}
    </div>
    </FixProvider>
  );
}

function RestartBadge({ onClick }: { onClick: () => void }) {
  const { restartPending } = useFixes();
  if (restartPending.length === 0) return null;
  const n = restartPending.length;
  return (
    <button
      className="footprint-btn restart-btn"
      onClick={onClick}
      aria-label={`Restart Windows to finish: ${restartPending.map((c) => c.title).join(", ")}`}
      title={restartPending.map((c) => c.title).join(", ")}
    >
      <Power size={14} />
      Restart to finish {n === 1 ? "1 change" : `${n} changes`}
    </button>
  );
}

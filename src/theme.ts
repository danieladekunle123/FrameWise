import { useEffect, useState } from "react";
import { inTauri } from "./api";

export type ThemePref = "system" | "light" | "dark";
const KEY = "framewise.theme";
const ACCENT_KEY = "framewise.useWindowsAccent";

function read<T extends string>(key: string, fallback: T): T {
  try {
    return (localStorage.getItem(key) as T) ?? fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: preference lasts for this session only */
  }
}

const media = () => window.matchMedia("(prefers-color-scheme: dark)");

export function resolve(pref: ThemePref): "light" | "dark" {
  if (pref === "system") return media().matches ? "dark" : "light";
  return pref;
}

async function syncTitleBar(pref: ThemePref) {
  if (!inTauri) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setTheme(pref === "system" ? null : pref);
  } catch {
    /* title bar keeps the system theme */
  }
}

/** Light, Dark or System theme; switches instantly and keeps the title bar in step. */
export function useTheme() {
  const [pref, setPref] = useState<ThemePref>(() => read<ThemePref>(KEY, "system"));
  const [resolved, setResolved] = useState(() => resolve(pref));

  useEffect(() => {
    const apply = () => {
      const r = resolve(pref);
      setResolved(r);
      document.documentElement.dataset.theme = r;
    };
    apply();
    syncTitleBar(pref);
    write(KEY, pref);
    if (pref !== "system") return;
    const m = media();
    m.addEventListener("change", apply);
    return () => m.removeEventListener("change", apply);
  }, [pref]);

  return { pref, setPref, resolved };
}

/** Optional: replace the violet accent with the user's Windows accent colour. */
export function useWindowsAccent(accent: string | null) {
  const [enabled, setEnabled] = useState(() => read<string>(ACCENT_KEY, "false") === "true");
  useEffect(() => {
    write(ACCENT_KEY, String(enabled));
    const root = document.documentElement;
    if (enabled && accent) {
      root.style.setProperty("--accent", accent);
      root.style.setProperty("--on-accent", readableOn(accent));
    } else {
      root.style.removeProperty("--accent");
      root.style.removeProperty("--on-accent");
    }
  }, [enabled, accent]);
  return { enabled, setEnabled };
}

/** Picks white or near-black text for a background colour, whichever has more contrast. */
export function readableOn(hex: string): string {
  const n = parseInt(hex.replace("#", ""), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const lum = 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
  const withWhite = 1.05 / (lum + 0.05);
  const withDark = (lum + 0.05) / 0.0548; // #0E0F14
  return withWhite >= withDark ? "#FFFFFF" : "#0E0F14";
}

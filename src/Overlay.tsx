import { useEffect, useState } from "react";
import { onEvent } from "./api";
import type { MonitorTick } from "./types";

/**
 * The in-game overlay (window "overlay", loaded at index.html#overlay): a dark translucent panel
 * with cyan numbers, whatever the app theme, so it reads over any game. It never takes input:
 * the window ignores the mouse and never takes focus.
 */
export function Overlay() {
  const [t, setT] = useState<MonitorTick | null>(null);
  useEffect(() => {
    document.documentElement.classList.add("overlay-doc");
    const offs = [onEvent<MonitorTick>("monitor-tick", setT), onEvent("monitor-stopped", () => setT(null))];
    return () => offs.forEach((o) => o());
  }, []);
  const s = t?.sensors;
  const n = (v: number | null | undefined, d = 0) => (v == null ? "–" : v.toFixed(d));
  return (
    <div className="overlay-panel">
      <div className="ov-fps">
        <span className="ov-big">{n(t?.fps)}</span>
        <span className="ov-unit">FPS</span>
      </div>
      <div className="ov-row">
        <span>1% low</span>
        <b>{n(t?.low_1)}</b>
        <span>Frame</span>
        <b>{n(t?.frame_ms, 1)} ms</b>
      </div>
      <div className="ov-row">
        <span>GPU</span>
        <b>
          {n(s?.gpu_pct)}%{s?.gpu_temp_c != null ? ` · ${n(s.gpu_temp_c)}°C` : ""}
        </b>
        <span>CPU</span>
        <b>{n(s?.cpu_pct)}%</b>
      </div>
      {(t?.throttling ?? []).length > 0 && <div className="ov-warn">Throttling</div>}
      {!t && <div className="ov-app">Monitor off</div>}
      {t?.app && <div className="ov-app">{t.app}</div>}
    </div>
  );
}

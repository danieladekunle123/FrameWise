import { Feather, RefreshCw, ShieldCheck, Undo2 } from "lucide-react";
import { LogoHorizontal } from "../components/Logo";

export function Welcome({ onStart, onSkip, scanning }: { onStart: () => void; onSkip: () => void; scanning: boolean }) {
  return (
    <div className="welcome" role="dialog" aria-label="Welcome to FrameWise">
      <div className="welcome-inner">
        <LogoHorizontal className="lockup" />
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <h1>The honest gaming optimiser</h1>
          <p className="muted" style={{ fontSize: 15, maxWidth: 560 }}>
            It finds the settings and hardware issues that really cost you FPS, fixes only what measurably helps, and
            shows the evidence.
          </p>
        </div>
        <div className="promises">
          <div className="card promise c-ok">
            <span className="icon">
              <ShieldCheck size={18} />
            </span>
            <h3>No snake oil</h3>
            <p>No RAM cleaners, registry cleaners or placebo tweaks. Only changes that measurably help.</p>
          </div>
          <div className="card promise c-accent">
            <span className="icon">
              <Undo2 size={18} />
            </span>
            <h3>Everything reversible</h3>
            <p>Every change is logged with its old value and an Undo. Security trade-offs are always your call.</p>
          </div>
          <div className="card promise c-live">
            <span className="icon">
              <Feather size={18} />
            </span>
            <h3>Near-zero footprint</h3>
            <p>FrameWise shows its own RAM and CPU use at all times, and closes fully when you close it.</p>
          </div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10 }}>
          <button className="btn btn-primary" style={{ height: 42, padding: "0 22px", fontSize: 15 }} onClick={onStart} disabled={scanning}>
            <RefreshCw size={16} className={scanning ? "spin" : undefined} />
            {scanning ? "Scanning…" : "Run my first scan"}
          </button>
          <p className="hint">Read-only, no admin rights needed, takes a few seconds.</p>
          <button className="btn-link" onClick={onSkip}>
            Skip for now
          </button>
        </div>
      </div>
    </div>
  );
}

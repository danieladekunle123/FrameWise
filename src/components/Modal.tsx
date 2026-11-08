import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

interface ModalProps {
  title: string;
  icon?: ReactNode;
  children: ReactNode;
  actions: ReactNode;
  onClose?: () => void;
  wide?: boolean;
}

/** A dialog with a focus trap entry point and Escape to close (when closable). */
export function Modal({ title, icon, children, actions, onClose, wide }: ModalProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("[data-autofocus], .modal-actions .btn-primary, .modal-actions .btn")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus?.();
    };
  }, [onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className={`modal card ${wide ? "wide" : ""}`} role="dialog" aria-modal="true" aria-labelledby="modal-title" ref={ref}>
        <div className="modal-head">
          {icon}
          <h2 id="modal-title">{title}</h2>
          {onClose && (
            <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close">
              <X size={16} />
            </button>
          )}
        </div>
        <div className="modal-body">{children}</div>
        <div className="modal-actions">{actions}</div>
      </div>
    </div>
  );
}

/** Full-window progress while a change runs (it may be waiting on a UAC prompt). */
export function ProgressOverlay({ text }: { text: string }) {
  return (
    <div className="modal-backdrop progress-overlay" role="status" aria-live="polite">
      <div className="modal card progress-card">
        <span className="spinner" aria-hidden="true" />
        <p>{text}</p>
      </div>
    </div>
  );
}

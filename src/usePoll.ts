import { useEffect, useState } from "react";

/**
 * Calls `fn` now and every `ms` while the window is visible. Polling stops when the window is
 * hidden or minimised, so FrameWise costs nothing when you're not looking at it.
 */
export function usePoll<T>(fn: () => Promise<T>, ms: number): T | undefined {
  const [value, setValue] = useState<T>();
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      if (document.visibilityState === "visible") {
        try {
          const v = await fn();
          if (alive) setValue(v);
        } catch {
          /* keep the last value */
        }
      }
      if (alive) timer = setTimeout(tick, ms);
    };
    tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [fn, ms]);
  return value;
}

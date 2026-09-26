import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "./api";
import type { Snapshot } from "./types";

/** Schedule from completion so a slow command can never overlap the next poll. */
export function useSnapshot() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const refreshRef = useRef<() => void>(() => {});

  useEffect(() => {
    let stopped = false;
    let running = false;
    let refreshPending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const refresh = async () => {
      if (stopped) return;
      if (running) { refreshPending = true; return; }
      clearTimeout(timer);
      running = true;
      try {
        const next = await api.getSnapshot();
        if (!stopped) { setSnapshot(next); setError(null); }
      } catch (cause) {
        if (!stopped) setError(errorMessage(cause));
      } finally {
        running = false;
        if (!stopped) {
          setLoading(false);
          if (refreshPending) { refreshPending = false; void refresh(); }
          else timer = setTimeout(() => void refresh(), 500);
        }
      }
    };

    refreshRef.current = () => { void refresh(); };
    void refresh();
    return () => { stopped = true; clearTimeout(timer); refreshRef.current = () => {}; };
  }, []);

  const refresh = useCallback(() => refreshRef.current(), []);
  return { snapshot, error, loading, refresh };
}

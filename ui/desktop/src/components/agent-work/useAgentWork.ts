import { useCallback, useEffect, useRef, useState } from 'react';
import {
  foldDesk,
  type AgentWorkRead,
  type AgentWorkRosterRow,
  type DeskModel,
} from './agentWorkModel';

/** The roster: every registered agent dir with its manifest, state and liveness. */
export function useAgentRoster(intervalMs = 5_000) {
  const [rows, setRows] = useState<AgentWorkRosterRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const refresh = useCallback(async () => {
    try {
      const r = await window.electron.agentWorkList();
      setRows(r);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoaded(true);
    }
  }, []);
  useEffect(() => {
    refresh();
    const id = setInterval(refresh, intervalMs);
    return () => clearInterval(id);
  }, [refresh, intervalMs]);
  return { rows, loaded, error, refresh };
}

/**
 * One desk, polled: fast while its engine is live (the lanes' digests rewrite ~2.5×/s), slow when
 * it is stopped. `now` ticks every second so the countdown and the phase clock move between polls.
 * `viewTick` is the tick the URL opened; the model's lanes and phases follow it.
 */
export function useDesk(dir: string | null, viewTick: number | null = null) {
  const [read, setRead] = useState<AgentWorkRead | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const inflight = useRef(new Set<string>());
  const dirRef = useRef(dir);
  dirRef.current = dir;

  const refresh = useCallback(async () => {
    const d = dirRef.current;
    if (!d || inflight.current.has(d)) return;
    inflight.current.add(d);
    try {
      const r = await window.electron.agentWorkRead(d);
      if (dirRef.current === d) {
        setRead(r);
        setError(null);
      }
    } catch (e) {
      if (dirRef.current === d) setError(e instanceof Error ? e.message : String(e));
    } finally {
      inflight.current.delete(d);
    }
  }, []);

  useEffect(() => {
    setRead(null);
    setError(null);
    if (!dir) return;
    refresh();
  }, [dir, refresh]);

  const live = read ? foldDesk(read, now)?.liveness === 'running' : false;
  useEffect(() => {
    if (!dir) return;
    const id = setInterval(refresh, live ? 1_500 : 5_000);
    return () => clearInterval(id);
  }, [dir, live, refresh]);

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(id);
  }, []);

  const model: DeskModel | null = read ? foldDesk(read, now, viewTick) : null;
  return { read, model, now, error, refresh };
}

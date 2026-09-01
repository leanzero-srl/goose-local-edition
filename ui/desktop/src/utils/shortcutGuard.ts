// While a run is live, an accidental key chord must not spawn a window, close the window that holds
// the run, quit the app, or navigate that window away — Cmd+Shift+N once opened a second window on a
// live run. Only the ACCELERATOR is refused: a mouse click on the same menu item is the user really
// meaning it, which is why every refusal message ends in "use the menu".
//
// TWO kinds of live run, ONE guard. A BENCHMARK run is main-owned (`activeBenchRun`), so main knows it
// is live without help. A SESSION-driven run (`goose swarm run` under the swarm provider) is a child of
// the window's goose serve lease: closing the last window on that lease releases it, and the lease's
// cleanup signals goosed's whole process group — the run dies with the window (measured: Cmd+W on a
// session window killed a live build). Main learns such a run is live only through the renderer's own
// read-swarm-run poll, which stamps the heartbeat it read into a per-run cache; see
// `isSwarmRunStampAlive` for what that dependency means.

import { engineLiveness } from '../components/swarm/swarmRunLiveness';

export type GuardedShortcutAction = 'spawn' | 'close' | 'quit' | 'navigate' | 'reload';

// 'spawn' and 'quit' touch the run from any window (a second backend, an orphaned run); 'close',
// 'navigate' and 'reload' only matter on the window that shows the benchmark view.
const benchmarkWindowOnly: ReadonlySet<GuardedShortcutAction> = new Set([
  'close',
  'navigate',
  'reload',
]);

// Of the window-scoped actions, the one that KILLS a session-driven run. navigate and reload leave the
// window — and so the lease and the engine under it — standing; a reload re-folds the panel from the
// durable run log. close releases the lease.
const liveRunWindowOnly: ReadonlySet<GuardedShortcutAction> = new Set(['close']);

export type ShortcutGuardInput = {
  action: GuardedShortcutAction;
  benchmarkRunning: boolean;
  triggeredByAccelerator: boolean;
  onBenchmarkView: boolean;
  /** SOME renderer holds a live swarm-run subscription (its cached heartbeat stamp is fresh). spawn and
   *  quit refuse on this from any window: a second window is a second backend, and quit cleans every
   *  lease. Absent means "not known", which reads as false — the guard fails OPEN, never closed. */
  sessionRunLive?: boolean;
  /** The FOCUSED window's own renderer holds such a subscription — the window whose close would take
   *  the run with it. */
  windowHoldsLiveRun?: boolean;
};

export function shouldRefuseShortcut({
  action,
  benchmarkRunning,
  triggeredByAccelerator,
  onBenchmarkView,
  sessionRunLive = false,
  windowHoldsLiveRun = false,
}: ShortcutGuardInput): boolean {
  if (!triggeredByAccelerator) return false;
  if (!benchmarkRunning && !sessionRunLive) return false;
  if (!benchmarkWindowOnly.has(action)) return true;
  if (benchmarkRunning && onBenchmarkView) return true;
  return liveRunWindowOnly.has(action) && windowHoldsLiveRun;
}

/** Which live run a refusal is protecting — the renderer words its notice from this. */
export type ShortcutRefusalReason = 'benchmark' | 'session-run';

export function shortcutRefusalReason(benchmarkRunning: boolean): ShortcutRefusalReason {
  return benchmarkRunning ? 'benchmark' : 'session-run';
}

/** What main caches per run directory from the last read-swarm-run it answered: the heartbeat stamp
 *  the renderer was shown, verbatim. */
export type SwarmRunStamp = { heartbeat: number | null; heartbeatExited: boolean };

/**
 * Is the engine behind this cached stamp alive — by the SAME window the liveness banner uses
 * (`SWARM_HEARTBEAT_STALE_MS` through `engineLiveness`; no seconds literal of this guard's own).
 *
 * THE POLL DEPENDENCY, named: main writes no stamp on its own — the cache is armed only by a renderer's
 * read-swarm-run poll, and a stamp is only as new as the last poll that wrote it. That is the guard's
 * decay, not a defect: a renderer that stops polling (the panel unmounted, the run switched) stops
 * refreshing the stamp, the stamp ages past the window, and the run reads dead here within one window —
 * a chord is refused only while someone is actually watching a live run. `EXITED:` reads dead at once.
 */
export function isSwarmRunStampAlive(stamp: SwarmRunStamp | undefined, now = Date.now()): boolean {
  if (!stamp) return false;
  return engineLiveness(stamp, now).state === 'alive';
}

export const BENCHMARK_ROUTE_HASH = '#/benchmark';

export function isBenchmarkViewUrl(url: string): boolean {
  const hash = url.slice(url.indexOf('#'));
  return (
    url.includes('#') &&
    (hash === BENCHMARK_ROUTE_HASH ||
      hash.startsWith(`${BENCHMARK_ROUTE_HASH}?`) ||
      hash.startsWith(`${BENCHMARK_ROUTE_HASH}/`))
  );
}

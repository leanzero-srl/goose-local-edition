/**
 * Is the ENGINE alive, and should the run get its own pane?
 *
 * These are two different questions and they used to be one. Workspace visibility was folded together with
 * a 45s heartbeat window AND a 300s activity window, so a slow local model — the only kind this fork runs —
 * made the entire run pane disappear mid-build. Every wall-clock, turn and volume cap was deleted from the
 * engine precisely so a slow model is never cut; a UI timer that hides the run is the same cut wearing a
 * different hat.
 *
 * So: VISIBILITY asks only whether a local run is present and underway. LIVENESS is a separate, non-terminal
 * WARNING, and it reads engine truth rather than a file's mtime — `.swarm/heartbeat` is rewritten every 5s
 * and, on the guard's Drop, stamped `EXITED:<rfc3339>`. Those are three distinguishable states:
 *   - fresh timestamp  → the engine is alive (even mid-long-tool-call, when task files are quiet for minutes)
 *   - `EXITED:`        → the engine returned early and tore itself down; Drop ran
 *   - frozen timestamp → the process was hard-killed (SIGKILL); Drop never ran
 * There is NO activity-mtime fallback: a quiet digest is what a slow model looks like, not what a dead one
 * looks like.
 */

/** The heartbeat ticks every 5s, so ~9 missed ticks is a dead engine, not a slow one. */
export const SWARM_HEARTBEAT_STALE_MS = 45_000;

export type SwarmRunLiveness = {
  present: boolean;
  inProgress: boolean;
  finished: boolean;
  /** Epoch ms of the stamp INSIDE .swarm/heartbeat (null when the run predates heartbeats). */
  heartbeat: number | null;
  /** The heartbeat file says `EXITED:` — the engine exited itself rather than being killed. */
  heartbeatExited: boolean;
};

export type EngineLiveness =
  | { state: 'alive' }
  | { state: 'unknown' }
  | { state: 'exited'; at: number | null }
  | { state: 'silent'; since: number };

/** What the heartbeat file says about the engine right now. Never terminal — the caller renders a banner. */
export function engineLiveness(
  run: Pick<SwarmRunLiveness, 'heartbeat' | 'heartbeatExited'>,
  now = Date.now()
): EngineLiveness {
  if (run.heartbeatExited) return { state: 'exited', at: run.heartbeat };
  if (run.heartbeat == null) return { state: 'unknown' };
  const since = now - run.heartbeat;
  return since > SWARM_HEARTBEAT_STALE_MS ? { state: 'silent', since } : { state: 'alive' };
}

/** True when the engine is provably not ticking. Used for the warning banner and for marking an in-flight
 *  lane 'interrupted' — never for deciding that a run is over. */
export function isEngineSilent(
  run: Pick<SwarmRunLiveness, 'heartbeat' | 'heartbeatExited'>,
  now = Date.now()
): boolean {
  const liveness = engineLiveness(run, now);
  return liveness.state === 'exited' || liveness.state === 'silent';
}

/** Does this session get the split conversation/run workspace? Presence and progress only — no timers. */
export function shouldSplitSwarmWorkspace({
  isLocal,
  run,
}: {
  isLocal: boolean;
  run: Pick<SwarmRunLiveness, 'present' | 'inProgress' | 'finished'>;
}): boolean {
  return isLocal && run.present && run.inProgress && !run.finished;
}

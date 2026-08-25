export const SWARM_ACTIVITY_STALE_MS = 300_000;
export const SWARM_HEARTBEAT_STALE_MS = 45_000;

export type SwarmRunLiveness = {
  present: boolean;
  inProgress: boolean;
  finished: boolean;
  heartbeat: number | null;
  mtime: number | null;
  clarify: { pending: boolean } | null;
};

export function isSwarmRunStale(
  run: Pick<SwarmRunLiveness, 'heartbeat' | 'mtime'>,
  now = Date.now()
): boolean {
  return run.heartbeat != null
    ? now - run.heartbeat > SWARM_HEARTBEAT_STALE_MS
    : run.mtime != null && now - run.mtime > SWARM_ACTIVITY_STALE_MS;
}

export function isSwarmRunTerminal(run: SwarmRunLiveness, now = Date.now()): boolean {
  return !run.clarify?.pending && (run.finished || isSwarmRunStale(run, now));
}

export function shouldSplitSwarmWorkspace({
  isLocal,
  run,
  now = Date.now(),
}: {
  isLocal: boolean;
  run: SwarmRunLiveness;
  now?: number;
}): boolean {
  return isLocal && run.present && run.inProgress && !isSwarmRunTerminal(run, now);
}

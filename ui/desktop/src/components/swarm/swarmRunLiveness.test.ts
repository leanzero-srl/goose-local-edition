import { describe, expect, it } from 'vitest';
import {
  SWARM_HEARTBEAT_STALE_MS,
  engineLiveness,
  isEngineSilent,
  shouldSplitSwarmWorkspace,
  type SwarmRunLiveness,
} from './swarmRunLiveness';

const NOW = 2_000_000_000_000;
const liveRun: SwarmRunLiveness = {
  present: true,
  inProgress: true,
  finished: false,
  heartbeat: NOW,
  heartbeatExited: false,
};

describe('engine liveness reads the heartbeat FILE, not activity mtimes', () => {
  it('separates alive, self-exited, hard-killed and pre-heartbeat runs', () => {
    expect(engineLiveness(liveRun, NOW)).toEqual({ state: 'alive' });
    expect(engineLiveness({ heartbeat: NOW - 1_000, heartbeatExited: true }, NOW)).toEqual({
      state: 'exited',
      at: NOW - 1_000,
    });
    const silent = engineLiveness(
      { heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1, heartbeatExited: false },
      NOW
    );
    expect(silent.state).toBe('silent');
    expect(engineLiveness({ heartbeat: null, heartbeatExited: false }, NOW)).toEqual({
      state: 'unknown',
    });
  });

  // A run with no heartbeat file predates the instrument. Guessing from a quiet activity file is exactly
  // what mislabelled a slow local model as dead, so 'unknown' must never read as silent.
  it('never calls a pre-heartbeat run silent', () => {
    expect(isEngineSilent({ heartbeat: null, heartbeatExited: false }, NOW)).toBe(false);
  });
});

describe('shouldSplitSwarmWorkspace', () => {
  it('splits on presence and progress alone — no timer may hide a live run', () => {
    expect(shouldSplitSwarmWorkspace({ isLocal: true, run: liveRun })).toBe(true);
    expect(shouldSplitSwarmWorkspace({ isLocal: false, run: liveRun })).toBe(false);
    expect(
      shouldSplitSwarmWorkspace({
        isLocal: true,
        run: { ...liveRun, inProgress: false, finished: true },
      })
    ).toBe(false);
    expect(shouldSplitSwarmWorkspace({ isLocal: true, run: { ...liveRun, present: false } })).toBe(
      false
    );
  });

  // THE BUG THIS PINS: staleness used to be folded into visibility, so a local model that went quiet for
  // 45s took the whole run pane with it. The engine had every cap removed for exactly this reason — and
  // shouldSplitSwarmWorkspace no longer even ACCEPTS a heartbeat, so no timer can reach visibility again.
  it('keeps the pane up while the engine is silent — staleness is a warning, never a cut', () => {
    const quiet = { ...liveRun, heartbeat: NOW - 10 * SWARM_HEARTBEAT_STALE_MS };
    expect(isEngineSilent(quiet, NOW)).toBe(true);
    expect(shouldSplitSwarmWorkspace({ isLocal: true, run: quiet })).toBe(true);
  });
});

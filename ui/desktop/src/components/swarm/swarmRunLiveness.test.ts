import { describe, expect, it } from 'vitest';
import {
  SWARM_ACTIVITY_STALE_MS,
  SWARM_HEARTBEAT_STALE_MS,
  isSwarmRunStale,
  isSwarmRunTerminal,
  type SwarmRunLiveness,
} from './swarmRunLiveness';

const NOW = 2_000_000_000_000;
const liveRun: SwarmRunLiveness = {
  present: true,
  inProgress: true,
  finished: false,
  heartbeat: NOW,
  mtime: NOW,
  clarify: null,
};

describe('swarm run liveness', () => {
  it('uses the heartbeat when available and mtime only for legacy runs', () => {
    expect(isSwarmRunStale({ heartbeat: NOW, mtime: NOW - SWARM_ACTIVITY_STALE_MS - 1 }, NOW)).toBe(
      false
    );
    expect(
      isSwarmRunStale({ heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1, mtime: NOW }, NOW)
    ).toBe(true);
    expect(
      isSwarmRunStale({ heartbeat: null, mtime: NOW - SWARM_ACTIVITY_STALE_MS - 1 }, NOW)
    ).toBe(true);
  });

  it('shares clean-finish, crash, and clarify-hold terminal truth', () => {
    expect(isSwarmRunTerminal({ ...liveRun, finished: true }, NOW)).toBe(true);
    expect(
      isSwarmRunTerminal({ ...liveRun, heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1 }, NOW)
    ).toBe(true);
    expect(
      isSwarmRunTerminal(
        {
          ...liveRun,
          heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1,
          clarify: { pending: true },
        },
        NOW
      )
    ).toBe(false);
  });
});

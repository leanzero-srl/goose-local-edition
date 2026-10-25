import { describe, expect, it } from 'vitest';
import { LINK_POLL_MS } from './useMacs';

// D2 (3.0.130): a dropped peer must read Offline on screen within 3.0 s. The engine's worst
// case is 2.3 s (PEER_LIVENESS_INTERVAL 500 ms + 2 x PEER_LIVENESS_DIAL_TIMEOUT 900 ms in
// crates/leanzero-link/src/state.rs), so the roster poll may add at most 500 ms (+ IPC).
const ENGINE_WORST_MS = 2300;
const SCREEN_BUDGET_MS = 3000;
const IPC_SLACK_MS = 100;

describe('LINK_POLL_MS', () => {
  it('keeps a dropped peer Offline on screen within the 3.0 s budget', () => {
    expect(LINK_POLL_MS).toBeLessThanOrEqual(500);
    expect(ENGINE_WORST_MS + LINK_POLL_MS + IPC_SLACK_MS).toBeLessThanOrEqual(SCREEN_BUDGET_MS);
  });
});

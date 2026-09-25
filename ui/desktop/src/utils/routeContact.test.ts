import { describe, expect, it } from 'vitest';
import { routeContactLost, type MainRead } from './routeContact';

const READY = { state: 'ready' };
const MARKED = {
  state: 'reconnecting',
  lastError:
    "Work's Mac Studio does not answer over LeanZero Link right now: the LeanZero Link mesh cannot reach it (connection refused)",
};
const main = (mode: string, statusDetail: string | null = null): MainRead => ({
  engine: 'remote',
  mode,
  statusDetail,
});

describe('routeContactLost — main’s read of the route alone decides "back" (Q-64)', () => {
  it('walks the 3.0.39 Link kill: one outage reads as one', () => {
    // 178.9 s: main's relay read times out.
    expect(
      routeContactLost(READY, null, main('reconnecting', 'timeout: no answer within 1500 ms'))
    ).toEqual({ why: 'timeout: no answer within 1500 ms' });
    // 182.2 s: main reads the Studio again — back.
    expect(routeContactLost(READY, null, main('running'))).toBeNull();
    // 184.4–187.5 s: the registry's Offline mark reaches the route while main reads `running` —
    // it re-raised the bar for 4.3 s; it no longer can.
    expect(routeContactLost(MARKED, null, main('running'))).toBeNull();
    // A renderer read of the route failing then is no drop either.
    expect(routeContactLost(READY, 'remoteSingleStatus: no answer', main('running'))).toBeNull();
  });

  it('a GENUINE second drop still shows — main’s next read fails', () => {
    expect(routeContactLost(MARKED, null, main('reconnecting', 'timeout: no answer'))).toEqual({
      why: 'timeout: no answer',
    });
  });

  it('the Mac’s own word that it is leaving is never stale — it shows even over a running read', () => {
    const leaving = {
      state: 'reconnecting',
      lastError:
        "Work's Mac Studio does not answer over LeanZero Link right now: Work's Mac Studio is restarting goose",
    };
    expect(routeContactLost(leaving, null, main('running'))).toEqual({ why: leaving.lastError });
  });

  it('with no read of the route by main, the route’s word and the renderer’s read decide', () => {
    expect(routeContactLost(MARKED, null, null)).toEqual({ why: MARKED.lastError });
    expect(routeContactLost(READY, 'read failed', null)).toEqual({ why: 'read failed' });
    expect(
      routeContactLost(MARKED, null, { engine: 'single', mode: 'off', statusDetail: null })
    ).toEqual({ why: MARKED.lastError });
    expect(routeContactLost(READY, null, null)).toBeNull();
  });
});

import { describe, expect, it } from 'vitest';
import {
  GONE_PAST_LONGEST_COMEBACK,
  RELAUNCH_IN_POLLS,
  expectedComebackMs,
  routeContactLost,
  routePeerGone,
  waitedPastComebacks,
  type MainRead,
  type RouteContact,
} from './routeContact';

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

describe('routePeerGone — away is the Mac’s own word, or a silence well past its expected comeback (Q-111)', () => {
  const SEC = 1000;
  const POLL = 2 * SEC;
  const LOST_AT = Date.UTC(2026, 8, 25, 23, 14);
  const contact = (over: Partial<RouteContact>): RouteContact => ({
    lostSinceMs: null,
    lostForMs: null,
    longestComebackMs: null,
    comebacks: 0,
    saidQuit: false,
    pollMs: POLL,
    ...over,
  });
  const lost = (lostForMs: number, over: Partial<RouteContact> = {}) =>
    contact({ lostSinceMs: LOST_AT, lostForMs, ...over });

  it('the threshold is a ratio of the route’s own expected comeback, never a typed number of seconds', () => {
    expect(GONE_PAST_LONGEST_COMEBACK).toBe(3);
    // Nothing measured yet: a relaunch's worth of the route's own poll — 12.5 × 2 s = 25 s.
    expect(RELAUNCH_IN_POLLS * POLL).toBe(25 * SEC);
    expect(expectedComebackMs({ longestComebackMs: null, pollMs: POLL })).toBe(25 * SEC);
    // A measured comeback longer than that (a 90 s mount) raises it; a 2 s blip never lowers it.
    expect(expectedComebackMs({ longestComebackMs: 90 * SEC, pollMs: POLL })).toBe(90 * SEC);
    expect(expectedComebackMs({ longestComebackMs: 2 * SEC, pollMs: POLL })).toBe(25 * SEC);
    // It moves with the poll the route is read at: a 1 s poll expects 12.5 s.
    expect(expectedComebackMs({ longestComebackMs: null, pollMs: SEC })).toBe(12.5 * SEC);
    expect(waitedPastComebacks(75 * SEC, { longestComebackMs: null, pollMs: POLL })).toBe(false);
    expect(waitedPastComebacks(75 * SEC + 1, { longestComebackMs: null, pollMs: POLL })).toBe(true);
  });

  it('the screenshot on a FRESH launch — no comeback measured, no quit notice: away at 76 s, not reconnecting for hours', () => {
    expect(routePeerGone(lost(60 * SEC), null)).toBeNull();
    expect(routePeerGone(lost(76 * SEC), null)).toEqual({
      because: 'silent',
      lostSinceMs: LOST_AT,
      lostForMs: 76 * SEC,
    });
    expect(routePeerGone(lost(8 * 3600 * SEC), null)?.because).toBe('silent');
  });

  it('only the Mac’s own word says its goose quit — in this read, or kept by main', () => {
    expect(routePeerGone(null, 'quit')).toEqual({ because: 'said-quit' });
    expect(routePeerGone(lost(0, { saidQuit: true }), null)).toEqual({ because: 'said-quit' });
    // Restarting goose is a blip by its own word.
    expect(routePeerGone(lost(10 * SEC), 'restart')).toBeNull();
  });

  it('contact not lost (main reads the Mac): never away', () => {
    expect(routePeerGone(contact({ longestComebackMs: 25 * SEC }), null)).toBeNull();
    expect(routePeerGone(null, null)).toBeNull();
  });
});

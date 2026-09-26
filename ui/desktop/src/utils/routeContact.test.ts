import { describe, expect, it } from 'vitest';
import {
  GONE_PAST_LONGEST_COMEBACK,
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

describe('routePeerGone — gone is the Mac’s own word, or a wait well past its measured comebacks (Q-111)', () => {
  const SEC = 1000;
  const contact = (over: Partial<RouteContact>): RouteContact => ({
    lostForMs: null,
    longestComebackMs: null,
    comebacks: 0,
    saidQuit: false,
    ...over,
  });

  it('the threshold is a ratio of this route’s own longest comeback, never a typed number of seconds', () => {
    expect(GONE_PAST_LONGEST_COMEBACK).toBe(3);
    // The owner's measured relaunch: 25 s → gone past 75 s.
    expect(waitedPastComebacks(75 * SEC, 25 * SEC)).toBe(false);
    expect(waitedPastComebacks(75 * SEC + 1, 25 * SEC)).toBe(true);
    // A route that only ever blipped for 2 s is called gone sooner; one that took 90 s to mount, later.
    expect(waitedPastComebacks(7 * SEC, 2 * SEC)).toBe(true);
    expect(waitedPastComebacks(200 * SEC, 90 * SEC)).toBe(false);
    // Nothing measured: no wait is too long.
    expect(waitedPastComebacks(8 * 3600 * SEC, null)).toBe(false);
  });

  it('walks the overnight quit: reconnecting at 60 s, gone at 76 s, still gone hours later', () => {
    const at = (lostForMs: number) =>
      routePeerGone(contact({ lostForMs, longestComebackMs: 25 * SEC, comebacks: 1 }), null);
    expect(at(60 * SEC)).toBeNull();
    expect(at(76 * SEC)).toEqual({
      because: 'unreachable',
      lostForMs: 76 * SEC,
      longestComebackMs: 25 * SEC,
    });
    expect(at(8 * 3600 * SEC)?.because).toBe('unreachable');
  });

  it('the Mac said it quit goose: gone at once, whether the words are in this read or kept by main', () => {
    expect(routePeerGone(null, 'quit')).toEqual({ because: 'said-quit' });
    expect(routePeerGone(contact({ lostForMs: 0, saidQuit: true }), null)).toEqual({
      because: 'said-quit',
    });
    // Restarting goose is a blip by its own word.
    expect(
      routePeerGone(contact({ lostForMs: 10 * SEC, longestComebackMs: 25 * SEC }), 'restart')
    ).toBeNull();
  });

  it('contact not lost (main reads the Mac), or no measurement: never gone by time', () => {
    expect(
      routePeerGone(contact({ lostForMs: null, longestComebackMs: 25 * SEC }), null)
    ).toBeNull();
    expect(routePeerGone(contact({ lostForMs: 8 * 3600 * SEC }), null)).toBeNull();
    expect(routePeerGone(null, null)).toBeNull();
  });
});

import { describe, it, expect } from 'vitest';
import {
  CONFIRM_CLOSE_RUN_CHANNEL,
  CONFIRM_CLOSE_RUN_REPLY_CHANNEL,
  ConfirmedCloses,
  decideClose,
  isTurnsInFlight,
  TURNS_IN_FLIGHT_CHANNEL,
} from '../closeGuard';
import { isSwarmRunStampAlive } from '../shortcutGuard';
import { SWARM_HEARTBEAT_STALE_MS } from '../../components/swarm/swarmRunLiveness';

/**
 * Q2 (branch review, 2026-09-01): the accelerator guard refuses Cmd+W on a window holding a live
 * session run, but the traffic-light close button was "deliberately unguarded: a click is the user
 * meaning it" — and one click released the lease, SIGTERMed goosed's process group and killed the
 * run. A click is now ASKED: main keeps the window on the first `close`, the renderer shows the
 * dialog, and only a confirmed reply lets the second `close` through.
 */
describe('decideClose — a mouse close on a protected window is asked, never refused, never obeyed blind', () => {
  const alive = {
    confirmed: false,
    windowHoldsLiveRun: true,
    windowHoldsLiveTurn: false,
    rendererCanAnswer: true,
  };

  it('asks when the window holds a live run and its renderer can answer', () => {
    expect(decideClose(alive)).toBe('ask');
  });

  it('passes an unprotected window through untouched — closes exactly as before', () => {
    expect(decideClose({ ...alive, windowHoldsLiveRun: false })).toBe('pass');
  });

  it('passes once the renderer confirmed — the flag is what lets the second close through', () => {
    expect(decideClose({ ...alive, confirmed: true })).toBe('pass');
  });

  it('FAILS OPEN when the renderer is destroyed or crashed: a window nobody can answer from is never stuck', () => {
    expect(decideClose({ ...alive, rendererCanAnswer: false })).toBe('pass');
  });

  it('the confirmed flag wins even over a still-live run (the user already answered)', () => {
    expect(
      decideClose({
        confirmed: true,
        windowHoldsLiveRun: true,
        windowHoldsLiveTurn: true,
        rendererCanAnswer: true,
      })
    ).toBe('pass');
  });
});

/**
 * Q-490 (reproduced twice, the second time on the installed 3.0.78 at 09:00:56Z): a second window's
 * chat was generating when that window was closed. Its prompt runs on THAT window's ACP connection,
 * and goosed logged "a prompt dropped before it ended (its connection closed …)" — the close ends
 * the answer. The guard saw swarm runs only (windowHoldsLiveRun false for a chat), so it passed the
 * close without a question. A window with a turn in flight is now asked like one with a run.
 */
describe('decideClose — a window whose chat is mid-turn is asked too (Q-490)', () => {
  const chatMidTurn = {
    confirmed: false,
    windowHoldsLiveRun: false,
    windowHoldsLiveTurn: true,
    rendererCanAnswer: true,
  };

  it('asks when the window has a prompt in flight and no swarm run (the 09:00:56Z close)', () => {
    expect(decideClose(chatMidTurn)).toBe('ask');
  });

  it('asks when the window holds both', () => {
    expect(decideClose({ ...chatMidTurn, windowHoldsLiveRun: true })).toBe('ask');
  });

  it('passes once the person confirmed, and fails open when the renderer cannot answer', () => {
    expect(decideClose({ ...chatMidTurn, confirmed: true })).toBe('pass');
    expect(decideClose({ ...chatMidTurn, rendererCanAnswer: false })).toBe('pass');
  });

  it('an idle window — no run, no turn — closes untouched, exactly as before', () => {
    expect(decideClose({ ...chatMidTurn, windowHoldsLiveTurn: false })).toBe('pass');
  });
});

describe('isTurnsInFlight — main takes only a well-formed report from a renderer', () => {
  it('accepts an empty list and named or unnamed chats', () => {
    expect(isTurnsInFlight([])).toBe(true);
    expect(
      isTurnsInFlight([
        { sessionId: '20260928_18', sessionName: 'Lighthouse story' },
        { sessionId: '20260929_1', sessionName: null },
      ])
    ).toBe(true);
  });

  it('refuses anything else', () => {
    expect(isTurnsInFlight(null)).toBe(false);
    expect(isTurnsInFlight({ sessionId: 'x', sessionName: null })).toBe(false);
    expect(isTurnsInFlight([{ sessionId: 7, sessionName: null }])).toBe(false);
    expect(isTurnsInFlight([{ sessionId: 'x' }])).toBe(false);
    expect(isTurnsInFlight([null])).toBe(false);
  });
});

describe('ConfirmedCloses — the pass-through flag is consumed by exactly one close', () => {
  it('walks the motivating sequence: first close asks, the confirmed reply arms, the second close passes, a later close asks again', () => {
    const flags = new ConfirmedCloses();
    const win = 7;
    const closeVerdict = () =>
      decideClose({
        confirmed: flags.take(win),
        windowHoldsLiveRun: true,
        windowHoldsLiveTurn: false,
        rendererCanAnswer: true,
      });

    expect(closeVerdict()).toBe('ask');
    flags.confirm(win);
    expect(flags.has(win)).toBe(true);
    expect(closeVerdict()).toBe('pass');
    expect(flags.has(win)).toBe(false);
    expect(closeVerdict()).toBe('ask');
  });

  it('a flag is per window: confirming one window never lets another through', () => {
    const flags = new ConfirmedCloses();
    flags.confirm(1);
    expect(flags.take(2)).toBe(false);
    expect(flags.take(1)).toBe(true);
  });

  it('forget drops a flag nobody took (the window went away on its own)', () => {
    const flags = new ConfirmedCloses();
    flags.confirm(3);
    flags.forget(3);
    expect(flags.take(3)).toBe(false);
  });
});

describe("the protected predicate is the accelerator guard's, stamp decay included", () => {
  const NOW = 1_800_000_000_000;
  const verdictFor = (stamp: Parameters<typeof isSwarmRunStampAlive>[0]) =>
    decideClose({
      confirmed: false,
      windowHoldsLiveRun: isSwarmRunStampAlive(stamp, NOW),
      windowHoldsLiveTurn: false,
      rendererCanAnswer: true,
    });

  it('a fresh stamp asks; a stamp past SWARM_HEARTBEAT_STALE_MS passes (no seconds literal of its own)', () => {
    expect(verdictFor({ heartbeat: NOW - 3_000, heartbeatExited: false })).toBe('ask');
    expect(
      verdictFor({ heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1, heartbeatExited: false })
    ).toBe('pass');
  });

  it('an EXITED run or no stamp at all closes as before', () => {
    expect(verdictFor({ heartbeat: NOW - 1_000, heartbeatExited: true })).toBe('pass');
    expect(verdictFor(undefined)).toBe('pass');
  });
});

describe('the IPC channel names are the ones main and preload agree on', () => {
  it('main asks on confirm-close-run and listens on confirm-close-run-reply', () => {
    expect(CONFIRM_CLOSE_RUN_CHANNEL).toBe('confirm-close-run');
    expect(CONFIRM_CLOSE_RUN_REPLY_CHANNEL).toBe('confirm-close-run-reply');
    expect(TURNS_IN_FLIGHT_CHANNEL).toBe('turns-in-flight');
  });
});

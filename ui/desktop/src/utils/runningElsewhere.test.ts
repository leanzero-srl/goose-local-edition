import { describe, expect, it } from 'vitest';
import type { RunningSessionDto } from '@aaif/goose-sdk';
import type { TurnInFlight } from './closeGuard';
import {
  isRunningElsewhereList,
  joinRunningRows,
  runningElsewhereFor,
  turnHolderOf,
} from './runningElsewhere';
import { isGlanceSessions, mergeGlanceSessions } from './engineGlance';

/**
 * Q-500: main joins every window's goosed `running` rows (each window's connection keeps its own busy
 * set, acp/server/needs_you.rs) and hands each window the OTHERS' — with the window whose connection
 * holds the prompt, the only one that can stop it.
 */

const MAIN = 1;
const SECOND = 2;
const row = (sessionId: string, startedAt: string, sessionName = sessionId): RunningSessionDto => ({
  sessionId,
  sessionName,
  workingDir: '/w',
  startedAt,
});
const COFFEE = row('20260929_15', '2026-09-29T12:30:14+00:00', 'Coffee Roasters Double-Charge Incident');
const JIRA = row('20260928_19', '2026-09-29T12:31:02+00:00', 'Jira DC to Cloud migration assessment');
const turn = (sessionId: string): TurnInFlight => ({ sessionId, sessionName: null });

describe('runningElsewhereFor (Q-500)', () => {
  // The 15:48 layout: the main window's connection runs Coffee and lists Jira (process-wide agents);
  // the second window's lists Jira only.
  const running = new Map([
    [MAIN, [COFFEE, JIRA]],
    [SECOND, [JIRA]],
  ]);
  const turns = new Map([
    [MAIN, [turn(COFFEE.sessionId)]],
    [SECOND, [] as TurnInFlight[]],
  ]);

  it('the second window is handed Coffee, held by the main window', () => {
    const elsewhere = runningElsewhereFor(SECOND, running, turns);
    expect(elsewhere).toEqual([
      { ...COFFEE, window: MAIN },
      { ...JIRA, window: null },
    ]);
    expect(isRunningElsewhereList(elsewhere)).toBe(true);
  });

  it('the main window is never handed its own rows, nor a hold on its own turn', () => {
    expect(runningElsewhereFor(MAIN, running, turns)).toEqual([{ ...JIRA, window: null }]);
    expect(turnHolderOf(COFFEE.sessionId, turns, MAIN)).toBeNull();
    expect(turnHolderOf(COFFEE.sessionId, turns, SECOND)).toBe(MAIN);
  });

  it('a turn two connections list is one row, from its earliest start', () => {
    const later = { ...COFFEE, startedAt: '2026-09-29T12:40:00+00:00' };
    const three = new Map([
      [MAIN, [COFFEE]],
      [SECOND, [later]],
      [3, [] as RunningSessionDto[]],
    ]);
    expect(runningElsewhereFor(3, three, new Map())).toEqual([{ ...COFFEE, window: null }]);
    expect(joinRunningRows([later], [{ ...COFFEE, window: MAIN }])).toEqual([COFFEE]);
  });

  it('a malformed push is refused whole', () => {
    expect(isRunningElsewhereList([{ ...COFFEE }])).toBe(false);
    expect(isRunningElsewhereList([{ ...COFFEE, window: '1' }])).toBe(false);
    expect(isRunningElsewhereList(null)).toBe(false);
  });
});

describe('the glance counts a turn once however many windows list it (Q-500)', () => {
  it('Coffee + Jira listed by the main window, Jira by the second: 2 running, not 3', () => {
    const main = { running: 2, needsYou: [], runningRows: [COFFEE, JIRA] };
    const second = { running: 1, needsYou: [], runningRows: [JIRA] };
    expect(isGlanceSessions(main)).toBe(true);
    expect(mergeGlanceSessions([main, second]).running).toBe(2);
  });

  it('a report without rows (an older renderer) still adds its count', () => {
    expect(
      mergeGlanceSessions([{ running: 1, needsYou: [] }, { running: 1, needsYou: [], runningRows: [JIRA] }])
        .running
    ).toBe(2);
  });

  it('a report whose rows are malformed is refused', () => {
    expect(isGlanceSessions({ running: 1, needsYou: [], runningRows: [{ sessionId: 'x' }] })).toBe(
      false
    );
  });
});

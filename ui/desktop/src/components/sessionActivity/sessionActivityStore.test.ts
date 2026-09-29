import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpElicitationRequest } from '../../acp/elicitationRequests';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

const elicitations = vi.hoisted(() => ({
  pending: [] as unknown[],
  listener: undefined as undefined | (() => void),
}));
vi.mock('../../acp/elicitationRequests', () => ({
  pendingAcpElicitations: () => elicitations.pending,
  subscribePendingAcpElicitations: (listener: () => void) => {
    elicitations.listener = listener;
    return () => {
      elicitations.listener = undefined;
    };
  },
}));

import {
  activeSessions,
  activityOf,
  answerMessage,
  disambiguatedNames,
  elapsedLabel,
  getSessionActivitySnapshot,
  isActive,
  refreshSessionActivity,
  resetSessionActivityForTests,
  resolveNeedsYou,
  SESSION_STATE_ORDER,
  sessionStates,
  startSessionActivitySync,
  type LoopStatus,
  type SessionActivitySnapshot,
  type SessionState,
} from './sessionActivityStore';
import { AppEvents } from '../../constants/events';

function item(id: string, sessionId: string, question = `Q ${id}`) {
  return {
    id,
    sessionId,
    sessionName: `Session ${sessionId}`,
    workingDir: `/proj/${sessionId}`,
    question,
    why: 'because',
    recommendedAnswer: 'yes',
    options: [],
    createdAt: '2026-09-26T10:00:00Z',
    status: 'open' as const,
  };
}

function running(sessionId: string, startedAt: string) {
  return {
    sessionId,
    sessionName: `Session ${sessionId}`,
    workingDir: `/proj/${sessionId}`,
    startedAt,
  };
}

function snapshotWith(partial: Partial<SessionActivitySnapshot>): SessionActivitySnapshot {
  return {
    running: [],
    elsewhere: [],
    needsYou: [],
    failed: [],
    stopped: [],
    background: [],
    looping: [],
    notesWaiting: [],
    elicitations: [],
    ...partial,
  };
}

function loop(sessionId: string, status: LoopStatus, nextTickAt?: string) {
  return { sessionId, status, ...(nextTickAt ? { nextTickAt } : {}) };
}

const FAILED_TICK = {
  sessionId: 'L',
  sessionName: 'Loop chat',
  workingDir: '/proj/L',
  failedAt: '2026-09-27T22:31:00Z',
  reason: 'Provider error: stream ended early',
};
const STOPPED_TICK = {
  sessionId: 'L',
  sessionName: 'Loop chat',
  workingDir: '/proj/L',
  stoppedAt: '2026-09-27T22:36:00Z',
  elapsedMs: 72_000,
};
const BACKGROUND_CHECK = {
  sessionId: 'L',
  sessionName: 'Loop chat',
  workingDir: '/proj/L',
  kind: 'factCheck' as const,
  startedAt: '2026-09-27T22:37:00Z',
};

describe('session activity: the one source of running / needs-you / failed / idle', () => {
  beforeEach(() => {
    resetSessionActivityForTests();
    acp.acpSessionActivity.mockReset();
    acp.acpResolveNeedsYou.mockReset();
    elicitations.pending = [];
  });
  afterEach(() => resetSessionActivityForTests());

  it('reads each state of a session from the snapshot, most urgent first', () => {
    const state = snapshotWith({
      running: [running('a', '2026-09-26T10:00:00Z'), running('b', '2026-09-26T10:05:00Z')],
      needsYou: [item('i1', 'b'), item('i2', 'b')],
      failed: [
        {
          sessionId: 'c',
          sessionName: 'C',
          workingDir: '/proj/c',
          failedAt: '2026-09-26T09:00:00Z',
          reason: 'The split across your Macs stopped mid-answer',
        },
        // A new turn on a failed session is RUNNING: the failure is history.
        {
          sessionId: 'a',
          sessionName: 'A',
          workingDir: '/proj/a',
          failedAt: '2026-09-26T09:00:00Z',
        },
      ],
    });
    expect(sessionStates(activityOf(state, 'a'))).toEqual(['running']);
    expect(sessionStates(activityOf(state, 'b'))).toEqual(['needs-you', 'running']);
    expect(activityOf(state, 'b').needsYou).toBe(2);
    expect(sessionStates(activityOf(state, 'c'))).toEqual(['failed']);
    expect(activityOf(state, 'c').failedReason).toContain('stopped mid-answer');
    expect(sessionStates(activityOf(state, 'idle'))).toEqual(['idle']);
  });

  // Session loops §8.6 / L7: needs-you > running > background > looping > failed > stopped > idle.
  describe('the order of states, looping included (Q-228)', () => {
    const table: Array<[string, Partial<SessionActivitySnapshot>, SessionState[]]> = [
      [
        'a tick in flight is a turn: Running, not Looping',
        { running: [running('L', '2026-09-27T22:40:00Z')], looping: [loop('L', 'running')] },
        ['running'],
      ],
      [
        "the tick's reviewers after it: Background, not Looping",
        { background: [BACKGROUND_CHECK], looping: [loop('L', 'waiting', '2026-09-27T22:50:00Z')] },
        ['background'],
      ],
      [
        'a tick that asked the person: Needs you leads, the loop still shows',
        { needsYou: [item('i1', 'L')], looping: [loop('L', 'needs_you')] },
        ['needs-you', 'looping'],
      ],
      [
        "the answer's turn runs: Needs you is gone, the turn is Running",
        { running: [running('L', '2026-09-27T22:45:00Z')], looping: [loop('L', 'needs_you')] },
        ['running'],
      ],
      [
        'a loop between ticks',
        { looping: [loop('L', 'waiting', '2026-09-27T22:50:00Z')] },
        ['looping'],
      ],
      [
        "a loop whose last tick failed reads Looping, not Failed (the failure is on the tick's row)",
        { failed: [FAILED_TICK], looping: [loop('L', 'waiting', '2026-09-27T22:50:00Z')] },
        ['looping'],
      ],
      [
        'two failed ticks paused the loop: Looping (Paused), not Failed',
        { failed: [FAILED_TICK], looping: [loop('L', 'paused')] },
        ['looping'],
      ],
      [
        'the person stopped a tick: its own state, the loop Paused, not Stopped',
        { stopped: [STOPPED_TICK], looping: [loop('L', 'paused')] },
        ['looping'],
      ],
      [
        'a yielded tick is never Stopped (the engine records no stop; the tick waits its turn)',
        { looping: [loop('L', 'waiting_turn')] },
        ['looping'],
      ],
      ['a self-paced tick named no delay', { looping: [loop('L', 'waiting_you')] }, ['looping']],
      ['a loop checking after its tick', { looping: [loop('L', 'checking')] }, ['looping']],
      ['a loop in another window', { looping: [loop('L', 'elsewhere')] }, ['looping']],
      [
        'an unreadable loop record is named, never read as "no loop"',
        {
          failed: [FAILED_TICK],
          looping: [{ sessionId: 'L', error: 'The loop record could not be read: bad id' }],
        },
        ['looping'],
      ],
      [
        'an ended loop is not looping: the last turn shows again',
        { failed: [FAILED_TICK], looping: [loop('L', 'ended')] },
        ['failed'],
      ],
      ['failed outranks stopped', { failed: [FAILED_TICK], stopped: [STOPPED_TICK] }, ['failed']],
      ['stopped alone', { stopped: [STOPPED_TICK] }, ['stopped']],
      ['nothing holds', {}, ['idle']],
    ];
    it.each(table)('%s', (_name, partial, expected) => {
      const states = sessionStates(activityOf(snapshotWith(partial), 'L'));
      expect(states).toEqual(expected);
      const ranks = states.map((state) => SESSION_STATE_ORDER.indexOf(state));
      expect(ranks).toEqual([...ranks].sort((a, b) => a - b));
    });

    it('reads the loop facts a row shows, and only for its own chat', () => {
      const state = snapshotWith({
        looping: [
          loop('L', 'waiting', '2026-09-27T22:50:00Z'),
          { sessionId: 'U', error: 'The loop record could not be read: bad id' },
        ],
      });
      expect(activityOf(state, 'L')).toMatchObject({
        loopStatus: 'waiting',
        loopNextTickAt: '2026-09-27T22:50:00Z',
        loopError: undefined,
      });
      expect(activityOf(state, 'U')).toMatchObject({
        loopStatus: undefined,
        loopError: 'The loop record could not be read: bad id',
      });
      expect(activityOf(state, 'other')).toMatchObject({
        loopStatus: undefined,
        loopNextTickAt: undefined,
        loopError: undefined,
      });
    });

    it('a waiting loop is a scheduled future, not work in flight: never Active now', () => {
      const state = snapshotWith({
        looping: [loop('L', 'waiting', '2026-09-27T22:50:00Z'), loop('P', 'paused')],
      });
      expect(isActive(activityOf(state, 'L'))).toBe(false);
      expect(isActive(activityOf(state, 'P'))).toBe(false);
      expect(activeSessions(state)).toEqual([]);
      // A tick in flight IS a running turn.
      const ticking = snapshotWith({
        running: [running('L', '2026-09-27T22:40:00Z')],
        looping: [loop('L', 'running')],
      });
      expect(activeSessions(ticking).map((row) => row.sessionId)).toEqual(['L']);
    });
  });

  // Q-169: a stopped turn left the row reading "15m ago", as if nothing had happened.
  it('reads a session whose last turn the person stopped as stopped, until a turn runs', () => {
    const stoppedRow = {
      sessionId: 's',
      sessionName: 'Jira Migration Kickoff',
      workingDir: '/proj/s',
      stoppedAt: '2026-09-27T09:58:00Z',
      elapsedMs: 372_000,
      outputTokens: 1_900,
    };
    const state = snapshotWith({ stopped: [stoppedRow] });
    expect(sessionStates(activityOf(state, 's'))).toEqual(['stopped']);
    expect(activityOf(state, 's')).toMatchObject({
      stoppedAt: '2026-09-27T09:58:00Z',
      stoppedElapsedMs: 372_000,
      stoppedOutputTokens: 1_900,
    });
    const rerun = snapshotWith({
      stopped: [stoppedRow],
      running: [running('s', '2026-09-27T10:00:00Z')],
    });
    expect(sessionStates(activityOf(rerun, 's'))).toEqual(['running']);
  });

  it('lists active sessions with waiting ones first, and counts live elicitations as needs-you', () => {
    const elicitation = {
      id: 'acp_elicitation_1',
      sessionId: 'd',
      request: { message: 'Pick a project' },
    } as unknown as AcpElicitationRequest;
    const rows = activeSessions(
      snapshotWith({
        running: [running('a', '2026-09-26T10:00:00Z')],
        needsYou: [item('i1', 'b', 'Which database?')],
        elicitations: [elicitation],
      })
    );
    expect(rows.map((r) => r.sessionId)).toEqual(['b', 'd', 'a']);
    expect(rows[0].headline).toBe('Which database?');
    expect(rows[1].headline).toBe('Pick a project');
    expect(rows[2].runningSince).toBe('2026-09-26T10:00:00Z');
  });

  it('gives same-title sessions " · 2" by age so they can be told apart', () => {
    const labels = disambiguatedNames(
      [
        { id: 'new', name: 'Jira Migration Kickoff Notes', createdAt: '2026-09-26T19:00:00Z' },
        { id: 'old', name: 'Jira Migration Kickoff Notes', createdAt: '2026-09-20T08:00:00Z' },
        { id: 'solo', name: 'Other', createdAt: '2026-09-21T08:00:00Z' },
      ],
      (s) => s.name
    );
    expect(labels.get('old')).toBe('Jira Migration Kickoff Notes');
    expect(labels.get('new')).toBe('Jira Migration Kickoff Notes · 2');
    expect(labels.get('solo')).toBe('Other');
  });

  it('formats a live elapsed and the answer message the model receives', () => {
    const now = Date.parse('2026-09-26T10:27:30Z');
    expect(elapsedLabel('2026-09-26T10:27:00Z', now)).toBe('30s');
    expect(elapsedLabel('2026-09-26T10:00:00Z', now)).toBe('27m');
    expect(elapsedLabel('2026-09-26T08:22:00Z', now)).toBe('2h 05m');
    expect(answerMessage(' Which database? ', ' SQLite ')).toBe(
      'Answer to your question "Which database?": SQLite'
    );
  });

  it('refreshes from the engine and skips an unchanged answer', async () => {
    const answer = {
      running: [running('a', '2026-09-26T10:00:00Z')],
      needsYou: [item('i1', 'b')],
      failed: [],
    };
    acp.acpSessionActivity.mockResolvedValue(answer);
    await refreshSessionActivity();
    const first = getSessionActivitySnapshot();
    expect(first.running).toEqual(answer.running);
    expect(first.needsYou).toEqual(answer.needsYou);
    await refreshSessionActivity();
    expect(getSessionActivitySnapshot()).toBe(first);
  });

  it("keeps the engine's looping list, and an engine older than loops sends none", async () => {
    const looping = [loop('L', 'waiting', '2026-09-27T22:50:00Z')];
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [], looping });
    await refreshSessionActivity();
    const first = getSessionActivitySnapshot();
    expect(first.looping).toEqual(looping);
    await refreshSessionActivity();
    expect(getSessionActivitySnapshot()).toBe(first);
    // The loop pauses: only the loop changed, and that alone is a new answer.
    acp.acpSessionActivity.mockResolvedValue({
      running: [],
      needsYou: [],
      failed: [],
      looping: [loop('L', 'paused')],
    });
    await refreshSessionActivity();
    expect(getSessionActivitySnapshot().looping).toEqual([loop('L', 'paused')]);

    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
    await refreshSessionActivity();
    expect(getSessionActivitySnapshot().looping).toEqual([]);
  });

  it('resolving drops the item at once and re-reads the engine', async () => {
    acp.acpSessionActivity.mockResolvedValueOnce({
      running: [],
      needsYou: [item('i1', 'b'), item('i2', 'b')],
      failed: [],
    });
    await refreshSessionActivity();
    acp.acpResolveNeedsYou.mockResolvedValue(item('i1', 'b'));
    acp.acpSessionActivity.mockResolvedValue({
      running: [],
      needsYou: [item('i2', 'b')],
      failed: [],
    });
    await resolveNeedsYou({ id: 'i1', sessionId: 'b' }, 'answer', 'SQLite');
    expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith('b', 'i1', 'answer', 'SQLite');
    expect(getSessionActivitySnapshot().needsYou.map((i) => i.id)).toEqual(['i2']);
  });

  it('a failed resolve leaves the item in place', async () => {
    acp.acpSessionActivity.mockResolvedValue({
      running: [],
      needsYou: [item('i1', 'b')],
      failed: [],
    });
    await refreshSessionActivity();
    acp.acpResolveNeedsYou.mockRejectedValue(new Error('engine gone'));
    await expect(resolveNeedsYou({ id: 'i1', sessionId: 'b' }, 'dismiss')).rejects.toThrow(
      'engine gone'
    );
    expect(getSessionActivitySnapshot().needsYou.map((i) => i.id)).toEqual(['i1']);
  });

  it('the sync re-reads when the engine says a run started or ended, and follows live elicitations', async () => {
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
    startSessionActivitySync();
    await Promise.resolve();
    const calls = acp.acpSessionActivity.mock.calls.length;
    window.dispatchEvent(new CustomEvent(AppEvents.SESSION_ACTIVITY_CHANGED));
    expect(acp.acpSessionActivity.mock.calls.length).toBe(calls + 1);

    elicitations.pending = [{ id: 'e1', sessionId: 's', request: { message: 'm' } }];
    elicitations.listener?.();
    expect(getSessionActivitySnapshot().elicitations).toHaveLength(1);
  });
});

/**
 * Q-500: the second window's connection cannot see the turn the main window's connection runs; main
 * pushes it (utils/runningElsewhere.ts), and the store joins it into every running read — and keeps
 * it across its own polls, which never list it.
 */
describe('turns another window runs (Q-500)', () => {
  const COFFEE = {
    sessionId: '20260929_15',
    sessionName: 'Coffee Roasters Double-Charge Incident',
    workingDir: '/w',
    startedAt: '2026-09-29T12:30:14+00:00',
  };
  const JIRA = {
    ...COFFEE,
    sessionId: '20260928_19',
    sessionName: 'Jira DC to Cloud migration assessment',
    startedAt: '2026-09-29T12:31:02+00:00',
  };
  let pushed: ((event: unknown, ...args: unknown[]) => void) | undefined;
  const original = (window as unknown as { electron: unknown }).electron;
  beforeEach(() => {
    pushed = undefined;
    elicitations.pending = [];
    (window as unknown as { electron: unknown }).electron = {
      on: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => {
        if (channel === 'session-running-elsewhere') pushed = fn;
      },
      off: vi.fn(),
    };
  });
  afterEach(() => {
    resetSessionActivityForTests();
    (window as unknown as { electron: unknown }).electron = original;
  });

  it('main’s push makes the chat running here, and the window’s own re-read keeps it', async () => {
    acp.acpSessionActivity.mockResolvedValue({ running: [JIRA], needsYou: [], failed: [] });
    startSessionActivitySync();
    await refreshSessionActivity();
    expect(activityOf(getSessionActivitySnapshot(), COFFEE.sessionId).runningSince).toBeUndefined();

    pushed?.({}, [{ ...COFFEE, window: 1 }]);
    const state = getSessionActivitySnapshot();
    expect(activityOf(state, COFFEE.sessionId)).toMatchObject({
      runningSince: COFFEE.startedAt,
      turnWindow: 1,
    });
    expect(sessionStates(activityOf(state, COFFEE.sessionId))).toEqual(['running']);
    expect(activeSessions(state).map((s) => s.sessionId)).toEqual([
      COFFEE.sessionId,
      JIRA.sessionId,
    ]);

    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
    await refreshSessionActivity();
    expect(activityOf(getSessionActivitySnapshot(), COFFEE.sessionId).runningSince).toBe(
      COFFEE.startedAt
    );

    pushed?.({}, []);
    expect(activityOf(getSessionActivitySnapshot(), COFFEE.sessionId).runningSince).toBeUndefined();
  });

  it('a malformed push changes nothing', () => {
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
    startSessionActivitySync();
    pushed?.({}, [{ sessionId: 'x' }]);
    expect(getSessionActivitySnapshot().elsewhere).toEqual([]);
  });
});

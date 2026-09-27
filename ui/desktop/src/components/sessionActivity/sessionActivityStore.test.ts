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
  refreshSessionActivity,
  resetSessionActivityForTests,
  resolveNeedsYou,
  sessionStates,
  startSessionActivitySync,
  type SessionActivitySnapshot,
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
    needsYou: [],
    failed: [],
    stopped: [],
    background: [],
    elicitations: [],
    ...partial,
  };
}

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

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import type { ReactNode } from 'react';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

const sessionsAcp = vi.hoisted(() => ({ acpListSessions: vi.fn() }));
vi.mock('../../acp/sessions', () => ({
  acpListSessions: sessionsAcp.acpListSessions,
  acpDeleteSession: vi.fn(),
  acpExportSession: vi.fn(),
  acpForkSession: vi.fn(),
  acpImportSession: vi.fn(),
  acpRenameSession: vi.fn(),
  acpShareSessionNostr: vi.fn(),
}));

import SessionActivityIndicator from './SessionActivityIndicator';
import ActiveNowSection from './ActiveNowSection';
import {
  FailedPill,
  LoopingPill,
  NeedsYouPill,
  RunningPill,
  StoppedPill,
  resetNowForTests,
} from './ActivityPills';
import { SystemNotificationInline } from '../context_management/SystemNotificationInline';
import SessionListView from '../sessions/SessionListView';
import { resetSessionActivityForTests, seedSessionActivityForTests } from './sessionActivityStore';
import { PHASE_FILL, TONE_FILL } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import NeedsYouTray from './NeedsYouCard';
import { BackgroundWorkLine } from './BackgroundWorkLine';
import { ChatState } from '../../types/chatState';

const NOW = Date.parse('2026-09-26T10:27:00Z');

function running(sessionId: string, sessionName: string, startedAt = '2026-09-26T10:00:00Z') {
  return { sessionId, sessionName, workingDir: `/Users/me/${sessionId}-dir`, startedAt };
}

function question(id: string, sessionId: string, sessionName: string, text: string) {
  return {
    id,
    sessionId,
    sessionName,
    workingDir: '/Users/me/api',
    question: text,
    why: 'w',
    recommendedAnswer: 'r',
    options: [],
    createdAt: '2026-09-26T09:00:00Z',
    status: 'open' as const,
  };
}

function LocationProbe() {
  const location = useLocation();
  return <div data-testid="location">{`${location.pathname}${location.search}`}</div>;
}

function renderRouted(node: ReactNode) {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <MemoryRouter initialEntries={['/settings']}>
        {node}
        <Routes>
          <Route path="*" element={<LocationProbe />} />
        </Routes>
      </MemoryRouter>
    </IntlProvider>
  );
}

describe('session state: running / needs-you / failed, the same everywhere', () => {
  beforeEach(() => {
    resetNowForTests(NOW);
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => resetSessionActivityForTests());

  it('pills are solid fills with their meaning in words, and running carries a live elapsed', () => {
    render(
      <IntlProvider locale="en" messages={{}}>
        <RunningPill since="2026-09-26T10:00:00Z" />
        <NeedsYouPill count={2} />
        <FailedPill reason="The split across your Macs stopped mid-answer" />
      </IntlProvider>
    );
    const run = screen.getByTestId('session-running-pill');
    expect(run.textContent).toBe('Running · 27m');
    for (const c of PHASE_FILL.writing.split(' ')) expect(run.className).toContain(c);
    const needs = screen.getByTestId('session-needs-you-pill');
    expect(needs.textContent).toBe('Needs you · 2');
    for (const c of TONE_FILL.warn.split(' ')) expect(needs.className).toContain(c);
    const failed = screen.getByTestId('session-failed-pill');
    expect(failed.textContent).toBe('Failed');
    expect(failed.getAttribute('title')).toContain('stopped mid-answer');
    for (const c of TONE_FILL.err.split(' ')) expect(failed.className).toContain(c);
  });

  // Q-169: a stopped turn left only the user's message, and the row read "15m ago".
  it('a stopped turn reads Stopped in a solid pill and leaves its line in the chat', async () => {
    const { container } = render(
      <IntlProvider locale="en" messages={{}}>
        <StoppedPill elapsedMs={372_000} outputTokens={1_900} />
        <SystemNotificationInline
          notification={{
            notificationType: 'inlineMessage',
            msg: 'You stopped this answer after 6 min · 1.9k tokens',
            data: { kind: 'turnStopped', elapsedMs: 372_000, outputTokens: 1_900 },
          }}
        />
      </IntlProvider>
    );
    const pills = screen.getAllByTestId('session-stopped-pill');
    expect(pills[0].textContent).toBe('Stopped');
    expect(pills[0].getAttribute('title')).toBe(
      'You stopped this answer after 6 min · 1.9k tokens'
    );
    for (const c of TONE_FILL.stopped.split(' ')) expect(pills[0].className).toContain(c);
    const line = screen.getByTestId('stopped-turn-line');
    expect(line.textContent).toContain('You stopped this answer after 6 min · 1.9k tokens');
    expect(within(line).getByTestId('session-stopped-pill')).toBeTruthy();
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  // Q-185, E2E #3i: the reply was done and goose's fact check ran for the chat while its row read
  // "4m ago", unmarked. The row, the chat and the Engine card now say the same thing.
  it('goose still working for a chat after its reply: a quiet solid pill on the row, the same words in the chat', async () => {
    Object.assign(window.electron, { getConfig: () => ({}) });
    const base = {
      workingDir: '/Users/me/api',
      messageCount: 3,
      updatedAt: '2026-09-25T10:00:00Z',
      lastMessageAt: '2026-09-25T10:00:00Z',
    };
    sessionsAcp.acpListSessions.mockResolvedValue({
      sessions: [
        { ...base, id: 'check-1', name: 'Kickoff', createdAt: '2026-09-26T10:00:00Z' },
        { ...base, id: 'turn-1', name: 'Readiness', createdAt: '2026-09-25T10:00:00Z' },
      ],
      nextCursor: null,
    });
    const checking = {
      sessionId: 'check-1',
      sessionName: 'Kickoff',
      workingDir: '/Users/me/api',
      kind: 'factCheck' as const,
      startedAt: '2026-09-26T10:26:57Z',
    };
    seedSessionActivityForTests({
      running: [running('turn-1', 'Readiness')],
      // goose's tool label for a RUNNING turn: the turn's Running pill says it all.
      background: [checking, { ...checking, sessionId: 'turn-1', kind: 'toolLabel' as const }],
    });
    render(
      <IntlProvider locale="en" messages={{}}>
        <MemoryRouter>
          <SessionListView onSelectSession={vi.fn()} />
          <BackgroundWorkLine sessionId="check-1" />
          <BackgroundWorkLine sessionId="turn-1" />
        </MemoryRouter>
      </IntlProvider>
    );
    const card = await screen.findByTestId('session-card-check-1');
    expect(card.getAttribute('data-state')).toBe('background');
    expect(card.getAttribute('aria-busy')).toBe('true');
    const pill = within(card).getByTestId('session-background-pill');
    expect(pill.textContent).toBe('Checking');
    expect(pill.getAttribute('title')).toBe(
      'goose is still working for this chat: Checking the reply'
    );
    for (const c of TONE_FILL.secondary.split(' ')) expect(pill.className).toContain(c);

    const turn = screen.getByTestId('session-card-turn-1');
    expect(turn.getAttribute('data-state')).toBe('running');
    expect(within(turn).queryByTestId('session-background-pill')).toBeNull();

    const lines = screen.getAllByTestId('chat-background-work');
    expect(lines).toHaveLength(1);
    expect(lines[0].textContent).toBe('Checking the reply');
    expect(lines[0].dataset.work).toBe('factCheck');
    // The new marks only (the history view around them has its own design test).
    for (const mark of [pill.parentElement!, lines[0]]) {
      assertStudioClean(mark);
      const classes = allClasses(mark).filter((c) => !c.startsWith('lucide'));
      expect(await missingUtilities(classes)).toEqual([]);
    }

    // The check ends: the row goes back to its time, the chat line goes.
    seedSessionActivityForTests({ running: [running('turn-1', 'Readiness')] });
    await waitFor(() =>
      expect(screen.getByTestId('session-card-check-1').getAttribute('data-state')).toBe('idle')
    );
    expect(screen.queryByTestId('chat-background-work')).toBeNull();
  }, 30_000);

  it('Q-307: the memory review is one phrase on the row and under the reply', async () => {
    Object.assign(window.electron, { getConfig: () => ({}) });
    sessionsAcp.acpListSessions.mockResolvedValue({
      sessions: [
        {
          id: 'review-1',
          name: 'Portugal capital question',
          workingDir: '/Users/me/api',
          messageCount: 2,
          createdAt: '2026-09-26T10:20:00Z',
          updatedAt: '2026-09-26T10:23:00Z',
          lastMessageAt: '2026-09-26T10:23:00Z',
        },
      ],
      nextCursor: null,
    });
    seedSessionActivityForTests({
      background: [
        {
          sessionId: 'review-1',
          sessionName: 'Portugal capital question',
          workingDir: '/Users/me/api',
          kind: 'memoryReview' as const,
          startedAt: '2026-09-26T10:26:57Z',
        },
      ],
    });
    render(
      <IntlProvider locale="en" messages={{}}>
        <MemoryRouter>
          <SessionListView onSelectSession={vi.fn()} />
          <BackgroundWorkLine sessionId="review-1" />
        </MemoryRouter>
      </IntlProvider>
    );
    const card = await screen.findByTestId('session-card-review-1');
    const pill = within(card).getByTestId('session-background-pill');
    expect(pill.textContent).toBe('Reviewing for memories');
    expect(screen.getByTestId('chat-background-work').textContent).toBe('Reviewing for memories');
  });

  // Session loops §8.6 (Q-228): a chat whose loop has not ended, between ticks.
  it('the Looping pill: solid fills, its words per loop state, nothing for an ended loop', async () => {
    const next = '2026-09-27T22:40:00Z';
    const at = new Date(next);
    const hm = `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
    const cases = [
      { status: 'waiting', nextTickAt: next, text: `Next ${hm}`, tone: 'accent', title: hm },
      { status: 'running', text: 'Looping', tone: 'accent', title: 'running a tick' },
      { status: 'checking', text: 'Looping', tone: 'accent', title: 'running its check' },
      { status: 'waiting_turn', text: 'Looping', tone: 'accent', title: 'the turn before it ends' },
      { status: 'needs_you', text: 'Looping', tone: 'accent', title: 'your answer' },
      { status: 'elsewhere', text: 'Looping', tone: 'accent', title: 'another goose window' },
      { status: 'waiting_you', text: 'Waiting for you', tone: 'warn', title: 'when to come back' },
      { status: 'paused', text: 'Paused', tone: 'stopped', title: 'is paused' },
      { status: 'waiting', text: 'Looping', tone: 'accent', title: 'sent no time' },
      {
        status: 'waiting',
        nextTickAt: 'soon',
        text: 'Looping',
        tone: 'accent',
        title: '"soon" is not an RFC 3339 time',
      },
    ] as const;
    const { container } = render(
      <IntlProvider locale="en" messages={{}}>
        {cases.map((c, index) => (
          <LoopingPill
            key={index}
            status={c.status}
            nextTickAt={'nextTickAt' in c ? c.nextTickAt : undefined}
          />
        ))}
        <LoopingPill error="The loop record could not be read: missing field `goal`" />
        <LoopingPill status="ended" />
        <LoopingPill />
      </IntlProvider>
    );
    const pills = screen.getAllByTestId('session-looping-pill');
    expect(pills).toHaveLength(cases.length + 1);
    cases.forEach((c, index) => {
      const pill = pills[index];
      expect(pill.textContent).toBe(c.text);
      expect(pill.dataset.loop).toBe(c.status);
      expect(pill.getAttribute('title')).toContain(c.title);
      expect(pill.getAttribute('aria-label')).toBe(pill.getAttribute('title'));
      for (const cls of TONE_FILL[c.tone].split(' ')) expect(pill.className).toContain(cls);
    });
    const unreadable = pills[cases.length];
    expect(unreadable.textContent).toBe('Loop unreadable');
    expect(unreadable.dataset.loop).toBe('unreadable');
    expect(unreadable.getAttribute('title')).toBe(
      'The loop record could not be read: missing field `goal`'
    );
    for (const cls of TONE_FILL.err.split(' ')) expect(unreadable.className).toContain(cls);
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('a looping chat: its row reads Looping over a failed or stopped last turn, a tick in flight reads Running, and a waiting loop is not Active now', async () => {
    Object.assign(window.electron, { getConfig: () => ({}) });
    const base = {
      workingDir: '/Users/me/api',
      messageCount: 3,
      updatedAt: '2026-09-25T10:00:00Z',
      lastMessageAt: '2026-09-25T10:00:00Z',
    };
    sessionsAcp.acpListSessions.mockResolvedValue({
      sessions: [
        { ...base, id: 'loop-wait', name: 'Generator', createdAt: '2026-09-26T09:00:00Z' },
        { ...base, id: 'loop-stop', name: 'Validator', createdAt: '2026-09-25T09:00:00Z' },
        { ...base, id: 'loop-tick', name: 'Kickoff', createdAt: '2026-09-24T09:00:00Z' },
      ],
      nextCursor: null,
    });
    seedSessionActivityForTests({
      running: [running('loop-tick', 'Kickoff')],
      failed: [
        {
          sessionId: 'loop-wait',
          sessionName: 'Generator',
          workingDir: '/Users/me/api',
          failedAt: '2026-09-26T10:00:00Z',
          reason: 'Provider error: stream ended early',
        },
      ],
      stopped: [
        {
          sessionId: 'loop-stop',
          sessionName: 'Validator',
          workingDir: '/Users/me/api',
          stoppedAt: '2026-09-26T10:05:00Z',
          elapsedMs: 72_000,
        },
      ],
      looping: [
        { sessionId: 'loop-wait', status: 'waiting', nextTickAt: '2026-09-26T10:40:00Z' },
        { sessionId: 'loop-stop', status: 'paused' },
        { sessionId: 'loop-tick', status: 'running' },
      ],
    });
    render(
      <IntlProvider locale="en" messages={{}}>
        <MemoryRouter>
          <SessionListView onSelectSession={vi.fn()} />
          <ActiveNowSection />
        </MemoryRouter>
      </IntlProvider>
    );
    const waiting = await screen.findByTestId('session-card-loop-wait');
    expect(waiting.getAttribute('data-state')).toBe('looping');
    expect(waiting.getAttribute('aria-busy')).toBeNull();
    expect(within(waiting).getByTestId('session-looping-pill').textContent).toMatch(
      /^Next \d{2}:\d{2}$/
    );
    expect(within(waiting).queryByTestId('session-failed-pill')).toBeNull();

    const stopped = screen.getByTestId('session-card-loop-stop');
    expect(stopped.getAttribute('data-state')).toBe('looping');
    expect(within(stopped).getByTestId('session-looping-pill').textContent).toBe('Paused');
    expect(within(stopped).queryByTestId('session-stopped-pill')).toBeNull();

    const ticking = screen.getByTestId('session-card-loop-tick');
    expect(ticking.getAttribute('data-state')).toBe('running');
    expect(ticking.getAttribute('aria-busy')).toBe('true');
    expect(within(ticking).getByTestId('session-running-pill')).toBeTruthy();
    expect(within(ticking).queryByTestId('session-looping-pill')).toBeNull();

    // Active now lists the tick in flight only.
    const active = screen.getByTestId('active-now-section');
    expect(within(active).queryByTestId('active-now-row-loop-tick')).toBeTruthy();
    expect(within(active).queryByTestId('active-now-row-loop-wait')).toBeNull();
    expect(within(active).queryByTestId('active-now-row-loop-stop')).toBeNull();
  }, 30_000);

  it('the top bar shows nothing when nothing is active', () => {
    seedSessionActivityForTests({});
    renderRouted(<SessionActivityIndicator />);
    expect(screen.queryByTestId('session-activity-indicator')).toBeNull();
  });

  it('the top bar counts waiting and running separately and one click jumps to a lone session', () => {
    seedSessionActivityForTests({
      running: [running('run-1', 'Jira Migration Kickoff Notes')],
      needsYou: [question('ny_1', 'ask-1', 'Service setup', 'Which database?')],
    });
    renderRouted(<SessionActivityIndicator />);
    expect(screen.getByTestId('indicator-needs-you').textContent).toContain('1 needs you');
    expect(screen.getByTestId('indicator-running').textContent).toContain('1 running');

    fireEvent.click(screen.getByTestId('indicator-running'));
    expect(screen.getByTestId('location').textContent).toBe('/pair?resumeSessionId=run-1');
    fireEvent.click(screen.getByTestId('indicator-needs-you'));
    expect(screen.getByTestId('location').textContent).toBe('/pair?resumeSessionId=ask-1');
  });

  it('with several running sessions the top bar lists them, each a jump to that session', async () => {
    seedSessionActivityForTests({
      running: [running('run-1', 'Notes'), running('run-2', 'Notes', '2026-09-26T10:20:00Z')],
    });
    renderRouted(<SessionActivityIndicator />);
    const trigger = screen.getByTestId('indicator-running');
    expect(trigger.textContent).toContain('2 running');
    fireEvent.pointerDown(trigger, { button: 0, pointerType: 'mouse' });
    const items = await screen.findAllByTestId('indicator-running-item');
    expect(items).toHaveLength(2);
    expect(items[0].textContent).toContain('run-1-dir · running · 27m');
    expect(items[1].textContent).toContain('run-2-dir · running · 7m');
    fireEvent.click(items[1]);
    await waitFor(() =>
      expect(screen.getByTestId('location').textContent).toBe('/pair?resumeSessionId=run-2')
    );
  });

  it('"Active now" leads the sidebar: waiting first, each row with its state, folder and a jump', () => {
    seedSessionActivityForTests({
      running: [running('run-1', 'Jira Migration Kickoff Notes')],
      needsYou: [question('ny_1', 'ask-1', 'Service setup', 'Which database?')],
    });
    renderRouted(<ActiveNowSection />);
    const section = screen.getByTestId('active-now-section');
    const rows = within(section).getAllByRole('button');
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
      'active-now-row-ask-1',
      'active-now-row-run-1',
    ]);
    expect(rows[0].getAttribute('data-state')).toBe('needs-you');
    expect(rows[0].getAttribute('aria-busy')).toBeNull();
    expect(rows[0].textContent).toContain('Which database?');
    expect(rows[1].getAttribute('data-state')).toBe('running');
    expect(rows[1].getAttribute('aria-busy')).toBe('true');
    expect(rows[1].textContent).toContain('Running · 27m');
    expect(rows[1].textContent).toContain('run-1-dir');
    fireEvent.click(rows[1]);
    expect(screen.getByTestId('location').textContent).toBe('/pair?resumeSessionId=run-1');
  });

  it('the card, the top bar and "Active now" use only classes main.css compiles, and no banned pattern', async () => {
    seedSessionActivityForTests({
      running: [running('run-1', 'Notes'), running('run-2', 'Notes')],
      needsYou: [
        { ...question('ny_1', 'ask-1', 'Service setup', 'Which database?'), options: ['A', 'B'] },
      ],
    });
    renderRouted(
      <>
        <SessionActivityIndicator />
        <ActiveNowSection />
        <NeedsYouTray sessionId="ask-1" chatState={ChatState.Idle} sendAnswer={vi.fn()} />
      </>
    );
    // Checked closed: an open Radix menu adds its own opacity-0 focus guards, which are not ours.
    assertStudioClean(document.body);
    // `no-drag` is the Electron drag-region rule from main.css, not a Tailwind utility.
    const classes = allClasses(document.body).filter(
      (c) => !c.startsWith('lucide') && c !== 'group' && c !== 'no-drag'
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('"Active now" is absent when nothing runs or waits', () => {
    seedSessionActivityForTests({
      failed: [
        { sessionId: 'f', sessionName: 'F', workingDir: '/x', failedAt: '2026-09-26T09:00:00Z' },
      ],
    });
    renderRouted(<ActiveNowSection />);
    expect(screen.queryByTestId('active-now-section')).toBeNull();
  });

  it('the session history leads with an Active now group and marks running, waiting and failed cards', async () => {
    Object.assign(window.electron, { getConfig: () => ({}) });
    const base = {
      workingDir: '/Users/me/api',
      messageCount: 3,
      updatedAt: '2026-09-25T10:00:00Z',
      lastMessageAt: '2026-09-25T10:00:00Z',
    };
    sessionsAcp.acpListSessions.mockResolvedValue({
      sessions: [
        { ...base, id: 'idle-1', name: 'Idle one', createdAt: '2026-09-20T10:00:00Z' },
        { ...base, id: 'run-1', name: 'Notes', createdAt: '2026-09-26T10:00:00Z' },
        { ...base, id: 'old-notes', name: 'Notes', createdAt: '2026-09-01T10:00:00Z' },
        { ...base, id: 'fail-1', name: 'Cut', createdAt: '2026-09-21T10:00:00Z' },
        { ...base, id: 'stop-1', name: 'Kickoff', createdAt: '2026-09-22T10:00:00Z' },
      ],
      nextCursor: null,
    });
    seedSessionActivityForTests({
      running: [running('run-1', 'Notes')],
      failed: [
        {
          sessionId: 'fail-1',
          sessionName: 'Cut',
          workingDir: '/Users/me/api',
          failedAt: '2026-09-25T10:00:00Z',
          reason: 'The split across your Macs stopped mid-answer',
        },
      ],
      stopped: [
        {
          sessionId: 'stop-1',
          sessionName: 'Kickoff',
          workingDir: '/Users/me/api',
          stoppedAt: '2026-09-25T10:00:00Z',
          elapsedMs: 372_000,
          outputTokens: 1_900,
        },
      ],
    });
    render(
      <IntlProvider locale="en" messages={{}}>
        <MemoryRouter>
          <SessionListView onSelectSession={vi.fn()} />
        </MemoryRouter>
      </IntlProvider>
    );
    const runCard = await screen.findByTestId('session-card-run-1');
    expect(runCard.getAttribute('data-state')).toBe('running');
    expect(runCard.getAttribute('aria-busy')).toBe('true');
    expect(within(runCard).getByTestId('session-running-pill').textContent).toBe('Running · 27m');
    // Same title, told apart by age: the older keeps the name, the newer is " · 2".
    expect(within(runCard).getByText('Notes · 2')).toBeTruthy();
    // The date groups arrive in a transition, after the Active now group.
    const oldNotes = await screen.findByTestId('session-card-old-notes');
    expect(within(oldNotes).getByText('Notes')).toBeTruthy();

    const failCard = screen.getByTestId('session-card-fail-1');
    expect(failCard.getAttribute('data-state')).toBe('failed');
    expect(within(failCard).getByTestId('session-failed-pill')).toBeTruthy();
    const stopCard = screen.getByTestId('session-card-stop-1');
    expect(stopCard.getAttribute('data-state')).toBe('stopped');
    expect(within(stopCard).getByTestId('session-stopped-pill').getAttribute('title')).toBe(
      'You stopped this answer after 6 min · 1.9k tokens'
    );
    expect(screen.getByTestId('session-card-idle-1').getAttribute('data-state')).toBe('idle');

    // The running card sits in the Active now group, above every date group.
    const headings = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
    expect(headings[0]).toBe('Active now');
  });
});

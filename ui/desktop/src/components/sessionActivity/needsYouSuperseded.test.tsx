import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter } from 'react-router-dom';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import SessionActivityIndicator from './SessionActivityIndicator';
import ActiveNowSection from './ActiveNowSection';
import { SessionActivityMarker } from './ActivityPills';
import NeedsYouTray from './NeedsYouCard';
import { glanceSessionsOf } from '../engineGlance/glanceStore';
import {
  getSessionActivitySnapshot,
  pickOptions,
  refreshSessionActivity,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivityStore';
import { ChatState } from '../../types/chatState';

// The critic's case (3.0.68, 09:08): the question the model asked, with the recommendation that
// begins with the first option's words and goes on with its reason.
const QUESTION = {
  id: 'ny_lead',
  sessionId: 'jira',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/Users/me/mihaiperdum',
  question: 'An inactive project lead — which decision does the script give them?',
  why: 'The stated rules conflict on exactly that case.',
  recommendedAnswer:
    'migrate — apply the lead override in every case. The "never skipped" rule is satisfied literally, and the reason column notes the lead so the check can confirm those 6 rows are migrate, never skip.',
  options: [
    'migrate — apply the lead override in every case',
    "migrate + special 'lead-inactive' marker so the review list catches exactly 6",
    "skip unless they have an email — a lead without an email can't be created in Cloud anyway",
  ],
  createdAt: '2026-09-28T09:08:00Z',
  status: 'open' as const,
};

function renderEveryNeedsYouSurface() {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <MemoryRouter>
        <SessionActivityIndicator />
        <ActiveNowSection />
        <div data-testid="sidebar-row">
          <SessionActivityMarker sessionId="jira" idle={<span>12m ago</span>} />
        </div>
        <NeedsYouTray sessionId="jira" chatState={ChatState.Idle} sendAnswer={vi.fn()} />
      </MemoryRouter>
    </IntlProvider>
  );
}

describe('Q-298: a question the person wrote past clears from every surface through the one store', () => {
  beforeEach(() => {
    acp.acpSessionActivity.mockReset();
    seedSessionActivityForTests({ needsYou: [QUESTION] });
  });
  afterEach(() => resetSessionActivityForTests());

  it('open: the top pill, Active now, the sidebar row, the engine card report and the chat card all say it', () => {
    renderEveryNeedsYouSurface();
    expect(screen.getByTestId('indicator-needs-you').textContent).toContain('1 needs you');
    expect(screen.getByTestId('active-now-row-jira').getAttribute('data-state')).toBe('needs-you');
    expect(screen.getByTestId('sidebar-row').textContent).toContain('Needs you');
    expect(glanceSessionsOf(getSessionActivitySnapshot()).needsYou).toEqual([
      { sessionId: 'jira', sessionName: 'Jira Migration Assessment', question: QUESTION.question },
    ]);
    expect(screen.getByTestId('needs-you-card')).toBeTruthy();
  });

  it('once the engine has closed it as superseded, all five clear on the next read', async () => {
    renderEveryNeedsYouSurface();
    // The engine's view after the person's message: the item is no longer open, so not listed.
    acp.acpSessionActivity.mockResolvedValue({
      running: [
        {
          sessionId: 'jira',
          sessionName: 'Jira Migration Assessment',
          workingDir: '/Users/me/mihaiperdum',
          startedAt: '2026-09-28T09:08:05Z',
        },
      ],
      needsYou: [],
      failed: [],
    });
    await act(() => refreshSessionActivity());

    expect(screen.queryByTestId('indicator-needs-you')).toBeNull();
    expect(screen.getByTestId('active-now-row-jira').getAttribute('data-state')).toBe('running');
    expect(screen.getByTestId('sidebar-row').textContent).not.toContain('Needs you');
    expect(glanceSessionsOf(getSessionActivitySnapshot()).needsYou).toEqual([]);
    expect(screen.queryByTestId('needs-you-card')).toBeNull();
    expect(screen.queryByTestId('needs-you-tray')).toBeNull();
  });
});

describe('Q-319: the recommended answer is not repeated as a pick', () => {
  afterEach(() => resetSessionActivityForTests());

  it('the critic’s card: the option the recommendation already is leaves the pick list', () => {
    seedSessionActivityForTests({ needsYou: [QUESTION] });
    renderEveryNeedsYouSurface();
    expect(screen.getByTestId('needs-you-recommended').textContent).toContain(
      'migrate — apply the lead override in every case. The "never skipped" rule'
    );
    expect(screen.getAllByTestId('needs-you-option').map((o) => o.textContent)).toEqual([
      QUESTION.options[1],
      QUESTION.options[2],
    ]);
  });

  it('the same words, the same words plus a reason, or only a shared first word', () => {
    const picks = (recommendedAnswer: string, options: string[]) =>
      pickOptions({ recommendedAnswer, options });
    expect(picks('PostgreSQL', ['PostgreSQL', 'SQLite'])).toEqual(['SQLite']);
    expect(picks('postgresql ', ['PostgreSQL', 'SQLite'])).toEqual(['SQLite']);
    expect(picks('PostgreSQL (it has the JSON types)', ['PostgreSQL', 'SQLite'])).toEqual([
      'SQLite',
    ]);
    expect(picks('A comma: the owner opens it in Excel', ['A comma', 'A tab'])).toEqual(['A tab']);
    // A recommendation that goes on with MORE choice is not the shorter option plus a reason.
    expect(picks('migrate + a marker', ['migrate', 'skip'])).toEqual(['migrate', 'skip']);
    expect(picks('SQLite3', ['SQLite', 'MySQL'])).toEqual(['SQLite', 'MySQL']);
    expect(picks('PostgreSQL', [])).toEqual([]);
  });

  it('when every option is the recommendation, there is no "Or pick" row at all', () => {
    seedSessionActivityForTests({
      needsYou: [{ ...QUESTION, recommendedAnswer: 'PostgreSQL', options: ['PostgreSQL'] }],
    });
    renderEveryNeedsYouSurface();
    expect(screen.queryAllByTestId('needs-you-option')).toEqual([]);
    expect(screen.queryByText('Or pick')).toBeNull();
  });
});

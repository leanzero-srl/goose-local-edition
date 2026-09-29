import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter } from 'react-router-dom';
import type { ReactNode } from 'react';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import ActiveNowSection from './ActiveNowSection';
import SessionActivityIndicator from './SessionActivityIndicator';
import { resetNowForTests } from './ActivityPills';
import {
  getSessionActivitySnapshot,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivityStore';
import { glanceSessionsOf } from '../engineGlance/glanceStore';
import { EngineGlanceCard } from '../engineGlance/EngineGlanceCard';
import { glancePush } from '../../utils/engineGlance.fixtures';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import { publishProjectPaths, resetPublishedProjectPaths } from '../../utils/projectNames';

/**
 * The owner's screenshot of 2026-09-29 (OWNER-2026-09-29/sidebar-active-now.png, vigil-0831.png):
 * two Active now rows crammed together (Q-483), one reading "mihaiperdum · Want /Users/mihaiperdum/
 * goose-builds/quality/RU-2026-…" (Q-484), two projects both "work" (Q-485), and "2 need you" in
 * the top bar beside "1 needs you" on the glance for the same one chat (Q-486).
 */

const NOW = Date.parse('2026-09-29T11:30:00Z');
const Q = '/Users/mihaiperdum/goose-builds/quality';
const WORK_3V = `${Q}/RU-2026-09-28-3v-split-tensor-cafe/work`;
const WORK_3W = `${Q}/RU-2026-09-29-3w-split-tensor-cafe/work`;
const RAW_QUESTION = `Want /Users/mihaiperdum/goose-builds/quality/RU-2026-09-28-3u-split-tensor/work/report.docx as the source, or the PDF?`;

function question(id: string, sessionId: string, createdAt: string, text = RAW_QUESTION) {
  return {
    id,
    sessionId,
    sessionName: 'Jira DC to Cloud migration assessment',
    workingDir: '/Users/mihaiperdum',
    question: text,
    why: 'w',
    recommendedAnswer: 'r',
    options: [],
    createdAt,
    status: 'open' as const,
  };
}

const running = (sessionId: string, sessionName: string, workingDir: string) => ({
  sessionId,
  sessionName,
  workingDir,
  startedAt: '2026-09-29T11:29:00Z',
});

function renderIn(node: ReactNode) {
  return render(
    <IntlProvider locale="en" messages={{}} timeZone="UTC">
      <MemoryRouter initialEntries={['/pair?resumeSessionId=bakery']}>{node}</MemoryRouter>
    </IntlProvider>
  );
}

describe('Active now rows — the owner screenshot of 2026-09-29', () => {
  beforeEach(() => {
    resetNowForTests(NOW);
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => {
    resetSessionActivityForTests();
    resetPublishedProjectPaths();
  });

  it('Q-483: rows sit apart, the title truncates before its badges and the badges wrap under it when narrow', () => {
    seedSessionActivityForTests({
      needsYou: [question('n1', 'jira', '2026-09-29T11:02:00Z')],
      running: [running('bakery', 'Bakery allergen menu site', WORK_3W)],
    });
    renderIn(<ActiveNowSection />);
    const list = screen.getByTestId('active-now-rows');
    // The tree's 1px gap let the open row's ring sit on its neighbour; the project list's own
    // rhythm is gap-px between 32px rows, so a two-line card needs a real gap.
    expect(list.className).toContain('gap-1.5');
    expect(list.className).not.toContain('gap-px');

    const bakery = screen.getByTestId('active-now-row-bakery');
    expect(bakery.getAttribute('aria-current')).toBe('true');
    expect(bakery.className).toContain('ring-inset');
    expect(bakery.className).toContain('py-2');

    for (const row of [bakery, screen.getByTestId('active-now-row-jira')]) {
      const title = within(row).getByTestId('active-now-title');
      for (const c of ['min-w-0', 'truncate', 'grow', 'basis-40'])
        expect(title.className).toContain(c);
      expect(title.className).not.toContain('flex-1');
      const line = title.parentElement!;
      expect(line.className).toContain('flex-wrap');
      const badges = within(row).getByTestId('active-now-badges');
      expect(badges.className).toContain('shrink-0');
      expect(badges.parentElement).toBe(line);
      expect(title.contains(badges)).toBe(false);
    }
  });

  it('Q-484: the subtitle says what the chat is doing, never an excerpt of its question', () => {
    seedSessionActivityForTests({
      needsYou: [
        question('n2', 'jira', '2026-09-29T11:05:00Z', 'Second question?'),
        question('n1', 'jira', '2026-09-29T11:02:00Z'),
      ],
      running: [running('bakery', 'Bakery allergen menu site', WORK_3W)],
    });
    renderIn(<ActiveNowSection />);
    const jira = screen.getByTestId('active-now-row-jira');
    const detail = within(jira).getByTestId('active-now-detail').textContent ?? '';
    expect(detail).toBe('mihaiperdum · waiting for your answer since 11:02 AM');
    expect(detail).not.toContain('/Users/');
    expect(detail).not.toContain('Want');
    expect(jira.getAttribute('title')).not.toContain('Want /Users');
    expect(within(jira).getByTestId('session-needs-you-pill').textContent).toBe('Needs you · 2');

    const bakery = within(screen.getByTestId('active-now-row-bakery'));
    expect(bakery.getByTestId('active-now-detail').textContent).toBe('work · started 11:29 AM');
  });

  it('Q-484: a live elicitation (no time of its own) says it waits, without a time', () => {
    seedSessionActivityForTests({
      running: [running('pick', 'Setup', '/p/setup')],
      elicitations: [
        {
          id: 'e1',
          sessionId: 'pick',
          request: { message: 'Pick /Users/me/secret-folder' },
        } as never,
      ],
    });
    renderIn(<ActiveNowSection />);
    expect(screen.getByTestId('active-now-detail').textContent).toBe(
      'setup · waiting for your answer'
    );
  });

  it('Q-485: two same-named folders in Active now read by what tells them apart', () => {
    seedSessionActivityForTests({
      running: [
        running('bakery', 'Bakery allergen menu site', WORK_3W),
        { ...running('vendor', 'Vendor tomllib', WORK_3V), startedAt: '2026-09-29T11:10:00Z' },
      ],
    });
    renderIn(<ActiveNowSection />);
    expect(
      within(screen.getByTestId('active-now-row-bakery')).getByTestId('active-now-detail')
        .textContent
    ).toBe('work — RU-…-29-3w-… · started 11:29 AM');
    expect(
      within(screen.getByTestId('active-now-row-vendor')).getByTestId('active-now-detail')
        .textContent
    ).toBe('work — RU-…-28-3v-… · started 11:10 AM');
  });

  it('Q-485: one "work" in Active now is told apart against the Projects list the sidebar published', () => {
    publishProjectPaths([WORK_3V, WORK_3W, '/Users/mihaiperdum']);
    seedSessionActivityForTests({
      running: [running('bakery', 'Bakery allergen menu site', WORK_3W)],
    });
    renderIn(<ActiveNowSection />);
    expect(screen.getByTestId('active-now-detail').textContent).toBe(
      'work — RU-…-29-3w-… · started 11:29 AM'
    );
  });
});

describe('Q-486: one chat asking two questions is counted the same way on every surface', () => {
  beforeEach(() => {
    resetNowForTests(NOW);
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => resetSessionActivityForTests());

  const twoInOne = () =>
    seedSessionActivityForTests({
      needsYou: [
        question('n1', 'jira', '2026-09-29T11:02:00Z'),
        question('n2', 'jira', '2026-09-29T11:05:00Z', 'Second question?'),
      ],
    });

  it('the top bar names what it counts: "2 questions in 1 chat", not "2 need you"', () => {
    twoInOne();
    renderIn(<SessionActivityIndicator />);
    const pill = screen.getByTestId('indicator-needs-you');
    expect(pill.getAttribute('aria-label')).toBe('2 questions in 1 chat');
    expect(screen.getByTestId('indicator-needs-you-words').textContent).toBe(
      '2 questions in 1 chat'
    );
  });

  it('the glance reports every question, so its card says the same words as the top bar', () => {
    twoInOne();
    const sessions = glanceSessionsOf(getSessionActivitySnapshot());
    expect(sessions.needsYou.map((n) => [n.sessionId, n.question])).toEqual([
      ['jira', RAW_QUESTION],
      ['jira', 'Second question?'],
    ]);
    const push = { ...glancePush({ ...INITIAL_SNAPSHOT, mode: 'off' }), sessions };
    render(
      <IntlProvider locale="en" messages={{}}>
        <EngineGlanceCard
          push={push}
          variant="dock"
          collapsed={false}
          expanded={false}
          onCollapsedChange={vi.fn()}
          onToggleExpanded={vi.fn()}
          onOpenEngine={vi.fn()}
          onOpenSession={vi.fn()}
        />
      </IntlProvider>
    );
    expect(screen.getByTestId('engine-glance').textContent).toContain('2 questions in 1 chat');
    expect(screen.getByTestId('engine-glance').textContent).not.toContain('1 needs you');
  });

  it('one question per chat keeps the short words everywhere: "2 need you"', () => {
    seedSessionActivityForTests({
      needsYou: [
        question('n1', 'jira', '2026-09-29T11:02:00Z'),
        { ...question('n2', 'bake', '2026-09-29T11:05:00Z'), sessionName: 'Bakery' },
      ],
    });
    renderIn(<SessionActivityIndicator />);
    expect(screen.getByTestId('indicator-needs-you').getAttribute('aria-label')).toBe('2 need you');
  });
});

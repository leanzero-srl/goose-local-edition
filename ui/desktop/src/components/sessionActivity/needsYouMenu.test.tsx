import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter } from 'react-router-dom';
import type { ReactNode } from 'react';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import SessionActivityIndicator from './SessionActivityIndicator';
import { resetNowForTests } from './ActivityPills';
import { questionGist } from './needsYouWords';
import {
  getSessionActivitySnapshot,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivityStore';
import { glanceSessionsOf } from '../engineGlance/glanceStore';
import { EngineGlanceCard } from '../engineGlance/EngineGlanceCard';
import { glancePush } from '../../utils/engineGlance.fixtures';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import { resetPublishedProjectPaths } from '../../utils/projectNames';

/**
 * Q-489: the top bar's "Waiting for you" menu (the "2 need you" pill) listed each chat by its raw
 * question — a paragraph per item, paths and all — where Active now says the chat's name and what
 * it waits on. The menu now says the same words: name · folder · "waiting for your answer since
 * HH:MM", and the question's first sentence on one line, the whole question as its tooltip.
 */

const NOW = Date.parse('2026-09-29T11:30:00Z');
const Q = '/Users/mihaiperdum/goose-builds/quality';
const WORK_3V = `${Q}/RU-2026-09-28-3v-split-tensor-cafe/work`;
const WORK_3W = `${Q}/RU-2026-09-29-3w-split-tensor-cafe/work`;
const FIRST_SENTENCE = `Want ${Q}/RU-2026-09-28-3u-split-tensor/work/report.docx as the source, or the PDF?`;
const PARAGRAPH = `${FIRST_SENTENCE} The docx carries Tuesday's tracked changes, the PDF is the signed copy; I can read either, not both in one pass.\n\nIf you pick the PDF I will re-derive the tables from its text layer.`;

function question(
  id: string,
  sessionId: string,
  sessionName: string,
  workingDir: string,
  createdAt: string,
  text: string
) {
  return {
    id,
    sessionId,
    sessionName,
    workingDir,
    question: text,
    why: 'w',
    recommendedAnswer: 'r',
    options: [],
    createdAt,
    status: 'open' as const,
  };
}

function renderIn(node: ReactNode) {
  return render(
    <IntlProvider locale="en" messages={{}} timeZone="UTC">
      <MemoryRouter initialEntries={['/']}>{node}</MemoryRouter>
    </IntlProvider>
  );
}

describe('Q-489: the top bar\'s "Waiting for you" menu is a list, not paragraphs', () => {
  beforeEach(() => {
    resetNowForTests(NOW);
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => {
    resetSessionActivityForTests();
    resetPublishedProjectPaths();
  });

  it('each item is the chat name, the Active now state line, and the question gist with the whole question as its tooltip', async () => {
    seedSessionActivityForTests({
      needsYou: [
        // Listed first, asked later: "since" dates the OLDEST question, so the gist beside it
        // must be that question's, not this one's.
        question(
          'n2',
          'jira',
          'Jira DC to Cloud migration',
          WORK_3V,
          '2026-09-29T11:05:00Z',
          'Also: keep the Confluence links?'
        ),
        question(
          'n1',
          'jira',
          'Jira DC to Cloud migration',
          WORK_3V,
          '2026-09-29T11:02:00Z',
          PARAGRAPH
        ),
        question(
          'n3',
          'bake',
          'Bakery allergen menu site',
          WORK_3W,
          '2026-09-29T11:12:00Z',
          'Which allergens go on the card?\nThe EU list has 14.'
        ),
      ],
    });
    renderIn(<SessionActivityIndicator />);
    const trigger = screen.getByTestId('indicator-needs-you');
    expect(trigger.getAttribute('aria-label')).toBe('3 questions in 2 chats');
    fireEvent.pointerDown(trigger, { button: 0, pointerType: 'mouse' });
    const items = await screen.findAllByTestId('indicator-needs-you-item');
    expect(items).toHaveLength(2);
    // What the old menu showed: the paragraph, its later sentences and the chat's other question.
    expect(items[0].textContent).not.toContain('tracked changes');
    expect(items[0].textContent).not.toContain('Confluence');
    expect(items[1].textContent).not.toContain('The EU list');

    const jira = within(items[0]);
    expect(jira.getByTestId('indicator-needs-you-item-name').textContent).toBe(
      'Jira DC to Cloud migration'
    );
    // The same words Active now says (activeRowDetail), with the same-named folders told apart.
    expect(jira.getByTestId('indicator-needs-you-item-detail').textContent).toBe(
      'work — RU-…-28-3v-… · waiting for your answer since 11:02 AM'
    );
    const gist = jira.getByTestId('indicator-needs-you-item-question');
    expect(gist.textContent).toBe(FIRST_SENTENCE);
    expect(gist.getAttribute('title')).toBe(PARAGRAPH);
    expect(gist.className).toContain('truncate');

    const bake = within(items[1]);
    expect(bake.getByTestId('indicator-needs-you-item-name').textContent).toBe(
      'Bakery allergen menu site'
    );
    expect(bake.getByTestId('indicator-needs-you-item-detail').textContent).toBe(
      'work — RU-…-29-3w-… · waiting for your answer since 11:12 AM'
    );
    expect(bake.getByTestId('indicator-needs-you-item-question').textContent).toBe(
      'Which allergens go on the card?'
    );
  });

  it('a lone waiting chat keeps the jump, and its tooltip carries the same words and the whole question', () => {
    seedSessionActivityForTests({
      needsYou: [
        question(
          'n1',
          'jira',
          'Jira DC to Cloud migration',
          WORK_3V,
          '2026-09-29T11:02:00Z',
          PARAGRAPH
        ),
      ],
    });
    renderIn(<SessionActivityIndicator />);
    expect(screen.getByTestId('indicator-needs-you').getAttribute('title')).toBe(
      `Jira DC to Cloud migration — work · waiting for your answer since 11:02 AM\n${PARAGRAPH}`
    );
  });

  it('the glance names a lone question by its gist, the whole question as its tooltip', () => {
    seedSessionActivityForTests({
      needsYou: [
        question(
          'n1',
          'jira',
          'Jira DC to Cloud migration',
          WORK_3V,
          '2026-09-29T11:02:00Z',
          PARAGRAPH
        ),
      ],
    });
    const sessions = glanceSessionsOf(getSessionActivitySnapshot());
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
    const gist = screen.getByTestId('engine-glance-question-gist');
    expect(gist.textContent).toBe(FIRST_SENTENCE);
    expect(gist.getAttribute('title')).toBe(PARAGRAPH);
    expect(screen.getByTestId('engine-glance-question').textContent).not.toContain(
      'tracked changes'
    );
  });
});

describe('questionGist — the one-line form of a question', () => {
  it.each([
    [PARAGRAPH, FIRST_SENTENCE],
    ['Ship 3.0.78 from report.docx now', 'Ship 3.0.78 from report.docx now'],
    ['Which one:\n1. Postgres\n2. SQLite', 'Which one:'],
    ['Pick one; the other waits.', 'Pick one'],
    ['  Which   database   should  I use?  ', 'Which database should I use?'],
    ['Done!', 'Done!'],
    ['', ''],
  ])('%j → %j', (input, gist) => {
    expect(questionGist(input)).toBe(gist);
  });

  it('reads only the head of a pasted page', () => {
    expect(questionGist('x'.repeat(100_000)).length).toBeLessThanOrEqual(600);
  });
});

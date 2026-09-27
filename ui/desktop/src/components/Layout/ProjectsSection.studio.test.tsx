import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { ProjectsSection } from './ProjectsSection';
import { acpListSessions, type SessionListItem } from '../../acp/sessions';
import { startNewSession } from '../../sessions';
import { SURFACE } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import { contrast, resolvedPaint, studioToken } from '../lz/resolvedPaint';
import { resetNowForTests } from '../sessionActivity/ActivityPills';
import {
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from '../sessionActivity/sessionActivityStore';

/**
 * The Projects tree in the Studio register (ui/desktop/DESIGN.md): a SectionHeader that counts
 * the rows the body shows, a ghost "+" whose glyph is the accent, hairline indent guides that are
 * separate elements (never a border-left), dense 32px rows, and the current session marked by the
 * inset ring alone. Behaviour is pinned by ProjectsSection.test.tsx; this file
 * pins the look and compiles every emitted class against main.css.
 */

const navMocks = vi.hoisted(() => ({
  recentSessions: { current: [] as unknown[] },
  activeSessionId: { current: undefined as string | undefined },
  fetchSessions: vi.fn(),
  handleSessionClick: vi.fn(),
}));

vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ extensionsList: [] }),
}));

vi.mock('../../sessions', () => ({
  startNewSession: vi.fn(),
  displaySessionListName: (name: string | null | undefined) =>
    !name || name === 'New Chat' ? 'New Session' : name,
}));

vi.mock('../../acp/sessions', () => ({
  acpListSessions: vi.fn(),
  acpDeleteSession: vi.fn(),
}));

vi.mock('../../hooks/useNavigationSessions', () => ({
  sessionToListItem: (s: Record<string, unknown>) => s,
  useNavigationSessions: () => ({
    recentSessions: navMocks.recentSessions.current,
    activeSessionId: navMocks.activeSessionId.current,
    fetchSessions: navMocks.fetchSessions,
    handleNavClick: vi.fn(),
    handleSessionClick: navMocks.handleSessionClick,
  }),
}));

function listItem(overrides: Partial<SessionListItem> = {}): SessionListItem {
  return {
    id: 'sess-1',
    name: 'Fix the panel',
    workingDir: '/proj/goose',
    updatedAt: new Date().toISOString(),
    messageCount: 4,
    createdAt: new Date().toISOString(),
    ...overrides,
  };
}

function electronMocks(projects: Array<{ path: string; addedAt: number }>) {
  Object.assign(window.electron, {
    listProjects: vi.fn().mockResolvedValue(projects),
    addProject: vi.fn().mockResolvedValue([]),
    removeProject: vi.fn().mockResolvedValue([]),
    directoryChooser: vi.fn().mockResolvedValue({ canceled: true, filePaths: [] }),
    revealInFinder: vi.fn().mockResolvedValue(true),
  });
}

const renderSection = () =>
  render(
    <MemoryRouter>
      <IntlProvider locale="en" messages={{}}>
        <ProjectsSection />
      </IntlProvider>
    </MemoryRouter>
  );

describe('ProjectsSection (Studio look)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navMocks.recentSessions.current = [];
    navMocks.activeSessionId.current = undefined;
    vi.mocked(acpListSessions).mockResolvedValue({ sessions: [], nextCursor: null });
    vi.mocked(startNewSession).mockResolvedValue(undefined as never);
  });

  it('the header is a SectionHeader whose count is the project rows the body shows, with a ghost "+" in the accent', async () => {
    electronMocks([
      { path: '/proj/goose', addedAt: 1 },
      { path: '/proj/other', addedAt: 2 },
    ]);
    renderSection();
    await screen.findByText('goose');
    const header = screen.getByTestId('lz-section-header');
    expect(within(header).getByText('Projects').className).toContain('uppercase');
    expect(screen.getByTestId('lz-section-count').textContent).toBe('2');
    const add = screen.getByLabelText('Add a project folder');
    expect(add.dataset.variant).toBe('ghost');
    expect(add.querySelector('svg')?.getAttribute('class')).toContain('text-lz-accent');
    expect(add.getAttribute('style')).toBeNull();
  });

  it('project rows are dense 32px rows with a plain folder glyph — no coloured square, no hand-written hue', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    renderSection();
    const row = (await screen.findByText('goose')).closest('button');
    expect(row).not.toBeNull();
    expect(row?.className).toContain('h-lz-row-dense');
    expect(row?.className).toContain('rounded-lz-control');
    expect(row?.className).toContain(SURFACE.hover);
    expect(row?.querySelector('[style]')).toBeNull();
    expect(screen.getByText('goose').className).toContain('font-lz-medium');
  });

  it('an expanded project draws a hairline guide beside 32px session rows; the current session carries the inset ring and NO dot', async () => {
    electronMocks([]);
    navMocks.activeSessionId.current = 'sess-1';
    navMocks.recentSessions.current = [
      listItem(),
      listItem({ id: 'sess-2', name: 'Ship the tree' }),
    ];
    renderSection();
    await screen.findByText('Fix the panel');

    const guide = screen.getByTestId('tree-guide');
    expect(guide.className).toContain('bg-lz-border');
    expect(guide.className).toContain('w-px');
    expect(guide.className).not.toMatch(/border-l/);

    const current = screen.getByText('Fix the panel').closest('button') as HTMLElement;
    expect(current.className).toContain('h-lz-row-dense');
    for (const c of SURFACE.selectedRing.split(' ')) expect(current.className).toContain(c);
    expect(current.className).not.toContain('bg-lz-accent');
    // A dot on the open row read as "running" (critic round 2026-09-26 row 5): the ring alone
    // marks it, and an idle open row carries no state pill either.
    expect(within(current).queryByRole('img')).toBeNull();
    expect(current.getAttribute('data-state')).toBe('idle');
    expect(within(current).queryByTestId('session-running-pill')).toBeNull();

    const other = screen.getByText('Ship the tree').closest('button') as HTMLElement;
    expect(within(other).queryByRole('img')).toBeNull();
    expect(other.className).toContain(SURFACE.hover);
    expect(other.className).not.toContain('ring-lz-accent');

    // The current session's ring is the accent and its label stays ink on the sidebar surface in
    // both themes, at rest and under the pointer (the neutral hover step) — never white on light.
    const label = screen.getByText('Fix the panel');
    for (const theme of ['light', 'dark'] as const) {
      const surface = studioToken('--color-lz-surface', theme);
      const rest = await resolvedPaint(current, theme, { inherit: { bg: surface } });
      expect(rest.missing).toEqual([]);
      expect(rest.ring).toBe(studioToken('--color-lz-accent', theme));
      const ink = await resolvedPaint(label, theme, { inherit: { bg: rest.bg ?? surface } });
      expect(contrast(ink.bg, ink.text)).toBeGreaterThan(4.5);
      const hovered = await resolvedPaint(current, theme, {
        hover: true,
        inherit: { bg: surface },
      });
      const hoveredInk = await resolvedPaint(label, theme, {
        inherit: { bg: hovered.bg ?? surface },
      });
      expect(contrast(hoveredInk.bg, hoveredInk.text)).toBeGreaterThan(4.5);
    }
  }, 30_000);

  it('row actions are ghost Buttons hidden by visibility until hover or focus — never an opacity', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    renderSection();
    const plus = await screen.findByLabelText('New session here — goose');
    expect(plus.dataset.variant).toBe('ghost');
    expect(plus.className).toContain('invisible');
    expect(plus.className).toContain('group-hover:visible');
    expect(plus.className).toContain('group-focus-within:visible');
    expect(plus.className).not.toMatch(/opacity/);
    const more = screen.getByLabelText('Project actions');
    expect(more.dataset.variant).toBe('ghost');
    fireEvent.click(more);
    expect(screen.getByLabelText('Project actions').className).toContain('visible');
  });

  it('the context menu is the overlay surface; remove reads in the err tone and confirms as the err fill; no native control', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    renderSection();
    fireEvent.contextMenu(await screen.findByText('goose'));
    const menu = screen.getByTestId('project-context-menu');
    for (const c of SURFACE.overlay.split(' ')) expect(menu.className).toContain(c);
    const remove = screen.getByText('Remove from projects').closest('button') as HTMLElement;
    expect(remove.className).toContain('text-lz-err');
    expect(remove.getAttribute('style')).toBeNull();
    fireEvent.click(remove);
    const confirm = screen
      .getByText('Confirm remove (keeps files & sessions)')
      .closest('button') as HTMLElement;
    expect(confirm.className).toContain('bg-lz-err-solid');
    expect(confirm.getAttribute('style')).toBeNull();
    assertStudioClean(document.body);
  });

  it('the failure twin and the paging row keep the register: err meta plus a ghost Retry, a ghost More', async () => {
    electronMocks([]);
    navMocks.recentSessions.current = Array.from({ length: 5 }, (_, i) =>
      listItem({ id: `s${i}`, name: `Session ${i}` })
    );
    vi.mocked(acpListSessions)
      .mockRejectedValueOnce(new Error('agent down'))
      .mockResolvedValueOnce({ sessions: [listItem()], nextCursor: 'cursor-1' });
    renderSection();
    fireEvent.click(await screen.findByText('Show more'));
    const failed = await screen.findByText("Couldn't load sessions");
    expect(failed.className).toContain('text-lz-err');
    const retry = screen.getByText('Retry').closest('button') as HTMLElement;
    expect(retry.dataset.variant).toBe('ghost');
    fireEvent.click(retry);
    await screen.findByText('Fix the panel');
    const more = screen.getByText('Show more').closest('button') as HTMLElement;
    expect(more.dataset.variant).toBe('ghost');
    expect(more.getAttribute('style')).toBeNull();
  });

  it('with the tree, a paged folder and the menu all open: no banned pattern, and every class compiles against main.css', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    navMocks.activeSessionId.current = 'loose';
    navMocks.recentSessions.current = [
      listItem({ id: 'loose', name: 'Loose chat', workingDir: '/elsewhere' }),
      ...Array.from({ length: 5 }, (_, i) => listItem({ id: `s${i}`, name: `Session ${i}` })),
    ];
    vi.mocked(acpListSessions).mockResolvedValue({
      sessions: [listItem()],
      nextCursor: 'cursor-1',
    });
    renderSection();
    await screen.findByText('Loose chat');
    fireEvent.click(await screen.findByText('Show more'));
    await screen.findByText('Fix the panel');
    fireEvent.contextMenu(screen.getByText('goose'));
    screen.getByTestId('project-context-menu');

    assertStudioClean(document.body);
    const classes = allClasses(document.body).filter(
      (c) => !c.startsWith('lucide') && c !== 'group'
    );
    expect(classes.length).toBeGreaterThan(40);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('running, waiting and failed sessions carry their state on the row, lead their folder past the preview, and compile', async () => {
    resetNowForTests(Date.parse('2026-09-26T10:27:00Z'));
    electronMocks([]);
    // Fixed instants, the OPEN chat (idle-0) the oldest: listItem's new Date() gave the six rows different
    // milliseconds on a slow CI runner, idle-0 sorted last and fell under "Show more" — which is also what a user
    // saw, so the open chat is now kept in view and this pins it.
    const idle = Array.from({ length: 6 }, (_, i) =>
      listItem({
        id: `idle-${i}`,
        name: `Idle ${i}`,
        updatedAt: `2026-09-26T08:0${i}:00Z`,
        createdAt: `2026-09-26T08:0${i}:00Z`,
      })
    );
    navMocks.activeSessionId.current = 'idle-0';
    navMocks.recentSessions.current = [
      ...idle,
      listItem({
        id: 'run-1',
        name: 'Jira Migration Kickoff Notes',
        createdAt: '2026-09-26T09:00:00Z',
      }),
      listItem({
        id: 'old-1',
        name: 'Jira Migration Kickoff Notes',
        createdAt: '2026-09-20T09:00:00Z',
      }),
      listItem({ id: 'wait-1', name: 'Service setup' }),
      listItem({ id: 'fail-1', name: 'Cut short' }),
    ];
    seedSessionActivityForTests({
      running: [
        {
          sessionId: 'run-1',
          sessionName: 'Jira Migration Kickoff Notes',
          workingDir: '/proj/goose',
          startedAt: '2026-09-26T10:00:00Z',
        },
      ],
      needsYou: [
        {
          id: 'ny_1',
          sessionId: 'wait-1',
          sessionName: 'Service setup',
          workingDir: '/proj/goose',
          question: 'Which database?',
          why: 'w',
          recommendedAnswer: 'r',
          options: [],
          createdAt: '2026-09-26T10:00:00Z',
          status: 'open',
        },
      ],
      failed: [
        {
          sessionId: 'fail-1',
          sessionName: 'Cut short',
          workingDir: '/proj/goose',
          failedAt: '2026-09-26T02:00:00Z',
          reason: 'The split across your Macs stopped mid-answer',
        },
      ],
    });
    try {
      renderSection();
      const runRow = await screen.findByTestId('session-row-run-1');
      expect(runRow.getAttribute('data-state')).toBe('running');
      expect(runRow.getAttribute('aria-busy')).toBe('true');
      expect(within(runRow).getByTestId('session-running-pill').textContent).toBe('Running · 27m');
      // Same title in one folder: the newer one is " · 2", and the running one is the newer.
      expect(within(runRow).getByText('Jira Migration Kickoff Notes · 2')).toBeTruthy();

      const waitRow = screen.getByTestId('session-row-wait-1');
      expect(waitRow.getAttribute('data-state')).toBe('needs-you');
      expect(waitRow.getAttribute('aria-busy')).toBeNull();
      expect(within(waitRow).getByTestId('session-needs-you-pill')).toBeTruthy();

      // The open idle row: ring only, no dot, no pill.
      const openRow = screen.getByTestId('session-row-idle-0');
      expect(openRow.getAttribute('data-state')).toBe('idle');
      expect(within(openRow).queryByRole('img')).toBeNull();

      // Active sessions lead the folder, ahead of the preview cut the idle ones fall under.
      const order = screen
        .getAllByTestId(/^session-row-/)
        .map((row) => row.getAttribute('data-testid'));
      expect(order.slice(0, 2).sort()).toEqual(['session-row-run-1', 'session-row-wait-1']);

      fireEvent.click(await screen.findByText('Show more'));
      const failRow = await screen.findByTestId('session-row-fail-1');
      expect(failRow.getAttribute('data-state')).toBe('failed');
      expect(within(failRow).getByTestId('session-failed-pill').getAttribute('title')).toContain(
        'stopped mid-answer'
      );

      assertStudioClean(document.body);
      const classes = allClasses(document.body).filter(
        (c) => !c.startsWith('lucide') && c !== 'group'
      );
      expect(await missingUtilities(classes)).toEqual([]);
    } finally {
      resetSessionActivityForTests();
    }
  }, 30_000);
});

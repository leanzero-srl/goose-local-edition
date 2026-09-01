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

/**
 * The Projects tree in the Studio register (ui/desktop/DESIGN.md): a SectionHeader that counts
 * the rows the body shows, a ghost "+" whose glyph is the accent, hairline indent guides that are
 * separate elements (never a border-left), dense 32px rows, and the current session marked by an
 * accent StatusDot plus the inset ring. Behaviour is pinned by ProjectsSection.test.tsx; this file
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

  it('an expanded project draws a hairline guide beside 32px session rows; the current session carries the accent dot and the inset ring', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    navMocks.activeSessionId.current = 'sess-1';
    vi.mocked(acpListSessions).mockResolvedValue({
      sessions: [listItem(), listItem({ id: 'sess-2', name: 'Ship the tree' })],
      nextCursor: null,
    });
    renderSection();
    fireEvent.click(await screen.findByText('goose'));
    await screen.findByText('Fix the panel');

    const guide = screen.getByTestId('tree-guide');
    expect(guide.className).toContain('bg-lz-border');
    expect(guide.className).toContain('w-px');
    expect(guide.className).not.toMatch(/border-l/);

    const current = screen.getByText('Fix the panel').closest('button') as HTMLElement;
    expect(current.className).toContain('h-lz-row-dense');
    for (const c of SURFACE.selectedRing.split(' ')) expect(current.className).toContain(c);
    expect(current.className).not.toContain('bg-lz-accent');
    const dot = within(current).getByRole('img', { name: 'Current session' });
    expect(dot.className).toContain('bg-lz-accent');
    expect(dot.getAttribute('data-live')).toBeNull();

    const other = screen.getByText('Ship the tree').closest('button') as HTMLElement;
    expect(within(other).queryByRole('img')).toBeNull();
    expect(other.className).toContain(SURFACE.hover);
    expect(other.className).not.toContain('ring-lz-accent');
  });

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
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    vi.mocked(acpListSessions)
      .mockRejectedValueOnce(new Error('agent down'))
      .mockResolvedValueOnce({ sessions: [listItem()], nextCursor: 'cursor-1' });
    renderSection();
    fireEvent.click(await screen.findByText('goose'));
    const failed = await screen.findByText("Couldn't load sessions");
    expect(failed.className).toContain('text-lz-err');
    const retry = screen.getByText('Retry').closest('button') as HTMLElement;
    expect(retry.dataset.variant).toBe('ghost');
    fireEvent.click(retry);
    await screen.findByText('Fix the panel');
    const more = screen.getByText('More sessions…').closest('button') as HTMLElement;
    expect(more.dataset.variant).toBe('ghost');
    expect(more.getAttribute('style')).toBeNull();
  });

  it('with the tree, Unfiled and the menu all open: no banned pattern, and every class compiles against main.css', async () => {
    electronMocks([{ path: '/proj/goose', addedAt: 1 }]);
    navMocks.activeSessionId.current = 'loose';
    navMocks.recentSessions.current = [
      listItem({ id: 'loose', name: 'Loose chat', workingDir: '/elsewhere' }),
    ];
    vi.mocked(acpListSessions).mockResolvedValue({
      sessions: [listItem()],
      nextCursor: 'cursor-1',
    });
    renderSection();
    fireEvent.click(await screen.findByText('goose'));
    await screen.findByText('Fix the panel');
    fireEvent.click(screen.getByText('Unfiled'));
    await screen.findByText('Loose chat');
    fireEvent.contextMenu(screen.getByText('goose'));
    screen.getByTestId('project-context-menu');

    assertStudioClean(document.body);
    const classes = allClasses(document.body).filter(
      (c) => !c.startsWith('lucide') && c !== 'group'
    );
    expect(classes.length).toBeGreaterThan(40);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

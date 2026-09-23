import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import {
  ProjectsSection,
  deriveProjects,
  filterProjects,
  normalizeDirPath,
  PREVIEW_COUNT,
} from './ProjectsSection';
import { acpListSessions, type SessionListItem } from '../../acp/sessions';
import { startNewSession } from '../../sessions';
import { AppEvents } from '../../constants/events';

/**
 * The Projects tree: folders DERIVED from where sessions ran, each showing its sessions newest
 * first with "Show more" behind the preview, no "Unfiled" bucket, the "+" registry only adding an
 * empty folder to start from. Every claim here is one the sidebar makes to the user.
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
  useNavigationSessions: () => ({
    recentSessions: navMocks.recentSessions.current,
    activeSessionId: navMocks.activeSessionId.current,
    fetchSessions: navMocks.fetchSessions,
    handleNavClick: vi.fn(),
    handleSessionClick: navMocks.handleSessionClick,
  }),
}));

const at = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();

function listItem(overrides: Partial<SessionListItem> = {}): SessionListItem {
  return {
    id: 'sess-1',
    name: 'Fix the panel',
    workingDir: '/proj/goose',
    updatedAt: at(1),
    messageCount: 4,
    createdAt: at(1),
    ...overrides,
  };
}

interface ElectronProjectMocks {
  listProjects: ReturnType<typeof vi.fn>;
  addProject: ReturnType<typeof vi.fn>;
  removeProject: ReturnType<typeof vi.fn>;
  directoryChooser: ReturnType<typeof vi.fn>;
  revealInFinder: ReturnType<typeof vi.fn>;
}

function electronMocks(): ElectronProjectMocks {
  const mocks: ElectronProjectMocks = {
    listProjects: vi.fn().mockResolvedValue([]),
    addProject: vi.fn().mockResolvedValue([]),
    removeProject: vi.fn().mockResolvedValue([]),
    directoryChooser: vi.fn().mockResolvedValue({ canceled: true, filePaths: [] }),
    revealInFinder: vi.fn().mockResolvedValue(true),
  };
  Object.assign(window.electron, mocks);
  return mocks;
}

const renderSection = () =>
  render(
    <MemoryRouter>
      <IntlProvider locale="en" messages={{}}>
        <ProjectsSection />
      </IntlProvider>
    </MemoryRouter>
  );

describe('ProjectsSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navMocks.recentSessions.current = [];
    navMocks.activeSessionId.current = undefined;
    vi.mocked(acpListSessions).mockResolvedValue({ sessions: [], nextCursor: null });
    vi.mocked(startNewSession).mockResolvedValue(undefined as never);
  });

  it('with no sessions and no folders it says where sessions will appear', async () => {
    electronMocks();
    renderSection();
    expect(
      await screen.findByText(/Your sessions appear here under the folder/)
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Add a project folder')).toBeInTheDocument();
    expect(screen.queryByText('Unfiled')).not.toBeInTheDocument();
  });

  it('groups sessions by their working directory — one folder row each, sessions under it, newest folder first', async () => {
    electronMocks();
    navMocks.recentSessions.current = [
      listItem({ id: 'a', name: 'Older goose work', workingDir: '/proj/goose', updatedAt: at(50) }),
      listItem({ id: 'b', name: 'Forge fix', workingDir: '/proj/lz-ppm-forge/', updatedAt: at(5) }),
      listItem({ id: 'c', name: 'Newest goose work', workingDir: '/proj/goose', updatedAt: at(2) }),
    ];
    renderSection();

    // The section header's fold controls are expanded buttons too; the folder rows are the ones under it.
    const rows = (await screen.findAllByRole('button', { expanded: true })).filter(
      (r) => !r.closest('[data-testid="lz-section-header"]')
    );
    expect(rows.map((r) => r.textContent)).toEqual(['goose', 'lz-ppm-forge']);
    expect(screen.queryByText('Unfiled')).not.toBeInTheDocument();

    const goose = screen.getByTestId('project-row-/proj/goose');
    const names = within(goose)
      .getAllByRole('button')
      .map((b) => b.textContent)
      .filter((t) => /goose work/.test(t ?? ''));
    expect(names[0]).toMatch(/Newest goose work/);
    expect(names[1]).toMatch(/Older goose work/);
    expect(
      within(screen.getByTestId('project-row-/proj/lz-ppm-forge')).getByText('Forge fix')
    ).toBeInTheDocument();
    expect(acpListSessions).not.toHaveBeenCalled();
  });

  it('previews the newest sessions, "Show more" reveals the rest, then pages the server with the exact cwd', async () => {
    electronMocks();
    navMocks.recentSessions.current = Array.from({ length: PREVIEW_COUNT + 2 }, (_, i) =>
      listItem({ id: `s${i}`, name: `Session ${i}`, updatedAt: at(i + 1) })
    );
    vi.mocked(acpListSessions)
      .mockResolvedValueOnce({
        sessions: [listItem({ id: 'old-1', name: 'Paged from the server', updatedAt: at(999) })],
        nextCursor: 'cursor-1',
      })
      .mockResolvedValueOnce({
        sessions: [listItem({ id: 'old-2', name: 'Even older', updatedAt: at(1999) })],
        nextCursor: null,
      });
    renderSection();

    expect(await screen.findByText('Session 0')).toBeInTheDocument();
    expect(screen.getByText(`Session ${PREVIEW_COUNT - 1}`)).toBeInTheDocument();
    expect(screen.queryByText(`Session ${PREVIEW_COUNT}`)).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('Show more'));
    expect(await screen.findByText(`Session ${PREVIEW_COUNT + 1}`)).toBeInTheDocument();
    expect(acpListSessions).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('Show more'));
    await waitFor(() => expect(acpListSessions).toHaveBeenCalledWith(null, { cwd: '/proj/goose' }));
    expect(await screen.findByText('Paged from the server')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Show more'));
    await waitFor(() =>
      expect(acpListSessions).toHaveBeenCalledWith('cursor-1', { cwd: '/proj/goose' })
    );
    expect(await screen.findByText('Even older')).toBeInTheDocument();
    expect(screen.queryByText('Show more')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('Show less'));
    expect(screen.queryByText('Even older')).not.toBeInTheDocument();
    expect(screen.getByText('Session 0')).toBeInTheDocument();
  });

  it('renders the FAILURE twin when paging cannot load, with a retry', async () => {
    electronMocks();
    navMocks.recentSessions.current = Array.from({ length: PREVIEW_COUNT }, (_, i) =>
      listItem({ id: `s${i}`, name: `Session ${i}`, updatedAt: at(i + 1) })
    );
    vi.mocked(acpListSessions)
      .mockRejectedValueOnce(new Error('agent down'))
      .mockResolvedValueOnce({
        sessions: [listItem({ id: 'old', name: 'Recovered', updatedAt: at(500) })],
        nextCursor: null,
      });
    renderSection();
    fireEvent.click(await screen.findByText('Show more'));

    expect(await screen.findByText("Couldn't load sessions")).toBeInTheDocument();
    fireEvent.click(screen.getByText('Retry'));
    expect(await screen.findByText('Recovered')).toBeInTheDocument();
  });

  it('a registered folder with no sessions is listed, says so, and is the only kind that can be removed', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/empty', addedAt: 1 }]);
    mocks.removeProject.mockResolvedValue([]);
    navMocks.recentSessions.current = [listItem()];
    renderSection();

    const empty = await screen.findByTestId('project-row-/proj/empty');
    expect(within(empty).getByText('No sessions yet')).toBeInTheDocument();

    fireEvent.contextMenu(screen.getByText('goose'));
    expect(await screen.findByTestId('project-context-menu')).toBeInTheDocument();
    expect(screen.queryByText('Remove from projects')).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });

    fireEvent.contextMenu(screen.getByText('empty'));
    fireEvent.click(await screen.findByText('Remove from projects'));
    fireEvent.click(await screen.findByText('Confirm remove (keeps files & sessions)'));
    await waitFor(() => expect(mocks.removeProject).toHaveBeenCalledWith('/proj/empty'));
    const { acpDeleteSession } = await import('../../acp/sessions');
    expect(acpDeleteSession).not.toHaveBeenCalled();
  });

  it('a new session from a folder row inherits that directory via startNewSession', async () => {
    electronMocks();
    navMocks.recentSessions.current = [listItem()];
    renderSection();
    fireEvent.click(await screen.findByLabelText('New session here — goose'));
    await waitFor(() =>
      expect(startNewSession).toHaveBeenCalledWith(undefined, expect.any(Function), '/proj/goose', {
        allExtensions: [],
      })
    );
  });

  it('clicking a session row opens it through the existing open-session handler', async () => {
    electronMocks();
    navMocks.recentSessions.current = [listItem()];
    renderSection();
    fireEvent.click(await screen.findByText('Fix the panel'));
    expect(navMocks.handleSessionClick).toHaveBeenCalledWith('sess-1');
  });

  it('a folder collapses and expands, showing its count while collapsed', async () => {
    electronMocks();
    navMocks.recentSessions.current = [listItem(), listItem({ id: 's2', name: 'Second' })];
    renderSection();
    const row = await screen.findByRole('button', { name: /goose/, expanded: true });
    fireEvent.click(row);
    expect(screen.queryByText('Fix the panel')).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId('project-row-/proj/goose')).getByText('2')
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /goose/, expanded: false }));
    expect(await screen.findByText('Fix the panel')).toBeInTheDocument();
  });

  it('a PROJECTS_CHANGED broadcast from another surface (the home landing) lists the new folder', async () => {
    electronMocks();
    renderSection();
    await screen.findByText(/Your sessions appear here/);
    const entry = { path: '/from/landing', addedAt: 9 };
    window.dispatchEvent(
      new CustomEvent(AppEvents.PROJECTS_CHANGED, { detail: { projects: [entry], added: [entry] } })
    );
    expect(await screen.findByText('landing')).toBeInTheDocument();
  });
});

describe('deriveProjects', () => {
  const s = (id: string, dir: string, minutesAgo: number): SessionListItem =>
    listItem({ id, workingDir: dir, updatedAt: at(minutesAgo) });

  it('the filter field narrows the tree: a session-title hit expands its folder with only the matches; a folder-name hit lists the folder; the header counts what shows', async () => {
    electronMocks();
    navMocks.recentSessions.current = [
      listItem({
        id: 'a',
        name: 'Webhook secret configuration',
        workingDir: '/runs/f114-live',
        updatedAt: at(3),
      }),
      listItem({
        id: 'b',
        name: 'Webhook test setup issue',
        workingDir: '/runs/f114-live',
        updatedAt: at(4),
      }),
      listItem({ id: 'c', name: 'Unrelated', workingDir: '/runs/f114-live', updatedAt: at(5) }),
      listItem({ id: 'd', name: 'Hello', workingDir: '/proj/goose', updatedAt: at(1) }),
      ...Array.from({ length: 6 }, (_, i) =>
        listItem({
          id: `o${i}`,
          name: `Old ${i}`,
          workingDir: `/tmp/folder-${i}`,
          updatedAt: at(100 + i),
        })
      ),
      listItem({
        id: 'w',
        name: 'Old webhook rewrite',
        workingDir: '/tmp/folder-9',
        updatedAt: at(900),
      }),
    ];
    renderSection();
    await screen.findByText('goose');
    const filter = screen.getByRole('textbox', {
      name: 'Filter projects by name and sessions by title',
    });

    fireEvent.change(filter, { target: { value: 'webhook' } });
    expect(screen.getByTestId('lz-section-count').textContent).toBe('2');
    const live = screen.getByTestId('project-row-/runs/f114-live');
    expect(within(live).getByText('Webhook secret configuration')).toBeInTheDocument();
    expect(within(live).getByText('Webhook test setup issue')).toBeInTheDocument();
    expect(within(live).queryByText('Unrelated')).toBeNull();
    // folder-9 is not among the newest three folders — the match opens it anyway
    const old = screen.getByTestId('project-row-/tmp/folder-9');
    expect(within(old).getByRole('button', { expanded: true })).toBeInTheDocument();
    expect(within(old).getByText('Old webhook rewrite')).toBeInTheDocument();
    expect(screen.queryByTestId('project-row-/proj/goose')).toBeNull();

    fireEvent.change(filter, { target: { value: 'GOOSE' } });
    expect(screen.getByTestId('lz-section-count').textContent).toBe('1');
    expect(within(screen.getByTestId('project-row-/proj/goose')).getByText('Hello')).toBeTruthy();

    fireEvent.change(filter, { target: { value: 'zzz' } });
    expect(screen.getByTestId('projects-filter-empty').textContent).toContain('zzz');
    expect(screen.getByTestId('lz-section-count').textContent).toBe('0');

    fireEvent.keyDown(filter, { key: 'Escape' });
    expect((filter as HTMLInputElement).value).toBe('');
    expect(screen.getByTestId('lz-section-count').textContent).toBe('9');
  });

  it('filterProjects searches the sessions "Show more" paged in, not only the recent list', () => {
    const projects = deriveProjects([listItem({ id: 'a', name: 'Recent', workingDir: '/p' })], []);
    const paged = {
      '/p': { sessions: [listItem({ id: 'z', name: 'Paged needle', workingDir: '/p' })] },
    };
    const hit = filterProjects(projects, 'needle', paged);
    expect(hit).toHaveLength(1);
    expect(hit[0].sessions?.map((s) => s.id)).toEqual(['z']);
    expect(filterProjects(projects, '  ', paged)[0].sessions).toBeNull();
  });

  it('one folder per normalized directory, sessions newest first, folders by last activity', () => {
    const projects = deriveProjects(
      [s('a', '/x/goose/', 30), s('b', '/y/forge', 10), s('c', '/x/goose', 1)],
      []
    );
    expect(projects.map((p) => p.path)).toEqual(['/x/goose', '/y/forge']);
    expect(projects[0].sessions.map((x) => x.id)).toEqual(['c', 'a']);
    expect(projects[0].name).toBe('goose');
    expect(projects.every((p) => !p.registered)).toBe(true);
  });

  it('a registered folder joins its sessions, or stands empty at the end', () => {
    const projects = deriveProjects(
      [s('a', '/x/goose', 1)],
      [
        { path: '/x/goose/', addedAt: 1 },
        { path: '/z/empty', addedAt: 2 },
      ]
    );
    expect(projects.map((p) => [p.path, p.registered, p.sessions.length])).toEqual([
      ['/x/goose', true, 1],
      ['/z/empty', true, 0],
    ]);
  });

  it('normalizeDirPath keeps root as root', () => {
    expect(normalizeDirPath('/')).toBe('/');
    expect(normalizeDirPath('///')).toBe('/');
  });
});

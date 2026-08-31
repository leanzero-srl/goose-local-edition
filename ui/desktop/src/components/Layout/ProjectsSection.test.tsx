import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { ProjectsSection, isUnfiledSession, normalizeDirPath } from './ProjectsSection';
import { acpListSessions, type SessionListItem } from '../../acp/sessions';
import { startNewSession } from '../../sessions';
import { AppEvents } from '../../constants/events';

/**
 * The Projects tree: user-curated folders in the sidebar, each expanding to ITS sessions via the
 * server-side cwd filter, with new sessions inheriting the project's directory and removal
 * touching the registry only. Every claim here is one the sidebar makes to the user.
 */

const navMocks = vi.hoisted(() => ({
  recentSessions: { current: [] as unknown[] },
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
    activeSessionId: undefined,
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
    vi.mocked(acpListSessions).mockResolvedValue({ sessions: [], nextCursor: null });
    vi.mocked(startNewSession).mockResolvedValue(undefined as never);
  });

  it('shows the inviting empty state when no projects are registered', async () => {
    electronMocks();
    renderSection();
    expect(
      await screen.findByText('Add a project folder to scope your sessions')
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Add a project folder')).toBeInTheDocument();
  });

  it('expanding a project fetches ITS sessions with the server-side cwd filter', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    vi.mocked(acpListSessions).mockResolvedValue({
      sessions: [listItem(), listItem({ id: 'sess-2', name: 'Ship the tree' })],
      nextCursor: null,
    });

    renderSection();
    fireEvent.click(await screen.findByText('goose'));

    await waitFor(() => expect(acpListSessions).toHaveBeenCalledWith(null, { cwd: '/proj/goose' }));
    expect(await screen.findByText('Fix the panel')).toBeInTheDocument();
    expect(screen.getByText('Ship the tree')).toBeInTheDocument();
  });

  it('an expanded project with no sessions says so honestly', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);

    renderSection();
    fireEvent.click(await screen.findByText('goose'));

    expect(await screen.findByText('No sessions yet')).toBeInTheDocument();
  });

  it('offers a More row while a cursor remains and pages with that cursor', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    vi.mocked(acpListSessions)
      .mockResolvedValueOnce({ sessions: [listItem()], nextCursor: 'cursor-1' })
      .mockResolvedValueOnce({
        sessions: [listItem({ id: 'sess-2', name: 'Older work' })],
        nextCursor: null,
      });

    renderSection();
    fireEvent.click(await screen.findByText('goose'));

    fireEvent.click(await screen.findByText('More sessions…'));

    await waitFor(() =>
      expect(acpListSessions).toHaveBeenCalledWith('cursor-1', { cwd: '/proj/goose' })
    );
    expect(await screen.findByText('Older work')).toBeInTheDocument();
    expect(screen.queryByText('More sessions…')).not.toBeInTheDocument();
  });

  it('renders the FAILURE twin when the session list cannot load, with a retry', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    vi.mocked(acpListSessions)
      .mockRejectedValueOnce(new Error('agent down'))
      .mockResolvedValueOnce({ sessions: [listItem()], nextCursor: null });

    renderSection();
    fireEvent.click(await screen.findByText('goose'));

    expect(await screen.findByText("Couldn't load sessions")).toBeInTheDocument();
    fireEvent.click(screen.getByText('Retry'));
    expect(await screen.findByText('Fix the panel')).toBeInTheDocument();
  });

  it('a new session from the project row inherits the PROJECT path via startNewSession', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);

    renderSection();
    fireEvent.click(await screen.findByLabelText('New session here — goose'));

    await waitFor(() =>
      expect(startNewSession).toHaveBeenCalledWith(undefined, expect.any(Function), '/proj/goose', {
        allExtensions: [],
      })
    );
  });

  it('remove goes through the registry ONLY: removeProject IPC, never session deletion', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([
      { path: '/proj/goose', addedAt: 1 },
      { path: '/proj/other', addedAt: 2 },
    ]);
    mocks.removeProject.mockResolvedValue([{ path: '/proj/other', addedAt: 2 }]);

    renderSection();
    fireEvent.contextMenu(await screen.findByText('goose'));
    fireEvent.click(await screen.findByText('Remove from projects'));
    fireEvent.click(await screen.findByText('Confirm remove (keeps files & sessions)'));

    await waitFor(() => expect(mocks.removeProject).toHaveBeenCalledWith('/proj/goose'));
    await waitFor(() => expect(screen.queryByText('goose')).not.toBeInTheDocument());
    expect(screen.getByText('other')).toBeInTheDocument();

    const { acpDeleteSession } = await import('../../acp/sessions');
    expect(acpDeleteSession).not.toHaveBeenCalled();
  });

  it('Unfiled lists ONLY sessions whose workingDir matches no project (exact, like the server)', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    navMocks.recentSessions.current = [
      listItem({ id: 'filed', name: 'Filed chat', workingDir: '/proj/goose/' }),
      listItem({ id: 'sub', name: 'Subdir chat', workingDir: '/proj/goose/deep' }),
      listItem({ id: 'loose', name: 'Loose chat', workingDir: '/elsewhere' }),
    ];

    renderSection();

    const unfiledHeader = await screen.findByText('Unfiled');
    expect(screen.getByText('2')).toBeInTheDocument();

    fireEvent.click(unfiledHeader);
    expect(await screen.findByText('Subdir chat')).toBeInTheDocument();
    expect(screen.getByText('Loose chat')).toBeInTheDocument();
    expect(screen.queryByText('Filed chat')).not.toBeInTheDocument();
  });

  it('clicking a session row opens it through the existing open-session handler', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    vi.mocked(acpListSessions).mockResolvedValue({ sessions: [listItem()], nextCursor: null });

    renderSection();
    fireEvent.click(await screen.findByText('goose'));
    fireEvent.click(await screen.findByText('Fix the panel'));

    expect(navMocks.handleSessionClick).toHaveBeenCalledWith('sess-1');
  });

  it('adding a project seeds the OS picker, registers the pick, and expands it', async () => {
    const mocks = electronMocks();
    mocks.directoryChooser.mockResolvedValue({ canceled: false, filePaths: ['/picked/app'] });
    mocks.addProject.mockResolvedValue([{ path: '/picked/app', addedAt: 3 }]);

    renderSection();
    fireEvent.click(await screen.findByLabelText('Add a project folder'));

    await waitFor(() => expect(mocks.addProject).toHaveBeenCalledWith('/picked/app'));
    expect(await screen.findByText('app')).toBeInTheDocument();
    await waitFor(() => expect(acpListSessions).toHaveBeenCalledWith(null, { cwd: '/picked/app' }));
  });

  it('a PROJECTS_CHANGED broadcast from another surface (the home landing) registers AND expands', async () => {
    electronMocks();
    renderSection();
    await screen.findByText('Add a project folder to scope your sessions');

    const entry = { path: '/from/landing', addedAt: 9 };
    window.dispatchEvent(
      new CustomEvent(AppEvents.PROJECTS_CHANGED, {
        detail: { projects: [entry], added: [entry] },
      })
    );

    expect(await screen.findByText('landing')).toBeInTheDocument();
    await waitFor(() =>
      expect(acpListSessions).toHaveBeenCalledWith(null, { cwd: '/from/landing' })
    );
  });
});

describe('unfiled membership (mirrors the server exact-match cwd filter)', () => {
  const projects = new Set(['/proj/goose']);

  it('exact match is filed, trailing slashes notwithstanding', () => {
    expect(isUnfiledSession('/proj/goose', projects)).toBe(false);
    expect(isUnfiledSession('/proj/goose/', projects)).toBe(false);
  });

  it('a SUBdirectory is unfiled — the server would never list it under the project', () => {
    expect(isUnfiledSession('/proj/goose/sub', projects)).toBe(true);
  });

  it('unknown and missing dirs are unfiled', () => {
    expect(isUnfiledSession('/elsewhere', projects)).toBe(true);
    expect(isUnfiledSession(undefined, projects)).toBe(true);
  });

  it('normalizeDirPath keeps root as root', () => {
    expect(normalizeDirPath('/')).toBe('/');
    expect(normalizeDirPath('/a/b/')).toBe('/a/b');
  });
});

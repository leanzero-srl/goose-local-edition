import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import ProjectLanding, { LANDING_RECENT_COUNT } from './ProjectLanding';
import { AppEvents } from '../constants/events';
import { TYPE } from './lz';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';
import { acpListRecentSessions, type SessionListItem } from '../acp/sessions';
import { startNewSession } from '../sessions';

/**
 * "/" — a truly empty install (no session anywhere, no folder added) states that sessions start
 * from a project and offers the SAME add-project picker the sidebar "+" uses. Once anything exists
 * (UX audit L1, 2026-09-23: the old landing read the "+" registry alone and told a person with 29
 * session folders to "Add a project") it is the continue page: newest sessions, desks waiting on
 * the person, and a new session in the folder worked in last.
 */

vi.mock('../acp/sessions', () => ({
  acpListRecentSessions: vi.fn(),
}));
vi.mock('../sessions', () => ({
  startNewSession: vi.fn(),
  displaySessionListName: (name: string | null | undefined) =>
    !name || name === 'New Chat' ? 'New Session' : name,
}));
vi.mock('./ConfigContext', () => ({ useConfig: () => ({ extensionsList: [] }) }));

interface ElectronMocks {
  listProjects: ReturnType<typeof vi.fn>;
  addProject: ReturnType<typeof vi.fn>;
  removeProject: ReturnType<typeof vi.fn>;
  directoryChooser: ReturnType<typeof vi.fn>;
  agentWorkList: ReturnType<typeof vi.fn>;
  agentWorkRead: ReturnType<typeof vi.fn>;
}

function electronMocks(): ElectronMocks {
  const mocks: ElectronMocks = {
    listProjects: vi.fn().mockResolvedValue([]),
    addProject: vi.fn().mockResolvedValue([]),
    removeProject: vi.fn().mockResolvedValue([]),
    directoryChooser: vi.fn().mockResolvedValue({ canceled: true, filePaths: [] }),
    agentWorkList: vi.fn().mockResolvedValue([]),
    agentWorkRead: vi.fn(),
  };
  Object.assign(window.electron, mocks);
  return mocks;
}

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
const session = (id: string, name: string, dir: string, m: number): SessionListItem => ({
  id,
  name,
  workingDir: dir,
  updatedAt: minutesAgo(m),
  messageCount: 3,
  createdAt: minutesAgo(m),
});

function Where() {
  const loc = useLocation();
  return <div data-testid="where">{loc.pathname + loc.search}</div>;
}

const renderLanding = () =>
  render(
    <MemoryRouter initialEntries={['/']}>
      <IntlProvider locale="en" messages={{}}>
        <Routes>
          <Route path="/" element={<ProjectLanding />} />
          <Route path="*" element={<Where />} />
        </Routes>
      </IntlProvider>
    </MemoryRouter>
  );

describe('ProjectLanding — a truly empty install', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(acpListRecentSessions).mockResolvedValue([]);
  });

  it('states the rule and carries NO chat input', async () => {
    electronMocks();
    renderLanding();
    expect(await screen.findByText('Start from a project')).toBeInTheDocument();
    expect(document.querySelector('textarea')).toBeNull();
    expect(document.querySelector('input')).toBeNull();
  });

  it('the Add-project button drives the same picker flow, broadcasts the change, and the landing becomes the continue page', async () => {
    const mocks = electronMocks();
    mocks.directoryChooser.mockResolvedValue({ canceled: false, filePaths: ['/picked/app'] });
    mocks.addProject.mockResolvedValue([{ path: '/picked/app', addedAt: 3 }]);

    const changed = vi.fn();
    window.addEventListener(AppEvents.PROJECTS_CHANGED, changed);
    try {
      renderLanding();
      fireEvent.click(await screen.findByText('Add a project'));
      await waitFor(() => expect(mocks.addProject).toHaveBeenCalledWith('/picked/app'));
      expect(mocks.directoryChooser).toHaveBeenCalled();
      await waitFor(() => expect(changed).toHaveBeenCalledTimes(1));
      expect(await screen.findByText('New session in app')).toBeInTheDocument();
      expect(screen.getByText('No sessions yet. Start the first one in app.')).toBeInTheDocument();
      expect(screen.queryByText('Add a project')).not.toBeInTheDocument();
    } finally {
      window.removeEventListener(AppEvents.PROJECTS_CHANGED, changed);
    }
  });

  it('is a composed EmptyState — the LeanZero mark, a display title, ONE body line — over a 3-row KeyValue, with the add action the ONE primary', async () => {
    electronMocks();
    const { container } = renderLanding();
    await screen.findByText('Start from a project');
    expect(screen.getByTestId('project-landing').className).toContain('max-w-[560px]');
    const empty = screen.getByTestId('lz-empty-state');
    expect(within(empty).getByTestId('leanzero-glyph')).toBeInTheDocument();
    expect(screen.getByTestId('lz-empty-state-icon').className).toContain('bg-lz-accent');
    const title = screen.getByRole('heading', { name: 'Start from a project' });
    for (const c of TYPE.display.split(' ')) expect(title.className).toContain(c);
    expect(empty.querySelectorAll('p')).toHaveLength(1);
    expect(screen.getAllByTestId('lz-key-value-row')).toHaveLength(3);
    expect(container.querySelectorAll('[data-variant="primary"]')).toHaveLength(1);
    expect(container.querySelector('[style]')).toBeNull();
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('a cancelled picker changes nothing', async () => {
    const mocks = electronMocks();
    renderLanding();
    fireEvent.click(await screen.findByText('Add a project'));
    await waitFor(() => expect(mocks.directoryChooser).toHaveBeenCalled());
    expect(mocks.addProject).not.toHaveBeenCalled();
    expect(await screen.findByText('Add a project')).toBeInTheDocument();
  });
});

describe('ProjectLanding — the continue page once anything exists', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('sessions but an EMPTY "+" registry is not an empty install: the newest sessions, newest first, with project and time; click resumes', async () => {
    electronMocks();
    const many = Array.from({ length: 12 }, (_, i) =>
      session(`s${i}`, `Session ${i}`, i % 2 ? '/runs/f114-live' : '/proj/goose', 10 + i * 10)
    );
    vi.mocked(acpListRecentSessions).mockResolvedValue([...many].reverse());
    const { container } = renderLanding();

    expect(await screen.findByText('Pick up where you left off')).toBeInTheDocument();
    expect(screen.queryByText('Start from a project')).toBeNull();
    expect(screen.queryByText('Add a project')).toBeNull();
    const list = screen.getByTestId('landing-recent-sessions');
    const rows = within(list).getAllByRole('button');
    expect(rows).toHaveLength(LANDING_RECENT_COUNT);
    expect(rows[0].textContent).toContain('Session 0');
    expect(rows[0].textContent).toContain('goose');
    expect(rows[0].textContent).toContain('10m ago');
    expect(rows[1].textContent).toContain('f114-live');
    // the header counts what the body shows
    const panel = list.closest('[data-testid="lz-panel"]') as HTMLElement;
    expect(within(panel).getByTestId('lz-section-count').textContent).toBe(
      String(LANDING_RECENT_COUNT)
    );
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);

    fireEvent.click(rows[1]);
    expect(screen.getByTestId('where').textContent).toBe('/pair?resumeSessionId=s1');
  }, 30_000);

  it('the ONE primary is a new session in the folder worked in last', async () => {
    electronMocks();
    vi.mocked(acpListRecentSessions).mockResolvedValue([
      session('a', 'Older', '/proj/goose', 90),
      session('b', 'Newest', '/runs/f114-live', 5),
    ]);
    renderLanding();
    const primary = (await screen.findByText('New session in f114-live')).closest(
      'button'
    ) as HTMLElement;
    expect(primary.dataset.variant).toBe('primary');
    expect(document.querySelectorAll('[data-variant="primary"]')).toHaveLength(1);
    fireEvent.click(primary);
    await waitFor(() =>
      expect(startNewSession).toHaveBeenCalledWith(
        undefined,
        expect.any(Function),
        '/runs/f114-live',
        {
          allExtensions: [],
        }
      )
    );
  });

  it('a failed session read shows the failure with Retry — never the empty-install page', async () => {
    electronMocks();
    vi.mocked(acpListRecentSessions)
      .mockRejectedValueOnce(new Error('acp down'))
      .mockResolvedValueOnce([session('a', 'Back again', '/proj/goose', 5)]);
    renderLanding();
    expect(await screen.findByText('Could not load your sessions: acp down')).toBeInTheDocument();
    expect(screen.queryByText('Start from a project')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('Back again')).toBeInTheDocument();
  });

  it('desks with open questions or drafts to approve are listed (read through foldDesk); a quiet desk is not; an unreadable desk says so', async () => {
    const mocks = electronMocks();
    vi.mocked(acpListRecentSessions).mockResolvedValue([session('a', 'Work', '/proj/goose', 5)]);
    const base = {
      manifest: null,
      state: null,
      pid: null,
      heartbeatMs: null,
      events: [],
      ticks: [],
      lanes: {},
      laneMtimes: {},
      ledger: null,
      scratchpad: '',
      pending: '',
      dailyLog: '',
      engineLog: '',
      now: Date.now(),
    };
    mocks.agentWorkList.mockResolvedValue([
      {
        dir: '/desks/needy',
        addedAt: '',
        manifest: { title: 'Access requests' },
        state: null,
        pid: null,
        heartbeatMs: null,
        exists: true,
      },
      {
        dir: '/desks/quiet',
        addedAt: '',
        manifest: { title: 'Quiet desk' },
        state: null,
        pid: null,
        heartbeatMs: null,
        exists: true,
      },
      {
        dir: '/desks/broken',
        addedAt: '',
        manifest: { title: 'Broken desk' },
        state: null,
        pid: null,
        heartbeatMs: null,
        exists: true,
      },
      {
        dir: '/desks/gone',
        addedAt: '',
        manifest: { title: 'Gone desk' },
        state: null,
        pid: null,
        heartbeatMs: null,
        exists: false,
      },
    ]);
    mocks.agentWorkRead.mockImplementation(async (dir: string) => {
      if (dir === '/desks/broken') throw new Error('state.json unreadable');
      if (dir === '/desks/needy')
        return {
          ...base,
          dir,
          asks: [
            { id: 'q1', status: 'open', question: 'Which group?' },
            { id: 'q2', status: 'answered', question: 'Old' },
          ],
          prepared: [{ id: 'd1', status: 'staged', staged_at: '' }],
        };
      return { ...base, dir, asks: [], prepared: [] };
    });
    renderLanding();
    const desks = await screen.findByTestId('landing-desks');
    const rows = within(desks).getAllByRole('button');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Access requests');
    expect(rows[0].textContent).toContain('Open questions: 1');
    expect(rows[0].textContent).toContain('Drafts to approve: 1');
    expect(rows[1].textContent).toContain('Could not read this desk: state.json unreadable');
    expect(within(desks).queryByText('Quiet desk')).toBeNull();
    expect(mocks.agentWorkRead).not.toHaveBeenCalledWith('/desks/gone');
    fireEvent.click(rows[0]);
    expect(screen.getByTestId('where').textContent).toBe('/agent-work?desk=%2Fdesks%2Fneedy');
  });
});

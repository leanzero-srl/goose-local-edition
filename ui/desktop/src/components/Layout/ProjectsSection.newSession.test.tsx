import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { ProjectsSection } from './ProjectsSection';
import { acpListSessions, type SessionListItem } from '../../acp/sessions';
import type { Session } from '../../types/session';

/**
 * Q-440 (owner demo, 3.0.73): "New session here" from the Nodes page. goosed creates the session
 * row at once and then loads its extensions (5 s measured: 20260928_38 "Session loaded" 19:26:56.4,
 * its extensions done ~19:27:01.6) before session/new answers — the sidebar listed the new
 * session while the page stayed on Nodes, the second use made a second session, and the first was
 * left an empty "New Session". This drives the real startNewSession with the engine's answer held.
 */

const engine = vi.hoisted(() => ({
  createSession: vi.fn(),
}));

vi.mock('../../acp/chatSessionController', () => ({
  acpChatSessionController: { createSession: engine.createSession },
}));

vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ extensionsList: [] }),
}));

vi.mock('../../acp/sessions', () => ({
  acpListSessions: vi.fn(),
  acpDeleteSession: vi.fn(),
  acpRenameSession: vi.fn(),
}));

const recent: SessionListItem[] = [
  {
    id: 'sess-1',
    name: 'Portugal capital question',
    workingDir: '/proj/demo',
    updatedAt: new Date().toISOString(),
    messageCount: 4,
    createdAt: new Date().toISOString(),
  },
];

vi.mock('../../hooks/useNavigationSessions', () => ({
  useNavigationSessions: () => ({
    recentSessions: recent,
    activeSessionId: undefined,
    fetchSessions: vi.fn(),
    handleNavClick: vi.fn(),
    handleSessionClick: vi.fn(),
  }),
}));

function Where() {
  const location = useLocation();
  return <p data-testid="where">{`${location.pathname}${location.search}`}</p>;
}

function renderOnNodesPage() {
  Object.assign(window.electron, {
    listProjects: vi.fn().mockResolvedValue([]),
    addProject: vi.fn().mockResolvedValue([]),
    removeProject: vi.fn().mockResolvedValue([]),
    directoryChooser: vi.fn().mockResolvedValue({ canceled: true, filePaths: [] }),
    revealInFinder: vi.fn().mockResolvedValue(true),
  });
  render(
    <MemoryRouter initialEntries={['/nodes?tab=nodes']}>
      <IntlProvider locale="en" messages={{}}>
        <ProjectsSection />
        <Routes>
          <Route path="*" element={<Where />} />
        </Routes>
      </IntlProvider>
    </MemoryRouter>
  );
}

function held() {
  let answer!: (session: Session) => void;
  const promise = new Promise<Session>((resolve) => {
    answer = resolve;
  });
  return { promise, answer };
}

const created = (id: string) =>
  ({ id, name: 'New Chat', working_dir: '/proj/demo', message_count: 0 }) as unknown as Session;

describe('Q-440: New session here from the Nodes page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(acpListSessions).mockResolvedValue({ sessions: [], nextCursor: null });
  });

  it('opens the session it creates, and a second use while it is created makes no second one', async () => {
    const first = held();
    engine.createSession.mockReturnValueOnce(first.promise);
    renderOnNodesPage();
    const plus = await screen.findByLabelText('New session here — demo');
    fireEvent.click(plus);
    expect(engine.createSession).toHaveBeenCalledTimes(1);
    // While goosed loads the session's extensions the "+" says it is working, and refuses a
    // second session.
    await waitFor(() => expect(plus).toBeDisabled());
    expect(plus).toHaveAttribute(
      'title',
      'Starting a session here — it opens when its extensions are loaded'
    );
    fireEvent.click(plus);
    fireEvent.click(plus);
    expect(engine.createSession).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('where')).toHaveTextContent('/nodes?tab=nodes');
    await act(async () => first.answer(created('20260928_38')));
    await waitFor(() =>
      expect(screen.getByTestId('where')).toHaveTextContent('/pair?resumeSessionId=20260928_38')
    );
    expect(plus).toBeEnabled();
    expect(plus).toHaveAttribute('title', 'New session here');

    // Once it opened, the folder's "+" makes the next session as asked.
    engine.createSession.mockResolvedValueOnce(created('20260928_39'));
    fireEvent.click(plus);
    await waitFor(() =>
      expect(screen.getByTestId('where')).toHaveTextContent('/pair?resumeSessionId=20260928_39')
    );
    expect(engine.createSession).toHaveBeenCalledTimes(2);
  });
});

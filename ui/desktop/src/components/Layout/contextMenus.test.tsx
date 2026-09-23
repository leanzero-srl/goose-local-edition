import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { ProjectsSection, askAboutSessionPrompt } from './ProjectsSection';
import { AppEvents } from '../../constants/events';
import type { SessionListItem } from '../../acp/sessions';

/** The session row's own right-click menu: open, rename (our dialog, never window.prompt), fork,
 *  "start an AI session about this session", delete with an in-menu confirm — every action through
 *  the existing ACP calls and the existing app events. */
const navMocks = vi.hoisted(() => ({
  recentSessions: { current: [] as unknown[] },
  handleSessionClick: vi.fn(),
  startChat: vi.fn(),
}));
vi.mock('../ConfigContext', () => ({ useConfig: () => ({ extensionsList: [] }) }));
vi.mock('./useStartChatAbout', () => ({ useStartChatAbout: () => navMocks.startChat }));
vi.mock('../../sessions', () => ({
  startNewSession: vi.fn(),
  displaySessionListName: (name: string | null | undefined) => name || 'New Session',
}));
const acp = vi.hoisted(() => ({
  acpListSessions: vi.fn(),
  acpDeleteSession: vi.fn(),
  acpRenameSession: vi.fn(),
  acpForkSession: vi.fn(),
}));
vi.mock('../../acp/sessions', () => acp);
vi.mock('../../hooks/useNavigationSessions', () => ({
  useNavigationSessions: () => ({
    recentSessions: navMocks.recentSessions.current,
    activeSessionId: undefined,
    fetchSessions: vi.fn(),
    handleNavClick: vi.fn(),
    handleSessionClick: navMocks.handleSessionClick,
  }),
}));

const session: SessionListItem = {
  id: 'sess-1',
  name: 'Fix the panel',
  workingDir: '/proj/goose',
  updatedAt: new Date().toISOString(),
  messageCount: 4,
  createdAt: new Date().toISOString(),
};

const renderSection = () =>
  render(
    <MemoryRouter>
      <IntlProvider locale="en" messages={{}}>
        <ProjectsSection />
      </IntlProvider>
    </MemoryRouter>
  );

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(window.electron, { listProjects: vi.fn().mockResolvedValue([]) });
  navMocks.recentSessions.current = [session];
  acp.acpListSessions.mockResolvedValue({ sessions: [], nextCursor: null });
});

describe('the session context menu', () => {
  it('renames through our dialog and broadcasts SESSION_RENAMED', async () => {
    acp.acpRenameSession.mockResolvedValue(undefined);
    const renamed = vi.fn();
    window.addEventListener(AppEvents.SESSION_RENAMED, renamed);
    renderSection();
    fireEvent.contextMenu(await screen.findByText('Fix the panel'));
    fireEvent.click(await screen.findByText('Rename'));
    const input = await screen.findByLabelText('Name');
    fireEvent.change(input, { target: { value: 'Panel fix, round two' } });
    fireEvent.click(screen.getByTestId('rename-save'));
    await waitFor(() =>
      expect(acp.acpRenameSession).toHaveBeenCalledWith('sess-1', 'Panel fix, round two')
    );
    expect(renamed).toHaveBeenCalled();
    expect((renamed.mock.calls[0][0] as CustomEvent).detail).toMatchObject({
      sessionId: 'sess-1',
      newName: 'Panel fix, round two',
      userInitiated: true,
    });
  });

  it('forks and opens the fork; asks an AI about the session with its id and directory', async () => {
    acp.acpForkSession.mockResolvedValue('sess-fork');
    renderSection();
    fireEvent.contextMenu(await screen.findByText('Fix the panel'));
    fireEvent.click(await screen.findByText('Fork session'));
    await waitFor(() => expect(navMocks.handleSessionClick).toHaveBeenCalledWith('sess-fork'));

    fireEvent.contextMenu(screen.getByText('Fix the panel'));
    fireEvent.click(await screen.findByText('Start an AI session about this session'));
    // The mocked profile carries no chatrecall, so the prompt says nothing of the session is attached.
    const prompt = askAboutSessionPrompt(session, { chatRecall: false });
    expect(navMocks.startChat).toHaveBeenCalledWith(prompt, { alsoEnable: ['chatrecall'] });
    expect(prompt).toContain('sess-1');
    expect(prompt).toContain('/proj/goose');
  });

  it('deletes only after the in-menu confirm and broadcasts SESSION_DELETED', async () => {
    acp.acpDeleteSession.mockResolvedValue(undefined);
    const deleted = vi.fn();
    window.addEventListener(AppEvents.SESSION_DELETED, deleted);
    renderSection();
    fireEvent.contextMenu(await screen.findByText('Fix the panel'));
    const menu = await screen.findByTestId('session-context-menu');
    fireEvent.click(within(menu).getByText('Delete session'));
    expect(acp.acpDeleteSession).not.toHaveBeenCalled();
    fireEvent.click(within(menu).getByText('Confirm delete (cannot be undone)'));
    await waitFor(() => expect(acp.acpDeleteSession).toHaveBeenCalledWith('sess-1'));
    expect((deleted.mock.calls[0][0] as CustomEvent).detail).toEqual({ sessionId: 'sess-1' });
  });
});

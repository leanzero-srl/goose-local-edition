import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { AgentWorkSection, askAboutAgentPrompt, deskHref } from './AgentWorkSection';

const nav = vi.hoisted(() => ({ navigate: vi.fn(), startChat: vi.fn() }));
vi.mock('react-router-dom', async (orig) => ({
  ...(await orig<typeof import('react-router-dom')>()),
  useNavigate: () => nav.navigate,
}));
vi.mock('./useStartChatAbout', () => ({ useStartChatAbout: () => nav.startChat }));

const DESK = {
  dir: '/agents/public-web',
  addedAt: '2026-09-01T00:00:00Z',
  manifest: { name: 'public-web', title: 'Public web research' },
  state: { status: 'idle', tick: 3, phase: 'idle', next_tick_at: null },
  pid: null,
  heartbeatMs: null,
  exists: true,
};
const TICKS = [
  {
    tick: 1,
    started_at: '2026-09-20T10:00:00Z',
    ended_at: '2026-09-20T10:05:00Z',
    outcome: 'ok',
    summary: 'first',
  },
  {
    tick: 2,
    started_at: '2026-09-20T11:00:00Z',
    ended_at: '2026-09-20T11:05:00Z',
    outcome: 'ok',
    summary: 'second',
  },
  {
    tick: 3,
    started_at: '2026-09-20T12:00:00Z',
    ended_at: '2026-09-20T12:05:00Z',
    outcome: 'ask',
    summary: 'third',
  },
];

function mocks() {
  const m = {
    agentWorkList: vi.fn().mockResolvedValue([DESK]),
    agentWorkRead: vi.fn().mockResolvedValue({ ...DESK, ticks: TICKS }),
    agentWorkRemove: vi.fn().mockResolvedValue(true),
    revealInFinder: vi.fn().mockResolvedValue(true),
  };
  Object.assign(window.electron, m);
  return m;
}

const renderSection = () =>
  render(
    <MemoryRouter>
      <IntlProvider locale="en" messages={{}}>
        <AgentWorkSection />
      </IntlProvider>
    </MemoryRouter>
  );

beforeEach(() => vi.clearAllMocks());

describe('AgentWorkSection', () => {
  it('lists every desk as a row; expanding reads the desk and nests its ticks newest first', async () => {
    const m = mocks();
    renderSection();
    const row = await screen.findByRole('button', { name: /Public web research/ });
    expect(row.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(row);
    await waitFor(() => expect(m.agentWorkRead).toHaveBeenCalledWith('/agents/public-web'));
    const ticks = await screen.findAllByTestId(/^tick-row-/);
    expect(ticks.map((t) => t.getAttribute('data-testid'))).toEqual([
      'tick-row-3',
      'tick-row-2',
      'tick-row-1',
    ]);
    expect(within(ticks[0]).getByText(/ask · third/)).toBeInTheDocument();
  });

  it('opening the desk or a tick navigates the Agent Work view by URL', async () => {
    mocks();
    renderSection();
    fireEvent.click(await screen.findByRole('button', { name: /Public web research/ }));
    fireEvent.click(await screen.findByTestId('tick-row-2'));
    expect(nav.navigate).toHaveBeenCalledWith('/agent-work?desk=%2Fagents%2Fpublic-web&tick=2');
    fireEvent.click(screen.getByTestId('desk-open-/agents/public-web'));
    expect(nav.navigate).toHaveBeenCalledWith(deskHref('/agents/public-web'));
    fireEvent.click(screen.getByLabelText('New agent'));
    expect(nav.navigate).toHaveBeenCalledWith('/agent-work?new=1');
  });

  it('the row’s context menu: ask an AI about the agent, and remove it from the roster after a confirm', async () => {
    const m = mocks();
    renderSection();
    fireEvent.contextMenu(await screen.findByText('Public web research'));
    const menu = await screen.findByTestId('desk-context-menu');
    fireEvent.click(within(menu).getByText('Start an AI session about this agent'));
    expect(nav.startChat).toHaveBeenCalledWith(askAboutAgentPrompt(DESK as never));
    expect(askAboutAgentPrompt(DESK as never)).toContain('/agents/public-web');

    fireEvent.contextMenu(screen.getByText('Public web research'));
    fireEvent.click(await screen.findByText('Remove from roster'));
    expect(m.agentWorkRemove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('Confirm remove (keeps the folder)'));
    await waitFor(() => expect(m.agentWorkRemove).toHaveBeenCalledWith('/agents/public-web'));
  });

  it('says so when there are no agents', async () => {
    const m = mocks();
    m.agentWorkList.mockResolvedValue([]);
    renderSection();
    expect(await screen.findByText(/No agents yet/)).toBeInTheDocument();
  });
});

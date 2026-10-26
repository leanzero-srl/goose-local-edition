import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import * as api from '../../acp/controller';
import { leanzeroLinkStatus } from '../../acp/leanzero-link';
import { fixtureEntry, fixtureHost } from './controllerFixtures';
import LinkedSessionsView from './LinkedSessionsView';

const fixtureDevice: NonNullable<api.ControllerStatus['device']> = {
  device_id: 'device-a',
  name: 'Controller',
  platform: 'desktop',
  role: 'controller',
  revision: 1,
  scopes: [],
  created_at: '',
  revoked_at: null,
};

vi.mock('../../acp/controller', async (original) => ({
  ...(await original<typeof import('../../acp/controller')>()),
  controllerStatus: vi.fn(),
  controllerCatalog: vi.fn(),
  controllerSync: vi.fn(),
}));
vi.mock('../../acp/leanzero-link', () => ({ leanzeroLinkStatus: vi.fn() }));
vi.mock('./ControllerWorkView', () => ({
  default: ({ entry, host }: { entry: typeof fixtureEntry; host: typeof fixtureHost }) => (
    <p>
      {entry.work.owner_node_id}/{entry.work.work_id} on {host.name}
    </p>
  ),
}));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(leanzeroLinkStatus).mockResolvedValue({ auth: { state: 'loggedOut' }, nodeCount: 0 });
  vi.mocked(api.controllerStatus).mockResolvedValue({
    state: 'ready',
    device: fixtureDevice,
    error: null,
  });
  vi.mocked(api.controllerCatalog).mockResolvedValue({
    updated_at: '2026-10-26T10:00:00Z',
    hosts: [
      { host: fixtureHost, entries: [fixtureEntry], stale: false, error: null },
      {
        host: { ...fixtureHost, owner_node_id: 'owner-b', name: 'Laptop' },
        entries: [
          {
            ...fixtureEntry,
            work: { ...fixtureEntry.work, owner_node_id: 'owner-b' },
            title: 'Second session',
          },
        ],
        stale: false,
        error: null,
      },
    ],
  });
});
describe('linked catalog selection', () => {
  it('keeps loading after enrollment stores the new device until its first verification finishes', async () => {
    vi.mocked(api.controllerStatus)
      .mockResolvedValueOnce({ state: 'signed_out', device: null, error: null })
      .mockResolvedValueOnce({ state: 'unavailable', device: fixtureDevice, error: null });
    vi.mocked(leanzeroLinkStatus).mockResolvedValue({
      auth: { state: 'connected', email: 'fixture@example.com', meshIp: '100.64.0.1' },
      nodeCount: 1,
    });
    vi.useFakeTimers();
    const view = render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    try {
      await act(async () => {});
      expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(api.controllerStatus).toHaveBeenCalledTimes(2);
      expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
      expect(api.controllerCatalog).not.toHaveBeenCalled();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(screen.getByText('Original session')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });
  it.each([
    {
      label: 'explicit denial',
      device: fixtureDevice,
      error: 'Controller enrollment was denied',
      connected: true,
    },
    { label: 'disconnected Link', device: fixtureDevice, error: null, connected: false },
    { label: 'missing device', device: null, error: null, connected: true },
    {
      label: 'revoked device',
      device: { ...fixtureDevice, revoked_at: '2026-10-26T10:00:00Z' },
      error: null,
      connected: true,
    },
  ])('preserves unavailable errors for $label', async ({ device, error, connected }) => {
    vi.mocked(api.controllerStatus).mockResolvedValue({ state: 'unavailable', device, error });
    vi.mocked(leanzeroLinkStatus).mockResolvedValue({
      auth: connected
        ? { state: 'connected', email: 'fixture@example.com', meshIp: '100.64.0.1' }
        : { state: 'loggedIn', email: 'fixture@example.com' },
      nodeCount: connected ? 1 : 0,
    });
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      error ?? 'The desktop controller is unavailable.'
    );
    expect(screen.queryByText('Connecting to your linked Macs…')).not.toBeInTheDocument();
  });
  it('does not treat a previously verified device becoming unavailable as initial loading', async () => {
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    await screen.findByText('Original session');
    vi.mocked(api.controllerStatus).mockResolvedValue({
      state: 'unavailable',
      device: fixtureDevice,
      error: null,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sync sessions' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The desktop controller is unavailable.'
    );
    expect(leanzeroLinkStatus).not.toHaveBeenCalled();
    expect(screen.getByText('Original session')).toBeInTheDocument();
  });
  it('keeps loading through pending status and the first catalog request', async () => {
    let resolveStatus!: (status: api.ControllerStatus) => void;
    let resolveCatalog!: (catalog: api.ControllerCatalog) => void;
    vi.mocked(api.controllerStatus).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveStatus = resolve;
      })
    );
    vi.mocked(api.controllerCatalog).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCatalog = resolve;
      })
    );
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
    expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
    await act(async () => resolveStatus({ state: 'ready', device: null, error: null }));
    expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
    expect(screen.queryByText('No compatible linked Macs are available.')).not.toBeInTheDocument();
    await act(async () => resolveCatalog({ hosts: [], updated_at: '2026-10-26T10:00:00Z' }));
    expect(screen.getByText('No compatible linked Macs are available.')).toBeInTheDocument();
    expect(screen.queryByText('Connecting to your linked Macs…')).not.toBeInTheDocument();
  });
  it.each(['loggedIn', 'connecting', 'connected'] as const)(
    'does not diagnose a %s Link account as signed out while controller enrollment catches up',
    async (state) => {
      vi.mocked(api.controllerStatus).mockResolvedValueOnce({
        state: 'signed_out',
        device: null,
        error: null,
      });
      vi.mocked(leanzeroLinkStatus).mockResolvedValue({
        auth:
          state === 'connected'
            ? { state, email: 'fixture@example.com', meshIp: '100.64.0.1' }
            : { state, email: 'fixture@example.com' },
        nodeCount: 1,
      });
      render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
      await waitFor(() => expect(leanzeroLinkStatus).toHaveBeenCalledOnce());
      expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
      expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
      expect(api.controllerCatalog).not.toHaveBeenCalled();
      expect(
        await screen.findByText('Original session', {}, { timeout: 3000 })
      ).toBeInTheDocument();
      expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
    }
  );
  it('treats a catalog without its first sync timestamp as pending', async () => {
    vi.mocked(api.controllerCatalog).mockResolvedValue({ hosts: [], updated_at: null });
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    await waitFor(() => expect(api.controllerCatalog).toHaveBeenCalledOnce());
    expect(screen.getByText('Connecting to your linked Macs…')).toBeInTheDocument();
    expect(screen.queryByText('No compatible linked Macs are available.')).not.toBeInTheDocument();
  });
  it('surfaces account lookup failure without claiming the account is signed out', async () => {
    vi.mocked(api.controllerStatus).mockResolvedValue({
      state: 'signed_out',
      device: null,
      error: null,
    });
    vi.mocked(leanzeroLinkStatus).mockRejectedValue(new Error('Link identity is unavailable'));
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    expect(await screen.findByRole('alert')).toHaveTextContent('Link identity is unavailable');
    expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
  });
  it('shows enrollment denial for a connected account instead of waiting indefinitely', async () => {
    vi.mocked(api.controllerStatus).mockResolvedValue({
      state: 'signed_out',
      device: null,
      error: 'Controller enrollment was denied',
    });
    vi.mocked(leanzeroLinkStatus).mockResolvedValue({
      auth: { state: 'connected', email: 'fixture@example.com', meshIp: '100.64.0.1' },
      nodeCount: 1,
    });
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    expect(await screen.findByRole('alert')).toHaveTextContent('Controller enrollment was denied');
    expect(screen.queryByText('Connecting to your linked Macs…')).not.toBeInTheDocument();
    expect(screen.queryByText(/Sign in to LeanZero Link/)).not.toBeInTheDocument();
  });
  it('keeps colliding work IDs separate by immutable owner and shows the owner on each card', async () => {
    render(<LinkedSessionsView navigation={<span>Session sources</span>} />, {
      wrapper: IntlTestWrapper,
    });
    await screen.findByText('Original session');
    expect(screen.getByText('Owner: Studio')).toBeInTheDocument();
    expect(screen.getByText('Owner: Laptop')).toBeInTheDocument();
    expect(screen.getAllByText('1 session')).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: /Second session/ }));
    expect(await screen.findByText('owner-b/work-a on Laptop')).toBeInTheDocument();
  });
  it('filtered counts match cards, and sync failure remains visible', async () => {
    vi.mocked(api.controllerSync).mockRejectedValue(new Error('Owner authority is unavailable'));
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    await screen.findByText('Original session');
    fireEvent.change(screen.getByRole('textbox', { name: 'Search linked sessions' }), {
      target: { value: 'Second' },
    });
    expect(screen.queryByText('Original session')).not.toBeInTheDocument();
    expect(screen.getByText('0 sessions')).toBeInTheDocument();
    expect(screen.getByText('1 session')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Sync sessions' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Owner authority is unavailable');
    await waitFor(() => expect(screen.getAllByText('Saved state — reconnecting')).toHaveLength(2));
  });
  it('does not query private history or fabricate an empty catalog when signed out', async () => {
    vi.mocked(api.controllerStatus).mockResolvedValue({
      state: 'signed_out',
      device: null,
      error: null,
    });
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    expect(await screen.findByText(/Sign in to LeanZero Link/)).toBeInTheDocument();
    expect(api.controllerCatalog).not.toHaveBeenCalled();
  });
  it('includes archived work only when the shared archive control is checked', async () => {
    vi.mocked(api.controllerCatalog).mockResolvedValue({
      updated_at: '2026-10-26T10:00:00Z',
      hosts: [
        {
          host: fixtureHost,
          entries: [{ ...fixtureEntry, title: 'Archived session', archived: true }],
          stale: false,
          error: null,
        },
      ],
    });
    render(<LinkedSessionsView navigation={null} />, { wrapper: IntlTestWrapper });
    await screen.findByText('0 sessions');
    expect(screen.queryByText('Archived session')).not.toBeInTheDocument();
    const control = screen.getByRole('checkbox', { name: 'Include archived sessions' });
    expect(control).not.toBeChecked();
    fireEvent.click(control);
    expect(control).toBeChecked();
    expect(screen.getByText('Archived session')).toBeInTheDocument();
    expect(screen.getByText('1 session')).toBeInTheDocument();
  });
});

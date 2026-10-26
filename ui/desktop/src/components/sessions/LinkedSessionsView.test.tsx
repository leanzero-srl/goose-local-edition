import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import * as api from '../../acp/controller';
import { fixtureEntry, fixtureHost } from './controllerFixtures';
import LinkedSessionsView from './LinkedSessionsView';

vi.mock('../../acp/controller', async (original) => ({
  ...(await original<typeof import('../../acp/controller')>()),
  controllerStatus: vi.fn(),
  controllerCatalog: vi.fn(),
  controllerSync: vi.fn(),
}));
vi.mock('./ControllerWorkView', () => ({
  default: ({ entry, host }: { entry: typeof fixtureEntry; host: typeof fixtureHost }) => (
    <p>
      {entry.work.owner_node_id}/{entry.work.work_id} on {host.name}
    </p>
  ),
}));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.controllerStatus).mockResolvedValue({
    state: 'ready',
    device: {
      device_id: 'device-a',
      name: 'Controller',
      platform: 'desktop',
      role: 'controller',
      revision: 1,
      scopes: [],
      created_at: '',
      revoked_at: null,
    },
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

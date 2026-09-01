import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import SwarmSettingsSection from './SwarmSettingsSection';
import type { SwarmConfig } from './golden';
import { allClasses, assertStudioClean } from '../../lz/assertStudioClean';
import { missingUtilities } from '../../lz/compileStudioCss';

// ---------------------------------------------------------------------------
// The legacy lever panel (unrouted since the owner removed the Settings > Swarm tab — see
// SettingsView.tsx). These pin its LeanZero Studio register: the no-fleet EmptyState, the
// node-weights DataTable, custom controls only, and that every emitted class compiles.
// ---------------------------------------------------------------------------

const mockRead = vi.fn();
const mockUpsert = vi.fn();
vi.mock('../../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead, upsert: mockUpsert }),
}));

const fleetState = {
  lanes: [] as unknown[],
  models: [] as string[],
  online: false,
  loading: false,
  endpoint: 'http://localhost:1234',
};
vi.mock('../../swarm/useFleet', () => ({
  useFleet: () => fleetState,
  deviceFromModelId: (id: string) => {
    const bare = id.split('/').pop() || id;
    const dash = bare.indexOf('-');
    return dash > 0 ? bare.slice(0, dash) : bare;
  },
}));

vi.mock('../../swarm/FanInCard', () => ({
  default: () => <div data-testid="fan-in-card" />,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const CFG: SwarmConfig = {
  endpoint: 'http://192.168.8.220:1234',
  devices: [
    { id: 'workhorse-mlx', model_id: 'workhorse-qwen3.5-9b-4bit-mlx', weight: 2, enabled: true, engine: 'mlx-sidecar' },
    { id: 'zai-glm', model_id: 'glm-5.3-flash', weight: 2, enabled: true, provider: 'zai', host: 'zai' },
  ],
  speed_weights: { zai: 3 },
};

const render = () => rtlRender(<SwarmSettingsSection />);

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  fleetState.lanes = [];
  fleetState.models = [];
  fleetState.online = false;
  mockRead.mockResolvedValue(CFG);
  mockUpsert.mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe('SwarmSettingsSection — LeanZero Studio register', () => {
  it('an offline fleet renders the No-fleet EmptyState naming the configured endpoint', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('lz-empty-state')).toBeInTheDocument();
    });
    expect(screen.getByText('No fleet detected')).toBeInTheDocument();
    expect(
      screen.getByText('Start LM Studio (LM Link) at http://192.168.8.220:1234 to see your nodes.')
    ).toBeInTheDocument();
    expect(screen.queryByTestId('fan-in-card')).toBeNull();
  });

  it('a live fleet renders the FanInCard, not the EmptyState', async () => {
    fleetState.online = true;
    fleetState.lanes = [{ device: 'workhorse' }];
    render();
    await waitFor(() => {
      expect(screen.getByTestId('fan-in-card')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('lz-empty-state')).toBeNull();
  });

  it('node weights are a DataTable keyed by device id: a node-hue dot per row, the map weight shown, a stepper that writes speed_weights', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByRole('table', { name: 'Node weights' })).toBeInTheDocument();
    });
    const rows = document.querySelectorAll('[data-testid="lz-row"]');
    expect([...rows].map((r) => r.getAttribute('data-key'))).toEqual(['workhorse-mlx', 'zai-glm']);
    // one identity dot per node row, plus the fleet status dot in the header line
    expect(screen.getAllByTestId('lz-status-dot').length).toBe(3);
    // the substring key `zai` resolves to 3 for zai-glm, exactly as the scheduler reads it
    await userEvent.click(screen.getByRole('button', { name: 'More work (zai-glm)' }));
    await waitFor(() => expect(mockUpsert).toHaveBeenCalled());
    const payload = mockUpsert.mock.calls[mockUpsert.mock.calls.length - 1][1] as SwarmConfig;
    expect(payload.speed_weights).toEqual({ zai: 4 });
  });

  it('every toggle is a role=switch button and the panel is Studio-clean with every class compiled', async () => {
    const { container } = render();
    await waitFor(() => {
      expect(screen.getByText('Golden formula')).toBeInTheDocument();
    });
    expect(screen.getAllByRole('switch').length).toBeGreaterThanOrEqual(4);
    expect(container.querySelector('select')).toBeNull();
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

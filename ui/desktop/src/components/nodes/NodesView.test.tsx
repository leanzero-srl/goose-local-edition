import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import type { ReactNode } from 'react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { GlanceNodesState } from '../engineGlance/glanceStore';
import type { SwarmConfig } from '../settings/swarm/golden';
import { CONFIG, NODE_CLOUD, NODE_FLASH, NODE_SPLIT } from './nodeGlance.fixtures';
import type { NodesConfig } from './model';

/**
 * The page shell (§8.2, §9 S6): the two uses above the tabs, the cards, and Your swarm pool — the
 * REAL SwarmNodesSection, because the shell's promise is that the pool stays the one writer of
 * `swarm` and its Share stepper still writes speed_weight. The tab bodies have their own suites.
 */

const store = vi.hoisted(() => ({ state: { kind: 'unread' } as unknown }));
vi.mock('../engineGlance/glanceStore', () => ({
  useGlanceNodes: () => store.state,
  refreshGlanceNodes: vi.fn(),
}));
vi.mock('./NodesTab', () => ({
  NodesTab: ({ onEditInPool }: { onEditInPool: () => void }) => (
    <button onClick={onEditInPool}>stub: Edit in your swarm pool</button>
  ),
}));
vi.mock('./StrategiesTab', () => ({ StrategiesTab: () => <div data-testid="strategies-body" /> }));
const mockEligibility = vi.fn();
vi.mock('../../acp/nodes', () => ({
  nodesBuildEligibility: (...a: unknown[]) => mockEligibility(...a),
  nodesWrite: vi.fn(),
}));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

// SwarmNodesSection's own doors, as its suite mocks them.
const mockRead = vi.fn();
const mockUpsert = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead, upsert: mockUpsert }),
}));
vi.mock('../swarm/useFleet', () => ({
  useFleet: () => ({ lanes: [], models: [], online: false, loading: false, endpoint: '' }),
  deviceFromModelId: (id: string) => id,
}));
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({ useLmStudioFleetVisible: () => false }));
vi.mock('../../acp/providers', () => ({ acpListProviderDetails: async () => [] }));
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineModelsList: async () => ({ models: [], diskAvailableBytes: 0, diskTotalBytes: 0 }),
  mlxEngineSettingsRead: async () => ({ modelId: '', modelsDir: '/x', port: 8090 }),
  mlxEngineSettingsUpdate: async (s: unknown) => s,
  mlxEngineStatus: async () => ({ state: 'stopped', restartRequired: false }),
}));
vi.mock('../../acp/mlx-serving-intent', () => ({
  mlxServingIntent: async () => ({ intent: null, error: null }),
}));
vi.mock('../../acp/leanzero-link', async (importActual) => ({
  ...(await importActual<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkNodes: async () => ({ self: null, peers: [] }),
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

import NodesView from './NodesView';

const POOL: SwarmConfig = {
  endpoint: 'http://localhost:1234',
  devices: [
    {
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 2,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
    },
  ],
} as SwarmConfig;

function readState(config: NodesConfig): GlanceNodesState {
  const nodes = [NODE_SPLIT, NODE_FLASH, NODE_CLOUD];
  return {
    kind: 'read',
    read: { config, nodes, stored: true, lmStudioHidden: 0 },
    residency: { nodes: [], serving: null, loaderInstalled: false },
    servedNode: null,
  };
}

function renderAt(path: string) {
  return render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[path]}>
        <NodesView />
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

beforeEach(() => {
  store.state = readState(CONFIG);
  mockRead.mockResolvedValue(POOL);
  mockUpsert.mockResolvedValue(undefined);
  mockEligibility.mockResolvedValue({ eligible: false, reasons: [], notes: [] });
  Object.assign(window.electron as unknown as Record<string, unknown>, { swarmCloud: vi.fn() });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('the Nodes page shell', () => {
  it('the two uses sit above the tabs and read what is stored', () => {
    renderAt('/nodes');
    const uses = screen.getByTestId('use-selectors');
    expect(within(uses).getByRole('combobox', { name: 'New chats start on:' })).toHaveTextContent(
      'Everyday (strategy)'
    );
    expect(within(uses).getByRole('combobox', { name: 'Swarm builds use:' })).toHaveTextContent(
      'Your swarm pool'
    );
  });

  it('asks goosed whether builds can use each stored strategy (never guessed)', async () => {
    renderAt('/nodes');
    await waitFor(() => expect(mockEligibility).toHaveBeenCalledWith('everyday'));
  });

  it('while builds use the pool, the pool is open — the real table, whose Share still writes speed_weight', async () => {
    renderAt('/nodes');
    const pool = screen.getByTestId('nodes-pool');
    expect(pool).toHaveAttribute('data-open', 'true');
    expect(within(pool).queryByTestId('nodes-pool-disclosure')).toBeNull();
    const more = await within(pool).findByRole('button', { name: 'More work (workhorse-mlx)' });
    await userEvent.click(more);
    await waitFor(() => expect(mockUpsert).toHaveBeenCalled());
    const [key, payload] = mockUpsert.mock.calls[mockUpsert.mock.calls.length - 1];
    expect(key).toBe('swarm');
    expect((payload as SwarmConfig).devices?.[0]).toMatchObject({
      id: 'workhorse-mlx',
      speed_weight: 2,
      weight: 2,
    });
  });

  it('while builds use a strategy, the pool folds into a Disclosure; Edit in your swarm pool opens it', async () => {
    store.state = readState({ ...CONFIG, forBuilds: { kind: 'strategy', id: 'everyday' } });
    renderAt('/nodes');
    const disclosure = screen.getByTestId('nodes-pool-disclosure');
    expect(disclosure).toHaveAttribute('data-state', 'closed');
    await userEvent.click(screen.getByText('stub: Edit in your swarm pool'));
    expect(screen.getByTestId('nodes-pool-disclosure')).toHaveAttribute('data-state', 'open');
    expect(screen.getByTestId('nodes-pool')).toHaveAttribute('data-open', 'true');
  });

  it('the Strategies tab shows the strategies, and neither the cards nor the pool', () => {
    renderAt('/nodes?tab=strategies');
    expect(screen.getByTestId('strategies-body')).toBeInTheDocument();
    expect(screen.queryByTestId('nodes-pool')).toBeNull();
    expect(screen.getByTestId('use-selectors')).toBeInTheDocument();
  });

  it('before the nodes are read, no use is claimed', () => {
    store.state = { kind: 'unread' };
    renderAt('/nodes');
    expect(screen.queryByTestId('use-selectors')).toBeNull();
  });
});

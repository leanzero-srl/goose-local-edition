import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { PROVIDER_ROUTES } from '../nodes/providerRoutes';
import type { MlxEngineStatus, MlxLocalModel } from '../../acp/mlx-engine';
import type { NodeState } from '../../acp/leanzero-link';

/**
 * LeanZero MLX's four inner tabs through the REAL Providers view and the real engine view (Q-194,
 * Q-203): Engine · My Macs · Models · Sampling in the URL as `mlx=`, written in place on every
 * click, restored by Back, and the setup strip above them opening each step's place. The engine
 * suite (MlxEngineView.test.tsx) owns what each tab shows; here only the routing is under test.
 */

const GB = 1024 * 1024 * 1024;
const QWEN = 'mlx-community/Qwen3-30B-A3B-4bit';
const MODELS: MlxLocalModel[] = [{ id: QWEN, sizeBytes: 17 * GB, complete: true, missingFiles: 0 }];

const mockStatus = vi.fn();
vi.mock('../../acp/mlx-engine', async (importOriginal) => ({
  MlxMountRefusedError: (await importOriginal<typeof import('../../acp/mlx-engine')>())
    .MlxMountRefusedError,
  mlxEngineStatus: (...args: unknown[]) => mockStatus(...args),
  mlxEngineMount: vi.fn(),
  mlxEngineUnmount: vi.fn(),
  mlxEngineSettingsRead: async () => ({
    modelId: QWEN,
    modelsDir: '/Users/x/mlx-models',
    port: 9600,
    servedModelName: 'leanzero-mlx',
    spawnCommand: ['uvx', 'rapid-mlx', 'serve'],
    modelProfiles: {},
  }),
  mlxEngineSettingsUpdate: async (s: unknown) => s,
  mlxEngineModelsList: async () => ({
    models: MODELS,
    diskAvailableBytes: 250 * GB,
    diskTotalBytes: 500 * GB,
  }),
  mlxEngineModelDelete: vi.fn(),
  mlxEngineBrowse: async () => ({ hits: [] }),
  mlxEngineBrowseFilters: async () => ({
    quants: [],
    archs: [],
    authors: [],
    sampledRepos: 0,
    computedAt: 0,
  }),
  mlxEngineModelCard: vi.fn(),
  mlxEngineDownload: vi.fn(),
  mlxEngineDownloadProgress: async () => null,
  mlxEngineDownloadCancel: vi.fn(),
  mlxEngineDownloadPause: vi.fn(),
  mlxEngineDownloadResume: vi.fn(),
}));
vi.mock('../../acp/mlx-replica', () => ({
  mlxEngineReplicaTargets: async () => ({ meshConnected: false, targets: [] }),
  mlxEngineReplicate: vi.fn(),
  mlxEngineReplicaProgress: async () => null,
  mlxEngineReplicaCancel: vi.fn(),
}));

const features = vi.hoisted(() => ({ leanzeroLink: true }));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: true,
    mlxEngine: true,
    mlxDistributed: false,
    leanzeroLink: features.leanzeroLink,
    isLoading: false,
  }),
}));
vi.mock('../../acp/mlx-distributed', async (importOriginal) => ({
  foreignOwner: (await importOriginal<typeof import('../../acp/mlx-distributed')>()).foreignOwner,
  latestMlxDistributedStatus: () => null,
  subscribeMlxDistributedStatus: () => () => undefined,
  mlxDistributedStatus: async () => null,
  mlxDistributedPreflight: vi.fn(),
  mlxDistributedStart: vi.fn(),
  mlxDistributedStop: vi.fn(),
  mlxDistributedConfigUpdate: vi.fn(),
}));
vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: async () => {
    throw new Error('no ACP client in this test');
  },
}));
const SELF_NODE: NodeState = {
  node_id: 'self-node',
  hostname: 'this-mac',
  mesh_ip: '100.64.0.1',
  status: { type: 'Idle' },
  sessions_active: 0,
  updated_at: '2026-09-01T12:00:00Z',
};
vi.mock('../../acp/leanzero-link', async (importActual) => ({
  ...(await importActual<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkStatus: async () => ({ auth: { state: 'loggedOut' }, nodeCount: 0 }),
  leanzeroLinkNodes: async () => ({ self: SELF_NODE, peers: [] }),
}));
vi.mock('../../acp/mlx-remote-single', async (importActual) => ({
  ...(await importActual<typeof import('../../acp/mlx-remote-single')>()),
  latestMlxRemoteSingleStatus: () => null,
  subscribeMlxRemoteSingleStatus: () => () => undefined,
  mlxRemoteSingleStatus: async () => ({ state: 'off' }),
  mlxRemoteSingleStop: vi.fn(),
}));

// My Macs is LeanZeroLinkSection, whole; its own suite covers it.
vi.mock('./LeanZeroLinkSection', () => ({ default: () => <div data-testid="my-macs-panel" /> }));
vi.mock('./CloudProvidersSection', () => ({ default: () => <div data-testid="cloud-panel" /> }));
vi.mock('./SwarmNodesSection', () => ({
  default: () => <div data-testid="swarm-nodes-section" />,
}));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
// The setup strip's node facts come from the glance store's nodes read: one node, nothing serving.
vi.mock('../../acp/nodes', () => ({
  nodesRead: vi.fn(async () => ({
    config: { version: 1 },
    // A whole ResolvedNodeDef: the Nodes step now lands on the real node cards (S6), which read
    // every field goosed sends.
    nodes: [
      {
        def: {
          id: 'mihai-mlx',
          name: 'Mihai Macbook engine',
          kind: 'mlx',
          placement: { kind: 'follows' },
          poolDevice: 'mihai-mlx',
          origin: 'pool',
        },
        model: 'qwen3.8-27b',
        modelFrom: { kind: 'pool' },
      },
    ],
    stored: false,
    lmStudioHidden: 0,
  })),
  nodesResidency: vi.fn(async () => ({ nodes: [], loaderInstalled: false })),
  nodesServedLast: vi.fn(async () => ({})),
}));
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: async () => ({ devices: [{ id: 'mihai-mlx' }] }) }),
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

function Where() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <output data-testid="where">{location.pathname + location.search}</output>
      <button onClick={() => navigate('/elsewhere')}>test: leave</button>
      <button onClick={() => navigate(-1)}>test: back</button>
    </>
  );
}

function renderAt(path: string) {
  return render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          {PROVIDER_ROUTES.map((route) => (
            <Route key={route.path} path={route.path} element={route.element} />
          ))}
          <Route path="elsewhere" element={<div data-testid="elsewhere" />} />
        </Routes>
        <Where />
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

const where = () => screen.getByTestId('where').textContent;
const engineTabs = () => screen.getByRole('radiogroup', { name: 'Engine sections' });
const innerTab = (name: RegExp) => within(engineTabs()).getByRole('radio', { name });
const statusOf = (overrides: Partial<MlxEngineStatus>): MlxEngineStatus => ({
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 40.2,
  totalMemoryGb: 64,
  ...overrides,
});

beforeEach(() => {
  sessionStorage.clear();
  features.leanzeroLink = true;
  mockStatus.mockResolvedValue(statusOf({}));
});
afterEach(cleanup);

describe('LeanZero MLX inner tabs, routed', () => {
  it('four tabs, My Macs second: Engine · My Macs · Models · Sampling', async () => {
    renderAt('/leanzero-swarm?tab=mlx');
    await act(async () => {});
    expect(
      within(engineTabs())
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['Engine', 'My Macs', 'Models1', 'Sampling']);
    expect(innerTab(/^Engine$/)).toHaveAttribute('aria-checked', 'true');
  });

  it('without LeanZero Link there is no My Macs tab, and a My Macs link opens the Engine tab', async () => {
    features.leanzeroLink = false;
    renderAt('/leanzero-swarm?tab=mlx&mlx=macs');
    await act(async () => {});
    expect(
      within(engineTabs())
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['Engine', 'Models1', 'Sampling']);
    expect(innerTab(/^Engine$/)).toHaveAttribute('aria-checked', 'true');
    expect(screen.queryByTestId('my-macs-panel')).not.toBeInTheDocument();
  });

  it('each click writes mlx= in place and mounts that tab; My Macs is the Link section', async () => {
    renderAt('/leanzero-swarm?tab=mlx');
    await act(async () => {});
    await userEvent.click(innerTab(/^My Macs$/));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=macs');
    expect(screen.getByTestId('my-macs-panel')).toBeInTheDocument();
    await userEvent.click(innerTab(/^Models/));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=models');
    expect(screen.queryByTestId('my-macs-panel')).not.toBeInTheDocument();
    await userEvent.click(innerTab(/^Sampling$/));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=sampling');
    await userEvent.click(innerTab(/^Engine$/));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=engine');
  });

  it('Back from another page restores the tab that was open', async () => {
    renderAt('/leanzero-swarm?tab=mlx');
    await act(async () => {});
    await userEvent.click(innerTab(/^Sampling$/));
    await userEvent.click(screen.getByText('test: leave'));
    expect(screen.getByTestId('elsewhere')).toBeInTheDocument();
    await userEvent.click(screen.getByText('test: back'));
    await act(async () => {});
    expect(innerTab(/^Sampling$/)).toHaveAttribute('aria-checked', 'true');
  });

  it('a deep link opens Models or Sampling directly (Q-203)', async () => {
    renderAt('/leanzero-swarm?tab=mlx&mlx=sampling');
    await act(async () => {});
    expect(innerTab(/^Sampling$/)).toHaveAttribute('aria-checked', 'true');
    cleanup();
    renderAt('/leanzero-swarm?tab=mlx&mlx=models');
    await act(async () => {});
    expect(innerTab(/^Models/)).toHaveAttribute('aria-checked', 'true');
  });

  it('the setup strip sits above the tabs and each step opens its place: My Macs, Models, Engine, Nodes', async () => {
    renderAt('/leanzero-swarm?tab=mlx&mlx=sampling');
    const strip = await screen.findByTestId('mlx-setup-steps');
    expect(
      strip.compareDocumentPosition(engineTabs()) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // Signed out of Link: step 1 is Next; the one model on this Mac makes step 2 done; nothing
    // runs; the nodes read found one node.
    await waitFor(() =>
      expect(within(strip).getByTestId('mlx-setup-step-4')).toHaveTextContent('1 node')
    );
    expect(within(strip).getByTestId('mlx-setup-step-1')).toHaveAttribute('data-state', 'next');
    expect(within(strip).getByTestId('mlx-setup-step-3')).toHaveTextContent('Run a model');

    await userEvent.click(within(strip).getByTestId('mlx-setup-step-1'));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=macs');
    await userEvent.click(within(strip).getByTestId('mlx-setup-step-2'));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=models');
    await userEvent.click(within(strip).getByTestId('mlx-setup-step-3'));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=engine');
    await userEvent.click(within(strip).getByTestId('mlx-setup-step-4'));
    expect(where()).toBe('/nodes');
  });

  it('step 3 names the model the engine is running', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, servedModelId: 'leanzero-mlx' })
    );
    renderAt('/leanzero-swarm?tab=mlx');
    await waitFor(() =>
      expect(screen.getAllByTestId('mlx-setup-step-3')[0]).toHaveTextContent(
        'Qwen3-30B-A3B-4bit running'
      )
    );
    expect(screen.getAllByTestId('mlx-setup-step-3')[0]).toHaveAttribute('data-state', 'done');
  });
});

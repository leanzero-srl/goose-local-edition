import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { assertStudioClean } from '../lz/assertStudioClean';
import { ComposerReadinessStrip, mlxProviderReadiness, swarmReadiness } from './ComposerReadiness';
import type { MountLookup } from './mlxMount';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
const mockMount = vi.fn<(modelId: string) => Promise<void>>();
const mockSettings = vi.fn<() => Promise<MlxEngineSettings>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: (modelId: string) => mockMount(modelId),
  mlxEngineSettingsRead: () => mockSettings(),
}));
const mockExtMethod = vi.fn();
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));
const mockReadConfig = vi.fn();
vi.mock('../../acp/config', () => ({
  acpReadConfig: (key: string) => mockReadConfig(key),
}));

/** The audit's machine (2026-09-23): the swarm's only node, mihai-mlx, on the local MLX engine. */
const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const MLX_NODE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const LM_NODE: SwarmDeviceRow = { id: 'studio-lm', model_id: 'qwen', weight: 1, enabled: true };
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
};
const RUNNING: MlxEngineStatus = {
  ...STOPPED,
  state: 'running',
  modelId: HF,
  servedModelId: ALIAS,
};

/** Live 2026-09-24: the owner's 27B split over the two Macs, serving on :8091. `servedModelId` is
 *  what the ranks serve since the one-rule fix — the single engine's alias. */
const DIST_READY: MlxDistributedStatus = {
  ...FLASH_READY,
  runner: 'mlxLmTensor',
  modelId: HF,
  servedModelId: ALIAS,
  baseUrl: 'http://127.0.0.1:8091',
};
const DIST_STARTING: MlxDistributedStatus = { ...DIST_READY, state: 'starting' };
/** What 3.0.19 served: the HF id, which the node does not name. */
const DIST_HF_ID: MlxDistributedStatus = { ...DIST_READY, servedModelId: HF };

const ready = (devices: SwarmDeviceRow[]): MountLookup => ({
  state: 'ready',
  devices,
  settings: SETTINGS,
});

describe('swarmReadiness — only what the renderer can know', () => {
  it('the audit case: the only enabled node is an unmounted local MLX node → unmounted, with its mount target', () => {
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, null)).toEqual({
      kind: 'unmounted',
      nodes: ['mihai-mlx'],
      target: { kind: 'ok', modelId: HF, servedId: ALIAS },
      fact: 'down',
    });
  });

  it('the engine serving the node’s alias is ready', () => {
    expect(swarmReadiness(ready([MLX_NODE]), RUNNING, null)).toEqual({ kind: 'ready' });
  });

  it('zero ENABLED nodes can never serve (enabled absent = false, as the router reads it)', () => {
    const { enabled: _e, ...unset } = MLX_NODE;
    expect(swarmReadiness(ready([{ ...MLX_NODE, enabled: false }]), STOPPED, null)).toEqual({
      kind: 'no-nodes',
    });
    expect(swarmReadiness(ready([unset as SwarmDeviceRow]), STOPPED, null)).toEqual({
      kind: 'no-nodes',
    });
  });

  it('a node this surface cannot probe (LM Studio, cloud, remote MLX) makes the answer unknown', () => {
    expect(swarmReadiness(ready([MLX_NODE, LM_NODE]), STOPPED, null)).toEqual({ kind: 'unknown' });
    expect(swarmReadiness(ready([{ ...MLX_NODE, provider: 'bedrock' }]), STOPPED, null)).toEqual({
      kind: 'unknown',
    });
    expect(swarmReadiness(ready([{ ...MLX_NODE, host: 'studio' }]), STOPPED, null)).toEqual({
      kind: 'unknown',
    });
  });

  it('no status yet, a failed read, a stray listener, or an unread pool are unknown — never red', () => {
    expect(swarmReadiness(ready([MLX_NODE]), null, null)).toEqual({ kind: 'unknown' });
    expect(
      swarmReadiness(ready([MLX_NODE]), { ...STOPPED, strayListenerPort: 8090 }, null)
    ).toEqual({
      kind: 'unknown',
    });
    expect(swarmReadiness({ state: 'loading' }, STOPPED, null)).toEqual({ kind: 'unknown' });
    expect(swarmReadiness({ state: 'failed', error: 'x' }, STOPPED, null)).toEqual({
      kind: 'unknown',
    });
  });

  it('a mounting engine is carried as the fact, not as a failure', () => {
    const r = swarmReadiness(
      ready([MLX_NODE]),
      { ...STOPPED, state: 'mounting', modelId: HF },
      null
    );
    expect(r).toMatchObject({ kind: 'unmounted', fact: 'mounting' });
  });
});

describe('swarmReadiness — the distributed engine owns this Mac', () => {
  it('serving the node’s id is ready — the single engine’s stopped status is moot', () => {
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, DIST_READY)).toEqual({ kind: 'ready' });
    expect(
      swarmReadiness(ready([MLX_NODE]), { ...STOPPED, strayListenerPort: 8090 }, DIST_READY)
    ).toEqual({ kind: 'ready' });
  });

  it('starting, or serving another id, is its own state — never an unmounted/Mount offer', () => {
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, DIST_STARTING)).toEqual({
      kind: 'distributed',
      nodes: ['mihai-mlx'],
      status: DIST_STARTING,
      wanted: ALIAS,
    });
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, DIST_HF_ID)).toMatchObject({
      kind: 'distributed',
      wanted: ALIAS,
    });
  });

  it('a distributed status that does not own the Mac leaves the single engine in charge', () => {
    const stopped: MlxDistributedStatus = { ...DIST_READY, mode: 'single', state: 'stopped' };
    expect(swarmReadiness(ready([MLX_NODE]), RUNNING, stopped)).toEqual({ kind: 'ready' });
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, stopped)).toMatchObject({
      kind: 'unmounted',
    });
  });
});

describe('mlxProviderReadiness', () => {
  it('stopped engine → unmounted with the saved model; running → ready; unknown status → unknown', () => {
    expect(mlxProviderReadiness(SETTINGS, STOPPED, null, 'LeanZero MLX')).toMatchObject({
      kind: 'unmounted',
      nodes: ['LeanZero MLX'],
      target: { kind: 'ok', modelId: HF },
    });
    expect(mlxProviderReadiness(SETTINGS, RUNNING, null, 'LeanZero MLX')).toEqual({
      kind: 'ready',
    });
    expect(mlxProviderReadiness(SETTINGS, null, null, 'LeanZero MLX')).toEqual({ kind: 'unknown' });
  });

  it('the omlx provider follows the distributed engine while it owns the Mac', () => {
    expect(mlxProviderReadiness(SETTINGS, STOPPED, DIST_READY, 'LeanZero MLX')).toEqual({
      kind: 'ready',
    });
    expect(mlxProviderReadiness(SETTINGS, STOPPED, DIST_STARTING, 'LeanZero MLX')).toMatchObject({
      kind: 'distributed',
      nodes: ['LeanZero MLX'],
    });
  });
});

function wrap(provider: string) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter initialEntries={['/pair']}>
        <Routes>
          <Route
            path="/pair"
            element={
              <div>
                <ComposerReadinessStrip provider={provider} />
                <textarea data-testid="composer" />
              </div>
            }
          />
          <Route path="/leanzero-swarm" element={<div data-testid="providers-view" />} />
        </Routes>
      </MemoryRouter>
    </IntlProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockStatus.mockResolvedValue(STOPPED);
  mockMount.mockResolvedValue(undefined);
  mockSettings.mockResolvedValue(SETTINGS);
  mockReadConfig.mockResolvedValue({ devices: [MLX_NODE] });
});

describe('ComposerReadinessStrip (UX audit C1)', () => {
  it('says so BEFORE sending: a solid warning naming the node, Mount with the short model name', async () => {
    const { container } = wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain('No model is mounted — mihai-mlx');
    expect(strip.className).toContain('bg-lz-warn-solid');
    const mount = screen.getByTestId('composer-readiness-mount');
    expect(mount.textContent).toBe('Mount Qwen3.8-27B-Atlassian-Q8-mlx');
    expect(mount.getAttribute('title')).toBe(HF);
    expect(screen.getByTestId('composer')).not.toBeDisabled();
    assertStudioClean(container);
  });

  it('Mount mounts the saved model and the strip shows Mounting until the engine answers', async () => {
    const user = userEvent.setup();
    wrap('swarm');
    const mount = await screen.findByTestId('composer-readiness-mount');
    mockStatus.mockResolvedValue({ ...STOPPED, state: 'mounting', modelId: HF });
    await user.click(mount);
    expect(mockMount).toHaveBeenCalledWith(HF);
    expect(await screen.findByTestId('composer-readiness-mounting')).toBeInTheDocument();
    expect(screen.queryByTestId('composer-readiness-mount')).toBeNull();
  });

  it('a refused mount is said verbatim and Mount stays', async () => {
    const user = userEvent.setup();
    mockMount.mockRejectedValue(new Error('not enough memory: 18 GB free, 30 GB needed'));
    wrap('swarm');
    await user.click(await screen.findByTestId('composer-readiness-mount'));
    expect((await screen.findByTestId('composer-readiness-detail')).textContent).toContain(
      'not enough memory: 18 GB free, 30 GB needed'
    );
    expect(screen.getByTestId('composer-readiness-mount')).toBeInTheDocument();
  });

  it('renders nothing once the engine serves the node', async () => {
    mockStatus.mockResolvedValue(RUNNING);
    wrap('swarm');
    await waitFor(() => expect(mockStatus).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId('composer-readiness')).toBeNull();
  });

  it('renders nothing for a provider whose readiness it cannot know — and never polls it', async () => {
    wrap('anthropic');
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByTestId('composer-readiness')).toBeNull();
    expect(mockReadConfig).not.toHaveBeenCalled();
    expect(mockStatus).not.toHaveBeenCalled();
  });

  it('a distributed run read ANYWHERE in the window replaces the Mount offer with its state', async () => {
    mockExtMethod.mockResolvedValue({ status: DIST_STARTING });
    await mlxDistributedStatus();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain('Distributed · 2 nodes · JACCL · Starting — mihai-mlx');
    expect(strip.textContent).not.toContain('No model is mounted');
    expect(screen.getByTestId('composer-readiness-distributed-starting')).toBeInTheDocument();
    expect(screen.queryByTestId('composer-readiness-mount')).toBeNull();

    mockExtMethod.mockResolvedValue({ status: DIST_READY });
    await mlxDistributedStatus();
    await waitFor(() => expect(screen.queryByTestId('composer-readiness')).toBeNull());

    mockExtMethod.mockRejectedValue(new Error('goosed gone'));
    await expect(mlxDistributedStatus()).rejects.toThrow('goosed gone');
    expect(await screen.findByTestId('composer-readiness-mount')).toBeInTheDocument();
  });

  it('Open Providers goes to the Providers view', async () => {
    const user = userEvent.setup();
    wrap('swarm');
    await user.click(await screen.findByTestId('composer-readiness-open-providers'));
    expect(screen.getByTestId('providers-view')).toBeInTheDocument();
  });
});

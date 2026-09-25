import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { assertStudioClean } from '../lz/assertStudioClean';
import { ComposerReadinessStrip } from './ComposerReadiness';
import { mlxProviderReadiness, swarmReadiness } from '../chatServedBy/chatServedBy';
import { useChatServedBy } from '../chatServedBy/useChatServedBy';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import { parseMlxLiveStatus, EMPTY_BOOK } from '../leanzero-swarm/mlxLiveStats';
import { GENERATING_STATUS, PREFILL_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';
import type { MountLookup } from './mlxMount';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';
import { mlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import { publishRestoreLine } from '../leanzero-swarm/mlxRestore';

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
const mockMount = vi.fn<(modelId: string) => Promise<void>>();
const mockSettings = vi.fn<() => Promise<MlxEngineSettings>>();
const mockUnmount = vi.fn<(nodeId?: string) => Promise<void>>();
const mockModelsList = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: (nodeId?: string) => mockUnmount(nodeId),
  mlxEngineModelsList: () => mockModelsList(),
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
/** A second window's goosed supervises nothing; the first window's run reaches it as `owner`. */
const OTHER_WINDOW: MlxDistributedStatus = {
  mode: 'single',
  state: 'stopped',
  admissionOpen: true,
  nodes: [],
  events: [],
  restarts: 0,
  owner: {
    state: 'answering',
    pid: 4242,
    baseUrl: 'http://127.0.0.1:8191',
    servedModelId: ALIAS,
    modelId: HF,
    backend: 'jaccl',
    nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
  },
};
const OTHER_WINDOW_LOADING: MlxDistributedStatus = {
  ...OTHER_WINDOW,
  owner: {
    ...OTHER_WINDOW.owner!,
    state: 'notAnswering',
    detail: 'GET http://127.0.0.1:8191/v1/models failed (connection refused)',
  },
};
/** What 3.0.19 served: the HF id, which the node does not name. */
const DIST_HF_ID: MlxDistributedStatus = { ...DIST_READY, servedModelId: HF };

const ready = (devices: SwarmDeviceRow[]): MountLookup => ({
  state: 'ready',
  devices,
  settings: SETTINGS,
});

describe('swarmReadiness — only what the renderer can know', () => {
  it('a remote-single route is where chat goes: said even while it serves, before any local fact', () => {
    const remote = {
      state: 'ready',
      peer: 'worksmacstudio-lan-9c1e2a',
      peerHostname: 'worksmacstudio-lan-9c1e2a',
      modelId: HF,
      servedModelId: ALIAS,
    };
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, null, remote)).toEqual({
      kind: 'remote',
      status: remote,
    });
    expect(swarmReadiness(ready([]), null, null, remote).kind).toBe('remote');
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, null, { state: 'off' }).kind).toBe(
      'unmounted'
    );
  });

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

describe('swarmReadiness — another window’s goosed owns the distributed engine', () => {
  it('its engine answering the node’s id is ready; not answering is its own state, never Mount', () => {
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, OTHER_WINDOW)).toEqual({ kind: 'ready' });
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, OTHER_WINDOW_LOADING)).toMatchObject({
      kind: 'distributed',
      wanted: ALIAS,
    });
  });

  it('a stale record owns nothing: the single engine’s rules apply', () => {
    const stale: MlxDistributedStatus = {
      ...OTHER_WINDOW,
      owner: { ...OTHER_WINDOW.owner!, state: 'stale' },
    };
    expect(swarmReadiness(ready([MLX_NODE]), STOPPED, stale)).toMatchObject({
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

/** The composer's one read of where chat goes, handed to the bar — as ChatInput does. */
function Composer({ provider, sessionId }: { provider: string; sessionId: string | null }) {
  const serving = useChatServedBy(provider, sessionId, false);
  return <ComposerReadinessStrip serving={serving} />;
}

function wrap(provider: string, sessionId: string | null = null) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter initialEntries={['/pair']}>
        <Routes>
          <Route
            path="/pair"
            element={
              <div>
                <Composer provider={provider} sessionId={sessionId} />
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
  // This Mac's models dir holds the 27B (31.0 GiB on the owner's machine).
  mockModelsList.mockResolvedValue({
    models: [{ id: HF, sizeBytes: 31 * 1024 ** 3, complete: true, missingFiles: 0 }],
    diskAvailableBytes: 0,
    diskTotalBytes: 0,
  });
});

describe('ComposerReadinessStrip — a relaunch bringing back what served', () => {
  it('says what comes back and where instead of "No model is mounted" + Mount; a failure says why', async () => {
    wrap('swarm');
    await screen.findByTestId('composer-readiness-mount');
    act(() =>
      publishRestoreLine({
        phase: 'restoring',
        what: { kind: 'single', modelId: HF, peerName: null },
      })
    );
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'restore-restoring');
    expect(strip.textContent).toContain('Restoring Qwen3.8-27B-Atlassian-Q8-mlx on this Mac…');
    expect(strip.className).toContain('bg-lz-phase-loading');
    expect(screen.queryByTestId('composer-readiness-mount')).toBeNull();

    act(() =>
      publishRestoreLine({
        phase: 'failed',
        what: { kind: 'single', modelId: HF, peerName: null },
        reason: { code: 'said', text: 'model needs 30.6 GB, 12.0 GB free' },
      })
    );
    expect(screen.getByTestId('composer-readiness').textContent).toContain(
      'Could not restore Qwen3.8-27B-Atlassian-Q8-mlx on this Mac: model needs 30.6 GB, 12.0 GB free'
    );
    expect(screen.getByTestId('mlx-restore-retry')).toBeInTheDocument();
    act(() => publishRestoreLine({ phase: 'idle' }));
    expect(await screen.findByTestId('composer-readiness-mount')).toBeInTheDocument();
  });
});

describe('ComposerReadinessStrip — a route to another Mac', () => {
  const ROUTE = {
    peer: 'worksmacstudio-lan-9c1e2a',
    peerHostname: 'WorksMacStudio.lan',
    peerComputerName: "Work's Mac Studio",
    modelId: HF,
  };
  afterEach(async () => {
    mockExtMethod.mockResolvedValue({ status: { state: 'off' } });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
  });

  it('while the route LOADS: names the model and that Mac the one way (its owner’s name), never its mesh hostname', async () => {
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'mounting' } });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain(
      "Loading Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio — a message waits until it is ready"
    );
    expect(strip.className).toContain('bg-lz-phase-loading');
    expect(strip.textContent).not.toContain('WorksMacStudio.lan');
    // One layout: the spinner leads, as in every state in progress (Q-63) — and loading HERE stays
    // one click away while it loads there (Q-57).
    expect(screen.getByTestId('composer-readiness-spinner')).toHaveAttribute('data-for', 'remote');
    expect(strip.firstElementChild).toBe(screen.getByTestId('composer-readiness-spinner'));
    // The size lands with this Mac's models list (read once the bar offers the load).
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-run-here')).toHaveTextContent(
        'Load Qwen3.8-27B-Atlassian-Q8-mlx here (31.0 GB)'
      )
    );
  });

  it('a FAILED route is red and says why in the peer’s words', async () => {
    mockExtMethod.mockResolvedValue({
      status: { ...ROUTE, state: 'failed', lastError: 'its engine exited (137)' },
    });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain("The engine on Work's Mac Studio failed");
    expect(strip.className).toContain('bg-lz-phase-failed');
    expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
      'its engine exited (137)'
    );
  });

  it('Q-8: a READY route renders NO bar — the chip names what serves; a permanent green "ready" is noise', async () => {
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'ready' } });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    await waitFor(() => expect(mockReadConfig).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId('composer-readiness')).toBeNull();
  });
});

describe('ComposerReadinessStrip — the Mac that serves chat stopped answering (Q-47)', () => {
  const ROUTE = {
    peer: 'worksmacstudio-lan-9c1e2a',
    peerHostname: 'WorksMacStudio.lan',
    peerComputerName: "Work's Mac Studio",
    modelId: HF,
    baseUrl: 'http://127.0.0.1:61001/relay/cafe',
  };
  afterEach(async () => {
    (window as unknown as { electron: unknown }).electron = {};
    mockExtMethod.mockReset();
    mockExtMethod.mockResolvedValue({ status: { state: 'off' } });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
  });

  it('the route says `reconnecting`: a solid amber bar names the Mac, spins, and offers to load the model here, named with its size', async () => {
    mockExtMethod.mockResolvedValue({
      status: { ...ROUTE, state: 'reconnecting', lastError: 'no answer from the Link peer' },
    });
    await mlxRemoteSingleStatus();
    const { container } = wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'reconnecting');
    expect(strip.textContent).toContain("Lost contact with Work's Mac Studio — reconnecting…");
    expect(strip.className).toContain('bg-lz-phase-loading');
    expect(strip.firstElementChild).toBe(screen.getByTestId('composer-readiness-spinner'));
    // Plain words on the bar; the read's own words only behind Details — never inline.
    expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
      // Q-52: nothing promises the answer continues.
      "goose keeps trying; once Work's Mac Studio answers, goose checks whether it still has your answer"
    );
    expect(strip.textContent).not.toContain('no answer from the Link peer');
    await userEvent.click(screen.getByTestId('composer-readiness-details'));
    expect(screen.getByTestId('composer-readiness-raw').textContent).toBe(
      'no answer from the Link peer'
    );
    // The size lands with this Mac's models list (read once the bar offers the load).
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-run-here')).toHaveTextContent(
        'Load Qwen3.8-27B-Atlassian-Q8-mlx here (31.0 GB)'
      )
    );
    assertStudioClean(container);
  });

  it('Q-54: a Mac that said it is restarting goose — the headline says so, and that this answer stops', async () => {
    mockExtMethod.mockResolvedValue({
      status: {
        ...ROUTE,
        state: 'reconnecting',
        lastError:
          "Work's Mac Studio does not answer over LeanZero Link right now: Work's Mac Studio is restarting goose",
      },
    });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain("Work's Mac Studio is restarting goose");
    expect(strip.textContent).not.toContain('checks whether it still has your answer');
    expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
      'goose reconnects when it is back'
    );
  });

  it('a model this Mac does not hold is never offered to load here (the mount would only fail)', async () => {
    mockModelsList.mockResolvedValue({ models: [], diskAvailableBytes: 0, diskTotalBytes: 0 });
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'reconnecting' } });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    await screen.findByTestId('composer-readiness');
    await waitFor(() => expect(mockModelsList).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByTestId('composer-readiness-run-here')).toBeNull();
  });

  it('a route read that FAILS is the same named state — the bar the recording left blank for ten seconds', async () => {
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'ready' } });
    await mlxRemoteSingleStatus();
    mockExtMethod.mockRejectedValue(new Error('remoteSingleStatus: no answer'));
    await expect(mlxRemoteSingleStatus()).rejects.toThrow();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'reconnecting');
    expect(strip.textContent).toContain("Lost contact with Work's Mac Studio");
  });

  it('main’s relay read failing while the route still says ready (kill-link, 11.6 s) raises it too', async () => {
    (window as unknown as { electron: unknown }).electron = {
      mlxEngineActivity: async (): Promise<MlxEngineSnapshot> => ({
        engine: 'remote',
        mode: 'reconnecting',
        modelId: null,
        baseUrl: ROUTE.baseUrl,
        stats: null,
        statusDetail: 'timeout: no answer within 1500 ms',
        rates: EMPTY_BOOK,
        serving: null,
        failedError: null,
      }),
    };
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'ready' } });
    await mlxRemoteSingleStatus();
    wrap('swarm', 's-mine');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'reconnecting');
    expect(strip.textContent).not.toContain('1500 ms');
    await userEvent.click(screen.getByTestId('composer-readiness-details'));
    expect(screen.getByTestId('composer-readiness-raw').textContent).toBe(
      'timeout: no answer within 1500 ms'
    );
  });

  it('Q-59: main’s PUSHED read clears the bar the moment the Mac answers — no wait for the next poll', async () => {
    const handlers: Record<string, (event: unknown, ...args: unknown[]) => void> = {};
    const lost: MlxEngineSnapshot = {
      engine: 'remote',
      mode: 'reconnecting',
      modelId: null,
      baseUrl: ROUTE.baseUrl,
      stats: null,
      statusDetail: 'timeout: no answer within 1500 ms',
      rates: EMPTY_BOOK,
      serving: null,
      failedError: null,
    };
    (window as unknown as { electron: unknown }).electron = {
      mlxEngineActivity: async () => lost,
      on: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => {
        handlers[channel] = fn;
      },
      off: () => undefined,
    };
    mockExtMethod.mockResolvedValue({ status: { ...ROUTE, state: 'ready' } });
    await mlxRemoteSingleStatus();
    wrap('swarm', 's-mine');
    expect(await screen.findByTestId('composer-readiness')).toHaveAttribute(
      'data-readiness',
      'reconnecting'
    );
    const idle = parseMlxLiveStatus(PREFILL_STATUS);
    if (!idle.ok) throw new Error(idle.detail);
    act(() =>
      handlers['mlx-engine-snapshot']?.(null, {
        ...lost,
        mode: 'running',
        stats: idle.stats,
        statusDetail: null,
        serving: { clients: [], unattributed: 0, swarmRuns: [], error: null },
      })
    );
    // Well inside one poll interval (2 s): only the push can have done it.
    await waitFor(() => expect(screen.queryByTestId('composer-readiness')).toBeNull(), {
      timeout: 500,
    });
  });

  it('"Run on this Mac instead" drops the route on THIS Mac and mounts here at once — never waiting on the Mac that is not answering', async () => {
    const calls: string[] = [];
    mockExtMethod.mockImplementation(async (method: string, params: unknown) => {
      calls.push(method);
      if (method.endsWith('remoteSingleStop')) {
        expect(params).toEqual({ keepMounted: true });
        return { unmounted: false, unmountError: null, status: { state: 'off' } };
      }
      return { status: { ...ROUTE, state: 'reconnecting' } };
    });
    // The Studio never answers the request to free its engine.
    mockUnmount.mockReturnValue(new Promise(() => undefined));
    mockMount.mockImplementation(async () => {
      calls.push('mount');
    });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    await userEvent.click(await screen.findByTestId('composer-readiness-run-here'));
    await waitFor(() => expect(mockMount).toHaveBeenCalledWith(HF));
    const stop = calls.indexOf('_goose/unstable/mlxEngine/remoteSingleStop');
    expect(stop).toBeGreaterThanOrEqual(0);
    expect(calls.indexOf('mount')).toBeGreaterThan(stop);
    expect(mockUnmount).toHaveBeenCalledWith(ROUTE.peer);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness')).toHaveAttribute(
        'data-readiness',
        'unmounted'
      )
    );
    // The Mac that still holds the model is a quiet line, not an error.
    const held = await screen.findByTestId('peer-held');
    expect(held.textContent).toContain(
      "Work's Mac Studio still holds the model — goose asked it to free it"
    );
    expect(held.className).not.toContain('bg-lz-phase-failed');
    await userEvent.click(screen.getByTestId('peer-held-dismiss'));
    expect(screen.queryByTestId('peer-held')).toBeNull();
  });

  it('a switch whose stop is refused says why in the bar and keeps the route’s state', async () => {
    mockExtMethod.mockImplementation(async (method: string) => {
      if (method.endsWith('remoteSingleStop'))
        throw new Error('remoteSingleActive: another window');
      return { status: { ...ROUTE, state: 'reconnecting' } };
    });
    await mlxRemoteSingleStatus();
    wrap('swarm');
    await userEvent.click(await screen.findByTestId('composer-readiness-run-here'));
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
        'Could not move chat to this Mac: remoteSingleActive: another window'
      )
    );
    expect(mockMount).not.toHaveBeenCalled();
  });
});

describe('ComposerReadinessStrip — the engine busy with another client (Q-17)', () => {
  const ROUTE = {
    state: 'ready',
    peer: 'worksmacstudio-lan-9c1e2a',
    peerHostname: 'WorksMacStudio.lan',
    peerComputerName: "Work's Mac Studio",
    modelId: HF,
  };
  /** The Studio reads one prompt while a request WAITS behind it — the only case a turn waits. */
  const PREFILL_WITH_WAITING = {
    ...PREFILL_STATUS,
    num_waiting: 1,
    requests: [GENERATING_STATUS.requests[0], ...PREFILL_STATUS.requests],
  };
  function snapshot(serving: MlxEngineSnapshot['serving']): MlxEngineSnapshot {
    const read = parseMlxLiveStatus(PREFILL_WITH_WAITING);
    if (!read.ok) throw new Error(read.detail);
    return {
      engine: 'remote',
      mode: 'running',
      modelId: HF,
      baseUrl: 'http://127.0.0.1:61001/relay/cafe',
      stats: read.stats,
      statusDetail: null,
      rates: EMPTY_BOOK,
      serving,
      failedError: null,
    };
  }
  afterEach(async () => {
    (window as unknown as { electron: unknown }).electron = {};
    mockExtMethod.mockResolvedValue({ status: { state: 'off' } });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
  });

  it('round 1, 08:42: the Studio reads another client’s prompt while a request waits — the bar says so, names the Mac, and the turn will wait', async () => {
    (window as unknown as { electron: unknown }).electron = {
      mlxEngineActivity: async () =>
        snapshot({ clients: [], unattributed: 1, swarmRuns: [], error: null }),
    };
    mockExtMethod.mockResolvedValue({ status: ROUTE });
    await mlxRemoteSingleStatus();
    wrap('swarm', 's-mine');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'busy');
    expect(strip.textContent).toMatch(
      /^Work's Mac Studio is reading another request’s [\d.]+k?-token prompt — your message waits its turn/
    );
    expect(strip.className).toContain('bg-lz-phase-held');
    expect(strip.textContent).not.toContain('ready');
    expect(screen.getByTestId('composer-readiness-open-engine')).toBeInTheDocument();
  });

  it('this chat’s own request is never "another request" — no bar', async () => {
    (window as unknown as { electron: unknown }).electron = {
      mlxEngineActivity: async () =>
        snapshot({
          clients: [
            { key: 'chat:s-mine', kind: 'chat', sessionId: 's-mine', sessionName: 'x', count: 1 },
          ],
          unattributed: 0,
          swarmRuns: [],
          error: null,
        }),
    };
    mockExtMethod.mockResolvedValue({ status: ROUTE });
    await mlxRemoteSingleStatus();
    wrap('swarm', 's-mine');
    await waitFor(() => expect(mockReadConfig).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByTestId('composer-readiness')).toBeNull();
  });
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
    expect(screen.getByTestId('composer-readiness-spinner')).toHaveAttribute(
      'data-for',
      'distributed'
    );
    expect(screen.queryByTestId('composer-readiness-mount')).toBeNull();

    mockExtMethod.mockResolvedValue({ status: DIST_READY });
    await mlxDistributedStatus();
    await waitFor(() => expect(screen.queryByTestId('composer-readiness')).toBeNull());

    mockExtMethod.mockRejectedValue(new Error('goosed gone'));
    await expect(mlxDistributedStatus()).rejects.toThrow('goosed gone');
    expect(await screen.findByTestId('composer-readiness-mount')).toBeInTheDocument();
  });

  it('another window’s run: named as owned there, read-only here, and its stop brings Mount back', async () => {
    const report = vi.fn();
    (window as unknown as { electron: unknown }).electron = { mlxDistributedReport: report };
    mockExtMethod.mockResolvedValue({ status: OTHER_WINDOW_LOADING });
    await mlxDistributedStatus();
    expect(report).not.toHaveBeenCalled();
    wrap('swarm');
    const strip = await screen.findByTestId('composer-readiness');
    expect(strip.textContent).toContain(
      'Distributed · 2 nodes · JACCL · Not answering · owned by another window — mihai-mlx'
    );
    expect(screen.getByTestId('composer-readiness-detail').textContent).toContain(
      'connection refused'
    );
    expect(screen.queryByTestId('composer-readiness-mount')).toBeNull();

    mockExtMethod.mockResolvedValue({ status: OTHER_WINDOW });
    await mlxDistributedStatus();
    await waitFor(() => expect(screen.queryByTestId('composer-readiness')).toBeNull());

    mockExtMethod.mockResolvedValue({ status: { ...OTHER_WINDOW, owner: undefined } });
    await mlxDistributedStatus();
    expect(report).toHaveBeenCalledTimes(1);
    expect(await screen.findByTestId('composer-readiness-mount')).toBeInTheDocument();
  });

  it('Open Engine goes to the Providers view’s engine', async () => {
    const user = userEvent.setup();
    wrap('swarm');
    await user.click(await screen.findByTestId('composer-readiness-open-engine'));
    expect(screen.getByTestId('providers-view')).toBeInTheDocument();
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { PlacementBadge, PlacementCard } from './PlacementCard';
import { PLAN_27B, PLAN_FLASH, NODES } from './placement.fixtures';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { PlacementPlan } from '../../acp/mlx-placement';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { NodesResponse } from '../../acp/leanzero-link';
import DISCOVERY from './mlxDistributedDiscovery.fixture.json';

const mockPlan = vi.fn();
const mockMeasure = vi.fn();
const mockRemoteStart = vi.fn();
const mockRemoteStop = vi.fn();
const mockRemoteStatus = vi.fn();
const mockDistributedStart = vi.fn();
const mockDistributedStop = vi.fn();
let remoteLatest: unknown = null;

vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlan(...a),
  mlxMeasureSpeed: (...a: unknown[]) => mockMeasure(...a),
}));
vi.mock('../../acp/mlx-remote-single', () => ({
  mlxRemoteSingleStart: (...a: unknown[]) => mockRemoteStart(...a),
  mlxRemoteSingleStop: (...a: unknown[]) => mockRemoteStop(...a),
  mlxRemoteSingleStatus: (...a: unknown[]) => mockRemoteStatus(...a),
  latestMlxRemoteSingleStatus: () => remoteLatest,
  subscribeMlxRemoteSingleStatus: () => () => undefined,
}));
const mockDiscover = vi.fn();
const mockProvision = vi.fn();
const mockDistributedStatus = vi.fn();
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedStart: (...a: unknown[]) => mockDistributedStart(...a),
  mlxDistributedStop: (...a: unknown[]) => mockDistributedStop(...a),
  mlxDistributedDiscover: (...a: unknown[]) => mockDiscover(...a),
  mlxDistributedProvision: (...a: unknown[]) => mockProvision(...a),
  mlxDistributedStatus: (...a: unknown[]) => mockDistributedStatus(...a),
}));
vi.mock('./LocalNetworkNotice', () => ({ touchLocalNetwork: vi.fn(async () => undefined) }));

// The Macs Run it names and copies between: the Link roster, each Mac's models and the links.
const mockNodes = vi.fn();
vi.mock('../../acp/leanzero-link', async (importActual) => ({
  ...(await importActual<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkStatus: vi.fn(async () => ({
    auth: { state: 'connected', email: 'm@x.co', meshIp: '100.64.0.4' },
    nodeCount: 2,
  })),
  leanzeroLinkNodes: (...a: unknown[]) => mockNodes(...a),
}));
const mockModelsList = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: vi.fn(async () => ({
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 90,
    totalMemoryGb: 128,
  })),
  mlxEngineModelsList: (...a: unknown[]) => mockModelsList(...a),
  mlxEngineDownload: vi.fn(),
  mlxEngineDownloadCancel: vi.fn(),
  mlxEngineDownloadPause: vi.fn(),
  mlxEngineDownloadProgress: vi.fn(async () => null),
  mlxEngineDownloadResume: vi.fn(),
  mlxEngineModelDelete: vi.fn(),
}));
const mockTargets = vi.fn();
const mockReplicate = vi.fn();
const mockReplicaProgress = vi.fn();
vi.mock('../../acp/mlx-replica', () => ({
  mlxEngineReplicaTargets: (...a: unknown[]) => mockTargets(...a),
  mlxEngineReplicate: (...a: unknown[]) => mockReplicate(...a),
  mlxEngineReplicaProgress: (...a: unknown[]) => mockReplicaProgress(...a),
  mlxEngineReplicaCancel: vi.fn(),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ leanzeroLink: true, mlxDistributed: true, mlxEngine: true }),
}));

const MODEL = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const GB = 1024 * 1024 * 1024;

function answer(plan: PlacementPlan) {
  return { plans: [plan], nodes: NODES, storeErrors: [], probeMs: 5404 };
}

const SELF = {
  node_id: 'mihai-macbook-2-aa',
  hostname: 'Mihai-Macbook-2.local',
  computer_name: 'Mihai Macbook',
  status: { type: 'Idle' as const },
  sessions_active: 0,
  updated_at: '2026-09-24T10:00:00Z',
};
const PEER = {
  node_id: 'wh',
  hostname: 'WorksMacStudio.lan',
  computer_name: 'Work’s Mac Studio',
  status: { type: 'Idle' as const },
  sessions_active: 0,
  updated_at: '2026-09-24T10:00:00Z',
  allows: { manage_models: true, answer_chat: true, run_split: true },
};
const ROSTER: NodesResponse = { self: SELF, peers: [PEER] };

const TB_LINK = {
  kind: 'thunderbolt' as const,
  local: { device: 'en3', kind: 'thunderbolt' as const, ipv4: '192.168.0.1', prefixLen: 30 },
  peer: { device: 'en3', kind: 'thunderbolt' as const, ipv4: '192.168.0.2', prefixLen: 30 },
};

/** The 27B plan as a Link roster sees it: the Studio is `link:wh`, its single engine startable. */
const PLAN_LINK: PlacementPlan = {
  ...PLAN_27B,
  candidates: (PLAN_27B.candidates ?? []).map((c) =>
    c.id === 'single:workhorse'
      ? {
          ...c,
          id: 'single:link:wh',
          key: { ...c.key, nodes: ['link:wh'] },
          action: { kind: 'remoteSingle' },
        }
      : c
  ),
  best: 'single:link:wh',
  bestAvailable: 'single:link:wh',
};

function renderCard(props: Partial<Parameters<typeof PlacementCard>[0]> = {}): {
  onMountHere: ReturnType<typeof vi.fn>;
  onStopHere: ReturnType<typeof vi.fn>;
} {
  const onMountHere = vi.fn();
  const onStopHere = vi.fn();
  render(
    <IntlTestWrapper>
      <PlacementCard
        modelId={MODEL}
        single={null}
        distributed={null}
        onMountHere={onMountHere}
        onStopHere={onStopHere}
        mountBusy={false}
        distributedCapability
        splitDetails={<p data-testid="split-details-body">set up · preflight · events</p>}
        {...props}
      />
    </IntlTestWrapper>
  );
  return { onMountHere, onStopHere };
}

beforeEach(() => {
  sessionStorage.clear();
  localStorage.clear();
  remoteLatest = null;
  mockRemoteStatus.mockResolvedValue({ state: 'off' });
  mockPlan.mockResolvedValue(answer(PLAN_27B));
  mockNodes.mockResolvedValue(ROSTER);
  mockModelsList.mockImplementation(async () => ({
    models: [{ id: MODEL, sizeBytes: 31 * GB, complete: true, missingFiles: 0 }],
    diskAvailableBytes: 100 * GB,
    diskTotalBytes: 900 * GB,
  }));
  mockTargets.mockImplementation(async (nodeId?: string) => ({
    meshConnected: true,
    targets: nodeId
      ? [{ nodeId: SELF.node_id, hostname: SELF.hostname, link: TB_LINK }]
      : [{ nodeId: 'wh', hostname: PEER.hostname, link: TB_LINK }],
  }));
  mockReplicate.mockResolvedValue({ link: TB_LINK, sourceUrl: 'http://192.168.0.1:1' });
  mockReplicaProgress.mockResolvedValue(null);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/**
 * The owner's walkthrough of 3.0.27: "Run across both Macs" for a model other than the saved split
 * said "open Details › Set up, pick this one, then Run". Run now switches the split itself.
 */
describe('Run across both Macs for a model the split is not set up with', () => {
  /** The saved split: the same two Macs, serving the Flash model, with the owner's own ports. */
  const SAVED = {
    ...DISCOVERY.chosen27b.config,
    modelId: 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
    port: 9191,
    watchdogWarnRatio: 0.07,
    nodes: DISCOVERY.chosen27b.config.nodes.map((n) => ({
      ...n,
      modelDir: `/models/rapid-mlx/Qwen3.8-Flash-Next-4bit`,
    })),
  };
  const STOPPED_FLASH = {
    mode: 'single',
    state: 'stopped',
    admissionOpen: true,
    nodes: [],
    config: SAVED,
  } as unknown as MlxDistributedStatus;
  const readyEnv = (d: typeof DISCOVERY.chosen27b) => ({
    ...d,
    nodes: d.nodes.map((n) => ({ ...n, env: { ...n.env, state: 'ready' } })),
  });

  it('Run detects the Macs for THIS model, keeps the saved setup, and starts — no Set up trip', async () => {
    mockDiscover.mockResolvedValue(readyEnv(DISCOVERY.chosen27b));
    mockDistributedStart.mockResolvedValue({ started: true });
    renderCard({ distributed: STOPPED_FLASH });
    const split = await screen.findByTestId('placement-way-split');
    expect(within(split).queryByText(/Set up, pick this one/)).toBeNull();
    await userEvent.click(within(split).getByTestId('placement-run-split'));
    await waitFor(() => expect(mockDistributedStart).toHaveBeenCalledTimes(1));
    expect(mockDiscover).toHaveBeenCalledWith(['workhorse'], MODEL);
    const config = mockDistributedStart.mock.calls[0][0];
    expect(config.modelId).toBe(MODEL);
    // The owner's setup survives the switch; the model's own folders come from the discovery.
    expect(config.port).toBe(9191);
    expect(config.watchdogWarnRatio).toBe(0.07);
    expect(config.nodes.map((n: { modelDir: string }) => n.modelDir)).toEqual(
      DISCOVERY.chosen27b.config.nodes.map((n) => n.modelDir)
    );
    expect(mockProvision).not.toHaveBeenCalled();
    expect(await screen.findByText('Starting — this card follows it.')).toBeInTheDocument();
  });

  it('a Mac without goose’s Python gets it built first, then the split starts', async () => {
    mockDiscover.mockResolvedValue(DISCOVERY.chosen27b);
    mockProvision.mockResolvedValue({ state: 'running', startedMs: 1, nodes: [] });
    mockDistributedStatus.mockResolvedValue({
      provision: { state: 'done', startedMs: 1, finishedMs: 2, nodes: [] },
    });
    mockDistributedStart.mockResolvedValue({ started: true });
    renderCard({ distributed: STOPPED_FLASH });
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-run-split'));
    await waitFor(() => expect(mockDistributedStart).toHaveBeenCalledTimes(1), { timeout: 5000 });
    expect(mockProvision).toHaveBeenCalledTimes(1);
    expect(mockProvision.mock.calls[0][0].modelId).toBe(MODEL);
  });

  it('a build that fails names the Mac and its words; nothing starts', async () => {
    mockDiscover.mockResolvedValue(DISCOVERY.chosen27b);
    mockProvision.mockResolvedValue({
      state: 'failed',
      startedMs: 1,
      nodes: [
        {
          rank: 1,
          name: 'Work’s Mac Studio',
          python: '/p',
          state: 'failed',
          detail: 'uv pip install mlx: no space left on device',
          lines: [],
          startedMs: 1,
        },
      ],
    });
    renderCard({ distributed: STOPPED_FLASH });
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    expect(
      await screen.findByText(
        'goose’s Python did not build on Work’s Mac Studio: uv pip install mlx: no space left on device'
      )
    ).toBeInTheDocument();
    expect(mockDistributedStart).not.toHaveBeenCalled();
  });

  it('the model missing on a Mac is said by name, and nothing starts', async () => {
    const missing = readyEnv(DISCOVERY.chosen27b);
    missing.models = missing.models.map((m) =>
      m.id === MODEL
        ? {
            ...m,
            onEveryNode: false,
            nodes: m.nodes.map((n) => (n.rank === 1 ? { ...n, state: 'absent' } : n)),
          }
        : m
    );
    mockDiscover.mockResolvedValue(missing);
    renderCard({ distributed: STOPPED_FLASH });
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    expect(
      await screen.findByText(
        'Qwen3.8-27B-Atlassian-Q8-mlx is not on Work’s Mac Studio yet — copy it there, then Run.'
      )
    ).toBeInTheDocument();
    expect(mockDistributedStart).not.toHaveBeenCalled();
  });

  it('“Run part of a split model” off on the Studio names the Mac and the switch — no probe', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.key.kind === 'tensor' ? { ...c, key: { ...c.key, nodes: ['local', 'link:wh'] } } : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    mockNodes.mockResolvedValue({
      self: SELF,
      peers: [{ ...PEER, allows: { manage_models: true, answer_chat: true, run_split: false } }],
    });
    renderCard({ distributed: STOPPED_FLASH });
    const split = await screen.findByTestId('placement-way-split');
    await waitFor(() => expect(mockModelsList).toHaveBeenCalledWith('wh'));
    await userEvent.click(within(split).getByTestId('placement-run-split'));
    expect(
      await screen.findByText(
        'Run part of a split model is off on Work’s Mac Studio — turn on “Let my other Macs use this Mac” there (Providers › My Macs)'
      )
    ).toBeInTheDocument();
    expect(mockDiscover).not.toHaveBeenCalled();
  });

  it('set up for THIS model already: Run starts the saved split as it is', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.key.kind === 'tensor' ? { ...c, action: { kind: 'startSplit', setupMatches: true } } : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    mockDistributedStart.mockResolvedValue({ started: true });
    renderCard({ distributed: STOPPED_FLASH });
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    await waitFor(() => expect(mockDistributedStart).toHaveBeenCalledWith(null));
    expect(mockDiscover).not.toHaveBeenCalled();
  });
});

describe('Run it on the real 27B plan', () => {
  it('three ways — this Mac, the Studio, both — each with its figure or the reason it lost', async () => {
    renderCard();
    const ways = await screen.findByTestId('placement-ways');
    const rows = within(ways).getAllByRole('listitem');
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
      'placement-way-local',
      'placement-way-peer',
      'placement-way-split',
    ]);

    const local = screen.getByTestId('placement-way-local');
    expect(within(local).getByText('Run on this Mac')).toBeInTheDocument();
    expect(
      within(local).getByText(/Does not fit: short 10\.8 GB on Mihai Macbook/)
    ).toBeInTheDocument();

    const peer = screen.getByTestId('placement-way-peer');
    expect(within(peer).getByText('Run on Work’s Mac Studio')).toBeInTheDocument();
    expect(within(peer).getByText('Best')).toBeInTheDocument();
    expect(within(peer).getByText('~21.9 tok/s writing')).toBeInTheDocument();
    expect(within(peer).getByText('20.8–23.0')).toBeInTheDocument();
    expect(within(peer).getByText('262,144 context')).toBeInTheDocument();

    const split = screen.getByTestId('placement-way-split');
    expect(within(split).getByText('Run across both Macs')).toBeInTheDocument();
    expect(within(split).getByText('Best you can start now')).toBeInTheDocument();
    expect(within(split).getByText('tensor split · JACCL')).toBeInTheDocument();
    expect(within(split).getByText('fits only at 72,704 context')).toBeInTheDocument();

    // The pipeline split goose cannot run for this model folds away with its reason.
    await userEvent.click(screen.getByText('1 other split'));
    expect(
      within(screen.getByTestId('placement-other-pipeline:jaccl:local+workhorse')).getByText(
        /^not supported yet: goose splits qwen3_5 tensor-parallel only$/
      )
    ).toBeInTheDocument();
    // No hardware lines under the card: the chips live on My Macs.
    expect(screen.queryByTestId('placement-nodes')).toBeNull();
    expect(mockPlan).toHaveBeenCalledWith('chat', MODEL);
  });

  it('switches the goal and plans again for it', async () => {
    renderCard();
    await screen.findByTestId('placement-ways');
    await userEvent.click(screen.getByRole('radio', { name: 'Long documents' }));
    await waitFor(() => expect(mockPlan).toHaveBeenLastCalledWith('longDocuments', MODEL));
  });

  it('the running way carries Stop and Measure speed; Measure records it', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      best: 'single:local',
      bestAvailable: 'single:local',
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.id === 'single:local'
          ? { ...c, outcome: { code: 'best' }, fit: { ...c.fit, status: 'fits', context: 262144 } }
          : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    mockMeasure.mockResolvedValue({
      records: [{ workload: 'chat', decodeTps: 21.5, prefillTps: 238 }],
    });
    const { onStopHere } = renderCard({
      single: { state: 'running', modelId: MODEL } as MlxEngineStatus,
    });
    const local = await screen.findByTestId('placement-way-local');
    expect(within(local).getByTestId('placement-live')).toHaveTextContent('Running');
    expect(within(local).queryByTestId('placement-run-local')).toBeNull();
    expect(within(local).getByText(/Running and not measured yet/)).toBeInTheDocument();
    await userEvent.click(within(local).getByTestId('placement-measure-local'));
    await waitFor(() => expect(mockMeasure).toHaveBeenCalledWith(MODEL, 'single:local', false));
    expect(
      await screen.findByText('Measured: 21.5 tok/s writing, 238 tok/s reading')
    ).toBeInTheDocument();
    await userEvent.click(within(local).getByTestId('placement-stop-local'));
    expect(onStopHere).toHaveBeenCalledTimes(1);
  });

  it('Run on the Studio starts its engine; a refusal reads in words, naming the Mac and the switch', async () => {
    mockPlan.mockResolvedValue(answer(PLAN_LINK));
    mockRemoteStart.mockResolvedValue({
      started: false,
      refusal: {
        code: 'chatServingDisabled',
        message:
          'chatServingDisabled: "Let my other Macs use this Mac › Answer chat" is off on WorksMacStudio.lan',
      },
      status: { state: 'off' },
    });
    renderCard();
    const peer = await screen.findByTestId('placement-way-peer');
    await userEvent.click(await within(peer).findByTestId('placement-run-peer'));
    expect(mockRemoteStart).toHaveBeenCalledWith('wh', MODEL);
    expect(
      await screen.findByText(
        'Answer chat is off on Work’s Mac Studio — turn on “Let my other Macs use this Mac” there (Providers › My Macs)'
      )
    ).toBeInTheDocument();
  });

  it('Run on this Mac mounts through the view', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      best: 'single:local',
      bestAvailable: 'single:local',
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.id === 'single:local'
          ? { ...c, outcome: { code: 'best' }, fit: { ...c.fit, status: 'fits' } }
          : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    const { onMountHere } = renderCard();
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    expect(onMountHere).toHaveBeenCalledTimes(1);
  });

  it('with no plan, every way can still be started — the failure is named, this Mac leads', async () => {
    mockPlan.mockRejectedValueOnce({ data: 'mlx_engine config is unreadable' });
    const { onMountHere } = renderCard();
    expect(await screen.findByText('mlx_engine config is unreadable')).toBeInTheDocument();
    expect(screen.getByText(/every way can still be started/)).toBeInTheDocument();
    const run = await screen.findByTestId('placement-run-local');
    await userEvent.click(run);
    expect(onMountHere).toHaveBeenCalledTimes(1);
    // The Studio lets this Mac load models: its way is offered too, named by the roster.
    expect(await screen.findByText('Run on Work’s Mac Studio')).toBeInTheDocument();
    expect(screen.getByText('Run across your Macs')).toBeInTheDocument();
  });

  it('the split’s Details is folded on first view, and the choice holds for the session', async () => {
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    expect(within(split).queryByTestId('split-details-body')).not.toBeVisible();
    await userEvent.click(within(split).getByText('Details'));
    expect(within(split).getByTestId('split-details-body')).toBeVisible();
    cleanup();
    renderCard();
    expect(
      within(await screen.findByTestId('placement-way-split')).getByTestId('split-details-body')
    ).toBeVisible();
  });

  it('the Studio lacks the model: Copy to it first over Thunderbolt, and the start goes on by itself', async () => {
    mockPlan.mockResolvedValue(answer(PLAN_LINK));
    localStorage.setItem('mlx-copy-rate:self>wh', String(1 * GB)); // a measured 1 GiB/s
    mockModelsList.mockImplementation(async (nodeId?: string) =>
      nodeId
        ? { models: [], diskAvailableBytes: 500 * GB, diskTotalBytes: 900 * GB }
        : {
            models: [{ id: MODEL, sizeBytes: 31 * GB, complete: true, missingFiles: 0 }],
            diskAvailableBytes: 100 * GB,
            diskTotalBytes: 900 * GB,
          }
    );
    mockRemoteStart.mockResolvedValue({ started: true, status: { state: 'mounting' } });
    renderCard();
    const peer = await screen.findByTestId('placement-way-peer');
    const copy = await within(peer).findByTestId('placement-copy-first-peer');
    expect(copy).toHaveTextContent('Copy to Work’s Mac Studio first (~1 min over Thunderbolt)');
    expect(within(peer).queryByTestId('placement-run-peer')).toBeNull();

    mockReplicaProgress.mockResolvedValue({
      state: 'copying',
      totalBytes: 31 * GB,
      copiedBytes: 13 * GB,
      filesTotal: 7,
      filesDone: 3,
      wireBytes: 13 * GB,
      wireMillis: 13000,
      elapsedMillis: 13000,
    });
    await userEvent.click(copy);
    expect(mockReplicate).toHaveBeenCalledWith(MODEL, 'wh', undefined);
    expect(
      await within(peer).findByText(
        'Copying to Work’s Mac Studio — 42% · it starts when the copy lands',
        undefined,
        { timeout: 3000 }
      )
    ).toBeInTheDocument();
    expect(mockRemoteStart).not.toHaveBeenCalled();

    mockReplicaProgress.mockResolvedValue({
      state: 'done',
      totalBytes: 31 * GB,
      copiedBytes: 31 * GB,
      filesTotal: 7,
      filesDone: 7,
      wireBytes: 31 * GB,
      wireMillis: 31000,
      elapsedMillis: 31000,
    });
    await waitFor(() => expect(mockRemoteStart).toHaveBeenCalledWith('wh', MODEL), {
      timeout: 3000,
    });
  });

  it('a way goose says does not fit offers no Run — only why', async () => {
    mockPlan.mockResolvedValue(answer(PLAN_FLASH));
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    expect(within(split).queryByTestId('placement-run-split')).toBeNull();
    expect(within(split).getByText(/Does not fit/)).toBeInTheDocument();
  });

  it('renders on Studio tokens only, no left rails', async () => {
    renderCard();
    const card = await screen.findByTestId('placement-card');
    await userEvent.click(screen.getByText('1 other split'));
    assertStudioClean(card);
    expect(allClasses(card).filter((c) => c === 'border-l' || /^border-l-\d/.test(c))).toEqual([]);
  });
});

describe('PlacementBadge', () => {
  it('says where a model fits, in solid tones', () => {
    render(
      <IntlTestWrapper>
        <PlacementBadge badge={{ kind: 'fitsPeer', name: 'Work’s Mac Studio' }} />
        <PlacementBadge badge={{ kind: 'tooBig', shortBytes: 14715588048 }} />
        <PlacementBadge badge={{ kind: 'fitsThisMac' }} />
        <PlacementBadge badge={{ kind: 'needsBothMacs' }} />
      </IntlTestWrapper>
    );
    expect(screen.getByText('Fits Work’s Mac Studio').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'accent'
    );
    expect(screen.getByText('Too big, short 13.7 GB').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'err'
    );
    expect(screen.getByText('Fits this Mac')).toBeInTheDocument();
    expect(screen.getByText('Needs both Macs')).toBeInTheDocument();
  });
});

describe('Run it follows the engine it started, in the engine-phase palette', () => {
  it('the split while it starts is amber, then green once it serves; Stop asks first', async () => {
    const starting = {
      mode: 'distributed',
      state: 'starting',
      modelId: MODEL,
      admissionOpen: true,
      nodes: [{ name: 'Mihai Macbook' }, { name: 'Work’s Mac Studio' }],
      events: [],
      restarts: 0,
    } as unknown as MlxDistributedStatus;
    const props = {
      modelId: MODEL,
      single: null,
      onMountHere: vi.fn(),
      onStopHere: vi.fn(),
      mountBusy: false,
      distributedCapability: true,
    };
    const { rerender } = render(
      <IntlTestWrapper>
        <PlacementCard {...props} distributed={starting} />
      </IntlTestWrapper>
    );
    const split = await screen.findByTestId('placement-way-split');
    const live = within(split).getByTestId('placement-live');
    expect(live).toHaveAttribute('data-phase', 'loading');
    expect(live).toHaveTextContent('Starting');
    rerender(
      <IntlTestWrapper>
        <PlacementCard {...props} distributed={{ ...starting, state: 'serving', inflight: 1 }} />
      </IntlTestWrapper>
    );
    expect(
      within(screen.getByTestId('placement-way-split')).getByTestId('placement-live')
    ).toHaveAttribute('data-phase', 'writing');
    // The Studio's single engine is not what runs: no live chip on it.
    expect(
      within(screen.getByTestId('placement-way-peer')).queryByTestId('placement-live')
    ).toBeNull();

    mockDistributedStop.mockResolvedValue({ stop: { verified: true, steps: [] } });
    await userEvent.click(
      within(screen.getByTestId('placement-way-split')).getByTestId('placement-stop-split')
    );
    expect(mockDistributedStop).not.toHaveBeenCalled();
    expect(
      screen.getByText(/Every part on Mihai Macbook, Work’s Mac Studio is stopped/)
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    await waitFor(() => expect(mockDistributedStop).toHaveBeenCalledTimes(1));
  });
});

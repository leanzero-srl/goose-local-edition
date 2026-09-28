import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import {
  PlacementBadge,
  PlacementCard,
  pickerBadgeOf,
  splitTradeOff,
  type PickerBadge,
} from './PlacementCard';
import { PLAN_27B, PLAN_FLASH, NODES } from './placement.fixtures';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { PlacementPlan } from '../../acp/mlx-placement';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { NodesResponse } from '../../acp/leanzero-link';
import DISCOVERY from './mlxDistributedDiscovery.fixture.json';
import { dismissPeerHeld, latestPeerHeld } from './routeSwitch';
import { factCheckSnapshot, liveSplitSnapshot } from '../../utils/mlxInFlight.fixtures';
import {
  publishListedNames,
  resetListedNamesForTests,
} from '../sessionActivity/sessionActivityStore';

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
  latestMlxRemoteSingleReadError: () => null,
  remoteRouteUp: (s: { state: string } | null) => s != null && s.state !== 'off',
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
const mockUnmount = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: (...a: unknown[]) => mockUnmount(...a),
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
  dismissPeerHeld();
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
    await waitFor(() => expect(mockDistributedStart).toHaveBeenCalledTimes(1));
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
        'Run part of a split model is off on Work’s Mac Studio — turn on “Let my other Macs use this Mac” there (LeanZero MLX › My Macs)'
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

  /**
   * Q-116, 3.0.46: the saved split's runner envs were built by an older goose. Run updates them
   * itself — the card says so by Mac while it runs — and then follows the start.
   */
  describe('the split’s runner from an older goose (Q-116)', () => {
    const SETUP_MATCHES: PlacementPlan = {
      ...PLAN_27B,
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.key.kind === 'tensor' ? { ...c, action: { kind: 'startSplit', setupMatches: true } } : c
      ),
    };
    const UPDATING = {
      ...STOPPED_FLASH,
      state: 'preflight',
      runnerUpdate: {
        state: 'running',
        startedMs: 1,
        nodes: [
          {
            rank: 0,
            name: 'Mihai Macbook',
            python:
              '/Users/mihaiperdum/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12/bin/python',
            state: 'running',
            step: 'install',
            detail: 'rapid-mlx @ git+https://github.com/leanzero-srl/Rapid-MLX@b7bd1afc2…',
            lines: ['GOOSE_PROV install rapid-mlx @ git+…', 'Resolved 31 packages in 1.2s'],
            startedMs: 1,
          },
          {
            rank: 1,
            name: 'Work’s Mac Studio',
            python:
              '/Users/workhorse/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12/bin/python',
            state: 'done',
            step: 'done',
            detail: 'installed',
            lines: ['GOOSE_PROV done installed'],
            startedMs: 1,
            finishedMs: 2,
          },
        ],
      },
    } as unknown as MlxDistributedStatus;

    it('Run updates the runner on both Macs, says so by Mac while it runs, then follows the start', async () => {
      mockPlan.mockResolvedValue(answer(SETUP_MATCHES));
      let finish: (value: unknown) => void = () => undefined;
      mockDistributedStart.mockReturnValue(
        new Promise((resolve) => {
          finish = resolve;
        })
      );
      renderCard({ distributed: UPDATING });
      const split = await screen.findByTestId('placement-way-split');
      // Nothing of the update shows before this card pressed Run.
      expect(screen.queryByTestId('placement-runner-update')).toBeNull();
      await userEvent.click(within(split).getByTestId('placement-run-split'));
      const notice = await screen.findByTestId('placement-runner-update');
      expect(
        within(notice).getByText(
          'Updating the split’s runner on Mihai Macbook and Work’s Mac Studio…'
        )
      ).toBeInTheDocument();
      const rows = within(notice).getAllByTestId('placement-runner-node');
      expect(rows.map((r) => r.getAttribute('data-state'))).toEqual(['running', 'done']);
      expect(within(rows[0]).getByText('installing')).toBeInTheDocument();
      expect(within(rows[0]).getByText('Resolved 31 packages in 1.2s')).toBeInTheDocument();
      expect(within(rows[1]).getByText('done')).toBeInTheDocument();
      // Plain words: no commit, no path.
      expect(notice.textContent).not.toMatch(/2f02ac645|b7bd1afc2|\.goose\/distributed/);
      expect(mockProvision).not.toHaveBeenCalled();

      await act(async () => finish({ started: true }));
      expect(await screen.findByText('Starting — this card follows it.')).toBeInTheDocument();
      expect(screen.queryByTestId('placement-runner-update')).toBeNull();
    });

    it('an update that fails names the Mac in plain words; the node’s output waits under Details', async () => {
      mockPlan.mockResolvedValue(answer(SETUP_MATCHES));
      mockDistributedStart.mockResolvedValue({
        started: false,
        refusal: {
          code: 'runnerUpdateFailed',
          message: 'Updating the split’s runner on Work’s Mac Studio failed',
          node: 'Work’s Mac Studio',
          detail:
            '/Users/workhorse/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12/bin/python (rapid-mlx-pipeline-qwen4-py3.12): uv pip install exited 1\nGOOSE_PROV fail uv pip install exited 1',
        },
      });
      const { container } = render(
        <IntlTestWrapper>
          <PlacementCard
            modelId={MODEL}
            single={null}
            distributed={STOPPED_FLASH}
            onMountHere={vi.fn()}
            onStopHere={vi.fn()}
            mountBusy={false}
            distributedCapability
          />
        </IntlTestWrapper>
      );
      await userEvent.click(
        within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
      );
      expect(
        await screen.findByText('Updating the split’s runner on Work’s Mac Studio failed')
      ).toBeInTheDocument();
      const details = screen.getByTestId('placement-notice-detail');
      expect(details).toHaveAttribute('data-state', 'closed');
      expect(within(details).getByText(/uv pip install exited 1/)).not.toBeVisible();
      await userEvent.click(within(details).getByRole('button', { name: 'Details' }));
      expect(within(details).getByText(/GOOSE_PROV fail uv pip install exited 1/)).toBeVisible();
      assertStudioClean(container);
    });
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
    // Q-174: plain words on the card; the runner and transport ride the chip's title.
    const kind = within(split).getByText('split over Thunderbolt');
    expect(kind).toHaveAttribute('title', 'tensor split · JACCL');
    expect(kind.parentElement?.textContent).not.toMatch(/JACCL|tensor/);
    expect(within(split).getByText('fits only at 72,704 context')).toBeInTheDocument();

    // Q-25 moved the pipeline split goose cannot run under the split's Details; Q-312: it is no
    // option at all, so Details carries no "1 other split … not supported yet" either.
    await userEvent.click(within(split).getByText('Details'));
    expect(screen.queryByTestId('placement-others')).toBeNull();
    expect(screen.queryByTestId('placement-other-pipeline:jaccl:local+workhorse')).toBeNull();
    expect(screen.queryByText(/not supported yet/)).toBeNull();
    // No hardware lines under the card: the chips live on My Macs.
    expect(screen.queryByTestId('placement-nodes')).toBeNull();
    expect(mockPlan).toHaveBeenCalledWith('chat', MODEL);
  });

  /**
   * Q-129: a single measured run was drawn as a range ("29.6–29.6 measured"). A measured figure draws
   * its runs through measuredFigure — the rule the Engine tile and the tray use: one run is a count,
   * never a range; several runs show their middle half; an estimate keeps its error range.
   */
  describe('the Studio way’s measured figure (Q-129)', () => {
    const withStudioDecode = (value: number, low: number, high: number, runs: number) => ({
      ...PLAN_27B,
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.key.kind === 'single' && c.key.nodes[0] !== 'local'
          ? {
              ...c,
              speed: {
                ...c.speed,
                decode: { estimate: { value, low, high }, measured: true, runs },
              },
            }
          : c
      ),
    });

    it('one run reads "measured · 1 run" beside its rate — never "29.6–29.6"', async () => {
      mockPlan.mockResolvedValue(answer(withStudioDecode(29.6066, 29.6066, 29.6066, 1)));
      renderCard();
      const peer = await screen.findByTestId('placement-way-peer');
      expect(within(peer).getByText('~29.6 tok/s writing')).toBeInTheDocument();
      expect(within(peer).getByText('measured · 1 run')).toBeInTheDocument();
      expect(within(peer).queryByText('29.6–29.6')).toBeNull();
      expect(within(peer).queryByTestId('placement-figure-range')).toBeNull();
    });

    it('the Studio’s 303 counted turns: the median, their middle half, and the count', async () => {
      mockPlan.mockResolvedValue(answer(withStudioDecode(26.96, 24.14, 30.74, 303)));
      renderCard();
      const peer = await screen.findByTestId('placement-way-peer');
      expect(within(peer).getByText('~27.0 tok/s writing')).toBeInTheDocument();
      expect(within(peer).getByText('24.1–30.7 middle half')).toBeInTheDocument();
      expect(within(peer).getByText('measured · 303 runs')).toBeInTheDocument();
    });

    it('an estimate keeps its error range and says estimated', async () => {
      renderCard();
      const peer = await screen.findByTestId('placement-way-peer');
      expect(within(peer).getByText('20.8–23.0')).toBeInTheDocument();
      expect(within(peer).getByText('estimated')).toBeInTheDocument();
    });
  });

  /**
   * Q-72: E2E #1 turn 0 took 20 min 20 s on the split — it writes ~14 tok/s against 22 on the Studio
   * alone, for ~1.2× faster prompt reading. For a model that fits one Mac, the split's row states the
   * trade-off in the plan's own figures, and goose's bare "Slower for this" is not repeated.
   */
  it('the split beside a Mac the model fits on says what it costs and buys, in the plan’s figures', async () => {
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    expect(
      within(split).getByText(
        'Work’s Mac Studio alone fits this model. Split across your Macs it writes ~13.9 tok/s against ~21.9 there, and reads prompts 1.2× faster (~416 vs ~335 tok/s) — worth it only when prompts are long and replies short.'
      )
    ).toBeInTheDocument();
    expect(within(split).queryByText(/Slower for this/)).toBeNull();
    // The Studio stays the recommendation for chat.
    expect(within(screen.getByTestId('placement-way-peer')).getByText('Best')).toBeInTheDocument();
    expect(within(split).queryByText('Best')).toBeNull();
  });

  /**
   * Long documents: goose ranks by a whole turn's expected time (planner.rs turn_figure), and Run it
   * shows goose's "Best" and that figure — never a reading rate the ranking no longer used.
   */
  it('Long documents: the headline is the whole-turn figure goose ranked by, and "Best" is goose’s', async () => {
    const turnOf = (id: string, value: number) => {
      const c = PLAN_27B.candidates!.find((x) => x.id === id)!;
      return {
        ...c,
        speed: {
          ...c.speed,
          turn: {
            estimate: { value, low: value * 0.9, high: value * 1.1 },
            measured: false,
            runs: 0,
          },
        },
      };
    };
    const long: PlacementPlan = {
      ...PLAN_27B,
      goal: 'longDocuments',
      candidates: PLAN_27B.candidates!.map((c) =>
        c.id === 'single:workhorse'
          ? turnOf(c.id, 304)
          : c.key.kind === 'tensor'
            ? turnOf(c.id, 347)
            : c
      ),
    };
    mockPlan.mockResolvedValue(answer(long));
    renderCard();
    await screen.findByTestId('placement-ways');
    await userEvent.click(screen.getByRole('radio', { name: 'Long documents' }));
    await waitFor(() => expect(mockPlan).toHaveBeenLastCalledWith('longDocuments', MODEL));
    const peer = await screen.findByTestId('placement-way-peer');
    expect(within(peer).getByText('Best')).toBeInTheDocument();
    expect(
      within(peer).getByText('~304 tok/s through a whole document turn, answer included')
    ).toBeInTheDocument();
    const row = screen.getByTestId('placement-way-split');
    expect(within(row).queryByText('Best')).toBeNull();
    expect(
      within(row).getByText('~347 tok/s through a whole document turn, answer included')
    ).toBeInTheDocument();
    expect(within(row).getByText(/reads prompts 1\.2× faster/)).toBeInTheDocument();
  });

  it('a model no single Mac fits: the split has no trade-off to state', () => {
    const split = PLAN_27B.candidates!.find((c) => c.key.kind === 'tensor')!;
    const tooBig: PlacementPlan = {
      ...PLAN_27B,
      best: split.id,
      candidates: PLAN_27B.candidates!.map((c) =>
        c.key.kind === 'single' ? { ...c, fit: { ...c.fit, status: 'short' as const } } : c
      ),
    };
    expect(splitTradeOff(tooBig, split)).toBeNull();
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

  /**
   * Q-26: the Studio's "Running" chip was grey (the idle tone) beside a green Writing tile — the chip
   * was coloured with no activity at all. It takes the tile's own read when that read is its engine's.
   */
  it('Q-26: the running way’s chip is the tile’s colour — green while its engine writes', async () => {
    mockPlan.mockResolvedValue(answer(PLAN_LINK));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    renderCard({ liveActivity: { engine: 'remote', activity: 'generating' } });
    const peer = await screen.findByTestId('placement-way-peer');
    const chip = within(peer).getByTestId('placement-live');
    expect(chip).toHaveTextContent('Running');
    expect(chip).toHaveAttribute('data-phase', 'writing');
  });

  it('Q-26: a read of another engine never colours this way; with no read it is the idle grey', async () => {
    mockPlan.mockResolvedValue(answer(PLAN_LINK));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    renderCard({ liveActivity: { engine: 'single', activity: 'generating' } });
    const peer = await screen.findByTestId('placement-way-peer');
    expect(within(peer).getByTestId('placement-live')).toHaveAttribute('data-phase', 'idle');
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
        'Answer chat is off on Work’s Mac Studio — turn on “Let my other Macs use this Mac” there (LeanZero MLX › My Macs)'
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

  /** The 27B on the Studio as a live remote single, the plan crediting its memory to the rest. */
  const localFits: PlacementPlan = {
    ...PLAN_LINK,
    candidates: (PLAN_LINK.candidates ?? []).map((c) =>
      c.id === 'single:local' ? { ...c, fit: { ...c.fit, status: 'fits' } } : c
    ),
  };

  it('Run is a switch: Run on this Mac stops the Studio’s copy first, then mounts here', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    const order: string[] = [];
    mockRemoteStop.mockImplementation(async () => {
      order.push('stop studio');
      return { unmounted: true, unmountError: null, status: { state: 'off' } };
    });
    const { onMountHere } = renderCard();
    onMountHere.mockImplementation(() => order.push('mount here'));
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    await waitFor(() => expect(order).toEqual(['stop studio', 'mount here']));
    expect(mockRemoteStop).toHaveBeenCalledWith(false);
  });

  it('a switch off a Studio that is NOT answering drops the route and mounts here — no "switch failed", no wait on the Studio', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'reconnecting', peer: 'wh', modelId: MODEL };
    const order: string[] = [];
    mockRemoteStop.mockImplementation(async (keepMounted: boolean) => {
      order.push(`drop route (keepMounted ${keepMounted})`);
      remoteLatest = null;
      return { unmounted: false, unmountError: null, status: { state: 'off' } };
    });
    mockUnmount.mockReturnValue(new Promise(() => undefined));
    const { onMountHere } = renderCard();
    onMountHere.mockImplementation(() => order.push('mount here'));
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    await waitFor(() => expect(order).toEqual(['drop route (keepMounted true)', 'mount here']));
    expect(mockUnmount).toHaveBeenCalledWith('wh');
    expect(screen.queryByText(/Nothing started/)).toBeNull();
    // The quiet line is the Engine view's (PeerHeldLine); the card publishes the fact.
    expect(latestPeerHeld()).toMatchObject({ phase: 'asking', peerNodeId: 'wh' });
  });

  it('Stop on a route whose Mac is NOT answering drops it here at once — no error, the button free again', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'reconnecting', peer: 'wh', modelId: MODEL };
    mockRemoteStop.mockResolvedValue({
      unmounted: false,
      unmountError: null,
      status: { state: 'off' },
    });
    mockUnmount.mockReturnValue(new Promise(() => undefined));
    renderCard();
    const stop = await screen.findByTestId('placement-stop-peer');
    await userEvent.click(stop);
    await waitFor(() => expect(mockRemoteStop).toHaveBeenCalledWith(true));
    await waitFor(() => expect(mockUnmount).toHaveBeenCalledWith('wh'));
    await waitFor(() => expect(stop).toBeEnabled());
    expect(screen.queryByText(/Could not|failed/i)).toBeNull();
    expect(latestPeerHeld()).toMatchObject({ phase: 'asking', peerNodeId: 'wh' });
  });

  it('a reachable Studio that keeps its model does not block the switch: the kept model is a quiet line', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    mockRemoteStop.mockImplementation(async () => {
      remoteLatest = null;
      return {
        unmounted: false,
        unmountError: "wh's engine was left mounted: peer refused",
        status: { state: 'off' },
      };
    });
    const { onMountHere } = renderCard();
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    await waitFor(() => expect(onMountHere).toHaveBeenCalledTimes(1));
    expect(mockRemoteStop).toHaveBeenCalledWith(false);
    await waitFor(() =>
      expect(latestPeerHeld()).toMatchObject({ phase: 'held', peerNodeId: 'wh' })
    );
  });

  it('the "Starting" notice ends once the way it started serves (Q-36)', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    mockRemoteStop.mockResolvedValue({
      unmounted: true,
      unmountError: null,
      status: { state: 'off' },
    });
    let setSingle: (s: MlxEngineStatus) => void = () => undefined;
    function Harness() {
      const [single, set] = useState<MlxEngineStatus | null>(null);
      setSingle = set;
      return (
        <PlacementCard
          modelId={MODEL}
          single={single}
          distributed={null}
          onMountHere={() => undefined}
          onStopHere={() => undefined}
          mountBusy={false}
          distributedCapability
        />
      );
    }
    render(
      <IntlTestWrapper>
        <Harness />
      </IntlTestWrapper>
    );
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    expect(await screen.findByText('Starting — this card follows it.')).toBeInTheDocument();
    remoteLatest = null;
    act(() => setSingle({ state: 'mounting', modelId: MODEL } as MlxEngineStatus));
    expect(screen.getByText('Starting — this card follows it.')).toBeInTheDocument();
    act(() => setSingle({ state: 'running', modelId: MODEL } as MlxEngineStatus));
    await waitFor(() => expect(screen.queryByText('Starting — this card follows it.')).toBeNull());
  });

  it('Run on the Studio while this Mac runs the model unmounts here first, then starts there', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    const order: string[] = [];
    mockUnmount.mockImplementation(async () => void order.push('unmount here'));
    mockRemoteStart.mockImplementation(async () => {
      order.push('start studio');
      return { started: true, status: { state: 'mounting' } };
    });
    renderCard({ single: { state: 'running', modelId: MODEL } as MlxEngineStatus });
    const peer = await screen.findByTestId('placement-way-peer');
    await userEvent.click(await within(peer).findByTestId('placement-run-peer'));
    await waitFor(() => expect(order).toEqual(['unmount here', 'start studio']));
  });

  /** PLAN_LINK with the split set up for this model: Run on it starts the saved split as it is. */
  const splitReady: PlacementPlan = {
    ...PLAN_LINK,
    candidates: (PLAN_LINK.candidates ?? []).map((c) =>
      c.key.kind === 'tensor' ? { ...c, action: { kind: 'startSplit', setupMatches: true } } : c
    ),
  };

  it('Q-112: a switch to the split waits for the Studio to let go of the route’s LOADING engine, then starts the split', async () => {
    mockPlan.mockResolvedValue(answer(splitReady));
    // 3.0.44: the route restoring after a relaunch — the Studio loading the 27B, the fabric not
    // yet answering for it.
    remoteLatest = { state: 'reconnecting', peer: 'wh', modelId: MODEL };
    const order: string[] = [];
    mockRemoteStop.mockImplementation(async (keepMounted: boolean) => {
      order.push(`drop route (keepMounted ${keepMounted})`);
      remoteLatest = null;
      return { unmounted: false, unmountError: null, status: { state: 'off' } };
    });
    let studioLetGo: () => void = () => undefined;
    mockUnmount.mockImplementation(
      (node: string) =>
        new Promise<void>((resolve) => {
          order.push(`unmount on ${node}`);
          studioLetGo = () => {
            order.push('studio let go');
            resolve();
          };
        })
    );
    mockDistributedStart.mockImplementation(async () => {
      order.push('start split');
      return { started: true };
    });
    renderCard();
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    await waitFor(() => expect(order).toEqual(['drop route (keepMounted true)', 'unmount on wh']));
    expect(mockDistributedStart).not.toHaveBeenCalled();
    act(() => studioLetGo());
    await waitFor(() =>
      expect(order).toEqual([
        'drop route (keepMounted true)',
        'unmount on wh',
        'studio let go',
        'start split',
      ])
    );
    expect(mockDistributedStart).toHaveBeenCalledWith(null);
    expect(await screen.findByText('Starting — this card follows it.')).toBeInTheDocument();
  });

  it('a switch to the split off an answering Studio starts it after the route’s stop answered', async () => {
    mockPlan.mockResolvedValue(answer(splitReady));
    remoteLatest = { state: 'mounting', peer: 'wh', modelId: MODEL };
    const order: string[] = [];
    mockRemoteStop.mockImplementation(async (keepMounted: boolean) => {
      order.push(`stop route (keepMounted ${keepMounted})`);
      remoteLatest = null;
      return { unmounted: true, unmountError: null, status: { state: 'off' } };
    });
    mockDistributedStart.mockImplementation(async () => {
      order.push('start split');
      return { started: true };
    });
    renderCard();
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    await waitFor(() => expect(order).toEqual(['stop route (keepMounted false)', 'start split']));
    expect(mockUnmount).not.toHaveBeenCalled();
  });

  it('a route that cannot be withdrawn (another window owns it) starts nothing, and says why', async () => {
    mockPlan.mockResolvedValue(answer(localFits));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    mockRemoteStop.mockRejectedValue(
      new Error('remoteSingleActive: another goose window on this Mac owns the route')
    );
    const { onMountHere } = renderCard();
    await userEvent.click(await screen.findByTestId('placement-run-local'));
    expect(
      await screen.findByText(
        /Nothing started: Qwen3.8-27B-Atlassian-Q8-mlx could not be stopped .*remoteSingleActive/
      )
    ).toBeInTheDocument();
    expect(onMountHere).not.toHaveBeenCalled();
  });

  it('plans again when what serves changes — a note from the loading moment does not outlive it', async () => {
    let setSingle: (s: MlxEngineStatus) => void = () => undefined;
    function Harness() {
      const [single, set] = useState<MlxEngineStatus>({
        state: 'mounting',
        modelId: MODEL,
      } as MlxEngineStatus);
      setSingle = set;
      return (
        <PlacementCard
          modelId={MODEL}
          single={single}
          distributed={null}
          onMountHere={() => undefined}
          onStopHere={() => undefined}
          mountBusy={false}
          distributedCapability
        />
      );
    }
    render(
      <IntlTestWrapper>
        <Harness />
      </IntlTestWrapper>
    );
    await waitFor(() => expect(mockPlan).toHaveBeenCalledTimes(1));
    act(() => setSingle({ state: 'running', modelId: MODEL } as MlxEngineStatus));
    await waitFor(() => expect(mockPlan).toHaveBeenCalledTimes(2));
    act(() => setSingle({ state: 'running', modelId: MODEL } as MlxEngineStatus));
    await new Promise((r) => setTimeout(r, 20));
    expect(mockPlan).toHaveBeenCalledTimes(2);
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
        'Copying to Work’s Mac Studio — 42% · it starts when the copy lands'
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
    await waitFor(() => expect(mockRemoteStart).toHaveBeenCalledWith('wh', MODEL));
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
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByText('Details')
    );
    assertStudioClean(card);
    expect(allClasses(card).filter((c) => c === 'border-l' || /^border-l-\d/.test(c))).toEqual([]);
  });
});

describe('PlacementBadge', () => {
  const only = (badge: PickerBadge['badge']): PickerBadge => ({
    badge,
    fitsOn: [],
    macs: 0,
    afterStopping: [],
  });

  it('says where a model fits, in solid tones', () => {
    render(
      <IntlTestWrapper>
        <PlacementBadge badge={only({ kind: 'fitsPeer', name: 'Work’s Mac Studio' })} />
        <PlacementBadge badge={only({ kind: 'tooBig', shortBytes: 14715588048 })} />
        <PlacementBadge badge={only({ kind: 'fitsThisMac' })} />
        <PlacementBadge badge={only({ kind: 'needsBothMacs' })} />
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

  /**
   * Q-42: under "Memory on Work's Mac Studio · 37.0 GB available of 96.0 GB" the picker said
   * "31 GB · Fits this Mac" — "this Mac" was the MacBook. The badge names the Mac(s) from the plan's
   * own single candidates.
   */
  it('names the Mac a model fits on, never "this Mac" beside another Mac’s memory', () => {
    const fits = (id: string, status: 'fits' | 'short') => {
      const c = PLAN_27B.candidates!.find((x) => x.id === id)!;
      return { ...c, fit: { ...c.fit, status } };
    };
    const plan = (local: 'fits' | 'short', studio: 'fits' | 'short'): PlacementPlan => ({
      ...PLAN_27B,
      badge: local === 'fits' ? { kind: 'fitsThisMac' } : { kind: 'fitsPeer', name: 'x' },
      candidates: [
        fits('single:local', local),
        fits('single:workhorse', studio),
        ...PLAN_27B.candidates!.filter((c) => c.key.kind !== 'single'),
      ],
    });
    const hereOnly = pickerBadgeOf(plan('fits', 'short'))!;
    const both = pickerBadgeOf(plan('fits', 'fits'))!;
    expect(hereOnly.fitsOn).toEqual(['Mihai Macbook']);
    expect(both).toMatchObject({ fitsOn: ['Mihai Macbook', 'Work’s Mac Studio'], macs: 2 });
    render(
      <IntlTestWrapper>
        <PlacementBadge badge={hereOnly} />
        <PlacementBadge badge={both} />
      </IntlTestWrapper>
    );
    expect(screen.getByText('Fits Mihai Macbook').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'ok'
    );
    expect(screen.getByText('Fits both Macs')).toBeInTheDocument();
    expect(screen.queryByText('Fits this Mac')).toBeNull();
  });

  /**
   * Q-120 (3.0.47): Flash read "Too big, short 1.6 GB" while the 27B held ~30 GB on the Studio
   * that stopping it frees. goose now counts that memory and names the model the fit waits on.
   */
  it('a fit that holds once the serving model stops says so — never "too big"', () => {
    const picker = pickerBadgeOf({
      ...PLAN_FLASH,
      badge: { kind: 'needsBothMacs' },
      badgeAfterStopping: [MODEL],
    })!;
    expect(picker.afterStopping).toEqual([MODEL]);
    render(
      <IntlTestWrapper>
        <PlacementBadge badge={picker} />
      </IntlTestWrapper>
    );
    const chip = screen.getByText('Needs both Macs · fits once Qwen3.8-27B-Atlassian-Q8-mlx stops');
    expect(chip.closest('[data-tone]')).toHaveAttribute('data-tone', 'warn');
    expect(screen.queryByText(/Too big/)).toBeNull();
  });

  it('a plan with no badge has no picker badge', () => {
    expect(pickerBadgeOf({ ...PLAN_27B, badge: undefined })).toBeNull();
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
    // No preflight says how its window was sized: no claim about it.
    expect(screen.queryByTestId('placement-split-context')).toBeNull();
    // Q-71: a window the start derived from free memory is fixed for the run; the row says so.
    rerender(
      <IntlTestWrapper>
        <PlacementCard
          {...props}
          distributed={
            {
              ...starting,
              state: 'serving',
              inflight: 1,
              contextLimit: 141568,
              lastPreflight: { contextLimit: 141568, contextSource: 'derived' },
            } as unknown as MlxDistributedStatus
          }
        />
      </IntlTestWrapper>
    );
    expect(screen.getByTestId('placement-split-context')).toHaveTextContent(
      'Its 141,568 context was sized from the memory free when it started, and stays that size while it runs — restart it with more memory free to grow it.'
    );
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
      await screen.findByText(/Every part on Mihai Macbook, Work’s Mac Studio is stopped/)
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Stop the split' }));
    await waitFor(() => expect(mockDistributedStop).toHaveBeenCalledTimes(1));
  });
});

/**
 * Q-119 (3.0.47, 2026-09-26): Flash picked while Work's Mac Studio served the 27B over Link, and
 * Run it still showed the 27B's cards. Run it plans the PICKED model; each way that can start says
 * what Run stops first, and Run does exactly that before it starts.
 */
describe('Run it for the picked model while another model is served', () => {
  const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
  const SPLIT_ID = 'pipeline:jaccl:local+workhorse';
  /** Flash as goose plans it with the 27B's ~30 GB on the Studio counted as free for the switch. */
  const FLASH_SWITCH: PlacementPlan = {
    ...PLAN_FLASH,
    best: SPLIT_ID,
    bestAvailable: SPLIT_ID,
    badge: { kind: 'needsBothMacs' },
    badgeAfterStopping: [MODEL],
    candidates: (PLAN_FLASH.candidates ?? []).map((c) =>
      c.id === SPLIT_ID
        ? {
            ...c,
            fit: {
              ...c.fit,
              status: 'fits',
              shortBytes: undefined,
              shortNode: undefined,
              context: 65_536,
              afterStopping: [MODEL],
            },
            outcome: { code: 'best' },
          }
        : c.id === 'single:workhorse'
          ? { ...c, id: 'single:link:wh', key: { ...c.key, nodes: ['link:wh'] } }
          : c
    ),
  };

  it('the cards are Flash’s: the Studio serving the 27B is not "Running" here, and the split says it stops the 27B first', async () => {
    mockPlan.mockResolvedValue(answer(FLASH_SWITCH));
    remoteLatest = {
      state: 'ready',
      peer: 'wh',
      modelId: MODEL,
      peerComputerName: 'Work’s Mac Studio',
    };
    renderCard({ modelId: FLASH });
    const split = await screen.findByTestId('placement-way-split');
    await waitFor(() => expect(mockPlan).toHaveBeenCalledWith('chat', FLASH));
    expect(screen.queryByTestId('placement-live')).toBeNull();
    expect(screen.queryByTestId('placement-stop-peer')).toBeNull();
    expect(
      await within(split).findByText(
        'Fits once Qwen3.8-27B-Atlassian-Q8-mlx stops — Run stops it on Work’s Mac Studio first.'
      )
    ).toBeInTheDocument();
    // The ways that cannot hold Flash offer no Run, so they carry no "stops first" line.
    expect(screen.getAllByTestId(/placement-stops-first-/)).toHaveLength(1);
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
  });

  it('Run on the split stops the 27B’s route first, then starts the split for Flash', async () => {
    mockPlan.mockResolvedValue(answer(FLASH_SWITCH));
    remoteLatest = { state: 'ready', peer: 'wh', modelId: MODEL };
    const order: string[] = [];
    mockRemoteStop.mockImplementation(async (keepMounted: boolean) => {
      order.push(`stop 27B route (keepMounted ${keepMounted})`);
      remoteLatest = null;
      return { unmounted: true, unmountError: null, status: { state: 'off' } };
    });
    mockDistributedStart.mockImplementation(async () => {
      order.push('start split');
      return { started: true };
    });
    renderCard({ modelId: FLASH, distributed: null });
    await userEvent.click(
      within(await screen.findByTestId('placement-way-split')).getByTestId('placement-run-split')
    );
    await waitFor(() =>
      expect(order).toEqual(['stop 27B route (keepMounted false)', 'start split'])
    );
    expect(mockDistributedStart).toHaveBeenCalledWith(null);
  });

  it('Run on this Mac for the 27B while this Mac serves Flash: says so, unmounts Flash, then mounts', async () => {
    const localFits: PlacementPlan = {
      ...PLAN_LINK,
      candidates: (PLAN_LINK.candidates ?? []).map((c) =>
        c.id === 'single:local' ? { ...c, fit: { ...c.fit, status: 'fits' } } : c
      ),
    };
    mockPlan.mockResolvedValue(answer(localFits));
    const order: string[] = [];
    mockUnmount.mockImplementation(async () => {
      order.push('unmount Flash here');
    });
    const { onMountHere } = renderCard({
      single: {
        state: 'running',
        modelId: FLASH,
        restartRequired: false,
        availableMemoryGb: 20,
        totalMemoryGb: 128,
      } as MlxEngineStatus,
    });
    onMountHere.mockImplementation(() => order.push('mount 27B here'));
    const local = await screen.findByTestId('placement-way-local');
    expect(
      await within(local).findByText('Run stops Qwen3.8-Flash-Next-4bit on this Mac first.')
    ).toBeInTheDocument();
    expect(within(local).queryByTestId('placement-live')).toBeNull();
    await userEvent.click(within(local).getByTestId('placement-run-local'));
    await waitFor(() => expect(order).toEqual(['unmount Flash here', 'mount 27B here']));
  });
});

/**
 * Q-148, the live round of 2026-09-26: the split wrote a 39-minute answer while Run it offered
 * "Run on Work's Mac Studio · Best" — one click that stopped the split first, with no question —
 * and "Best" fit only at 45,083 context while the answer being written held ~64k.
 */
describe('Q-148: a way that would cut the answer being written asks first, and is never "Best" when too small', () => {
  const bridge = window.electron as unknown as { mlxEngineActivity?: () => Promise<unknown> };
  const SPLIT_SERVING = {
    mode: 'distributed',
    state: 'serving',
    modelId: MODEL,
    admissionOpen: true,
    inflight: 1,
    nodes: [{ name: 'Mihai Macbook' }, { name: 'Work’s Mac Studio' }],
    events: [],
    restarts: 0,
  } as unknown as MlxDistributedStatus;
  /** The Studio fits the 27B alone only at 45,083 context — goose's "Best" for chat. */
  const STUDIO_SMALL: PlacementPlan = {
    ...PLAN_LINK,
    candidates: (PLAN_LINK.candidates ?? []).map((c) =>
      c.id === 'single:link:wh'
        ? { ...c, fit: { ...c.fit, status: 'smallerContext', context: 45083 } }
        : c
    ),
  };
  afterEach(() => {
    delete bridge.mlxEngineActivity;
  });

  it('while the split writes: the Studio is not "Best", says why, says what Run cuts, and Run asks before it stops anything', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot());
    mockPlan.mockResolvedValue(answer(STUDIO_SMALL));
    mockDistributedStop.mockResolvedValue({
      status: { mode: 'single', state: 'stopped', nodes: [] },
      stop: { verified: true, steps: [] },
    });
    mockRemoteStart.mockResolvedValue({ started: true });
    renderCard({ distributed: SPLIT_SERVING });
    const peer = await screen.findByTestId('placement-way-peer');
    expect(await within(peer).findByTestId('placement-too-small-peer')).toHaveTextContent(
      'Its 45,083 context is under the 64,224 tokens the conversation being answered holds now.'
    );
    expect(within(peer).queryByText('Best')).toBeNull();
    expect(within(peer).getByTestId('placement-cuts-live-peer')).toHaveTextContent(
      'Run cuts the answer being written in “Jira Migration Kickoff Notes” — 39m 15s in, 24,228 tokens written.'
    );

    await userEvent.click(within(peer).getByTestId('placement-run-peer'));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Cut the answer being written?')).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        'Run on Work’s Mac Studio cuts the answer being written in “Jira Migration Kickoff Notes” — 39m 15s in, 24,228 tokens written.'
      )
    ).toBeInTheDocument();
    expect(mockDistributedStop).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Keep it writing' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mockDistributedStop).not.toHaveBeenCalled();
    expect(mockRemoteStart).not.toHaveBeenCalled();

    await userEvent.click(within(peer).getByTestId('placement-run-peer'));
    await userEvent.click(
      within(await screen.findByRole('dialog')).getByRole('button', {
        name: 'Run on Work’s Mac Studio',
      })
    );
    await waitFor(() => expect(mockRemoteStart).toHaveBeenCalledTimes(1));
    expect(mockDistributedStop).toHaveBeenCalledTimes(1);
  });

  it('Q-185, E2E #3i: goose’s fact check after the reply is named as that, with the chat’s listed name', async () => {
    publishListedNames([
      {
        id: '20260927_5',
        base: 'Jira Migration Kickoff Notes',
        label: 'Jira Migration Kickoff Notes · 5',
      },
    ]);
    bridge.mlxEngineActivity = vi.fn(async () => factCheckSnapshot());
    mockPlan.mockResolvedValue(answer(STUDIO_SMALL));
    renderCard({ distributed: SPLIT_SERVING });
    const peer = await screen.findByTestId('placement-way-peer');
    const said =
      'Run cuts goose’s background work for “Jira Migration Kickoff Notes · 5”: Checking the reply — 3s in, still reading its 1,094-token prompt.';
    expect(await within(peer).findByTestId('placement-cuts-live-peer')).toHaveTextContent(said);
    expect(within(peer).getByTestId('placement-cuts-live-peer')).not.toHaveTextContent(
      'the answer being written'
    );

    await userEvent.click(within(peer).getByTestId('placement-run-peer'));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Cut goose’s background work?')).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        'Run on Work’s Mac Studio cuts goose’s background work for “Jira Migration Kickoff Notes · 5”: Checking the reply — 3s in, still reading its 1,094-token prompt.'
      )
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Let it finish' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mockDistributedStop).not.toHaveBeenCalled();
    resetListedNamesForTests();
  });

  it('the split’s own Stop names the cut too', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot());
    mockDistributedStop.mockResolvedValue({ stop: { verified: true, steps: [] } });
    renderCard({ distributed: SPLIT_SERVING });
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-stop-split'));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(
        'Stop the split cuts the answer being written in “Jira Migration Kickoff Notes” — 39m 15s in, 24,228 tokens written.'
      )
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Stop the split' }));
    await waitFor(() => expect(mockDistributedStop).toHaveBeenCalledTimes(1));
  });

  /**
   * Q-28 (3.0.30, R5 r5-0-start.png): with the split up, "Run on this Mac" offered no Run — only
   * "The split owns this Mac — stop it to run a model here alone" — while Run was a switch on every
   * other way. It is one here too: it says it stops the split, stops it, then mounts here.
   */
  it('Q-28: while the split runs, Run on this Mac stops the split first, then mounts here', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot([]));
    mockDistributedStop.mockResolvedValue({
      status: { mode: 'single', state: 'stopped', nodes: [] },
      stop: { verified: true, steps: [] },
    });
    // goose credits the split's memory back to this Mac: the 27B fits here once the split stops.
    mockPlan.mockResolvedValue(
      answer({
        ...PLAN_LINK,
        candidates: (PLAN_LINK.candidates ?? []).map((c) =>
          c.id === 'single:local'
            ? {
                ...c,
                fit: {
                  ...c.fit,
                  status: 'fits',
                  shortBytes: undefined,
                  shortNode: undefined,
                  afterStopping: [MODEL],
                },
                outcome: { code: 'best' },
              }
            : c
        ),
      })
    );
    const { onMountHere } = renderCard({ distributed: SPLIT_SERVING });
    const local = await screen.findByTestId('placement-way-local');
    expect(await within(local).findByTestId('placement-stops-first-local')).toHaveTextContent(
      'Fits once Qwen3.8-27B-Atlassian-Q8-mlx stops — Run stops it on Mihai Macbook and Work’s Mac Studio first.'
    );
    expect(within(local).queryByText(/The split owns this Mac/)).toBeNull();
    const run = within(local).getByTestId('placement-run-local');
    expect(run).toBeEnabled();
    await userEvent.click(run);
    await waitFor(() => expect(onMountHere).toHaveBeenCalledTimes(1));
    expect(mockDistributedStop).toHaveBeenCalledTimes(1);
  });

  it('with nothing in flight the switch goes on at once, and goose’s "Best" stands', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot([]));
    mockPlan.mockResolvedValue(answer(STUDIO_SMALL));
    mockDistributedStop.mockResolvedValue({
      status: { mode: 'single', state: 'stopped', nodes: [] },
      stop: { verified: true, steps: [] },
    });
    mockRemoteStart.mockResolvedValue({ started: true });
    renderCard({ distributed: SPLIT_SERVING });
    const peer = await screen.findByTestId('placement-way-peer');
    expect(await within(peer).findByText('Best')).toBeInTheDocument();
    expect(within(peer).queryByTestId('placement-too-small-peer')).toBeNull();
    expect(within(peer).queryByTestId('placement-cuts-live-peer')).toBeNull();
    await userEvent.click(within(peer).getByTestId('placement-run-peer'));
    await waitFor(() => expect(mockRemoteStart).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

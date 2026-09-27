import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, renderHook, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import { PlacementCard } from './PlacementCard';
import { usePlacementPlans } from './usePlacementPlans';
import { PLAN_27B, NODES } from './placement.fixtures';
import type { PlacementPlan } from '../../acp/mlx-placement';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { NodesRead } from '../../acp/nodes';
import type { NodesConfig } from '../nodes/model';

/**
 * Run it's "Save as node" (DESIGN-NODES-AND-STRATEGIES.md §8.6) and the extracted plans hook (D10).
 * PlacementCard.test.tsx stays the card's own suite, unchanged; this one covers what S2 added.
 */

const mockPlan = vi.fn();
vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlan(...a),
  mlxMeasureSpeed: vi.fn(),
}));
vi.mock('../../acp/mlx-remote-single', () => ({
  mlxRemoteSingleStart: vi.fn(),
  mlxRemoteSingleStop: vi.fn(),
  mlxRemoteSingleStatus: vi.fn(async () => ({ state: 'off' })),
  latestMlxRemoteSingleStatus: () => null,
  latestMlxRemoteSingleReadError: () => null,
  remoteRouteUp: () => false,
  subscribeMlxRemoteSingleStatus: () => () => undefined,
}));
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedStart: vi.fn(),
  mlxDistributedStop: vi.fn(),
  mlxDistributedDiscover: vi.fn(),
  mlxDistributedProvision: vi.fn(),
  mlxDistributedStatus: vi.fn(),
}));
vi.mock('./LocalNetworkNotice', () => ({ touchLocalNetwork: vi.fn(async () => undefined) }));
vi.mock('../../acp/leanzero-link', async (importActual) => ({
  ...(await importActual<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkStatus: vi.fn(async () => ({ auth: { state: 'loggedOut' }, nodeCount: 0 })),
  leanzeroLinkNodes: vi.fn(async () => null),
}));
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: vi.fn(),
  mlxEngineStatus: vi.fn(async () => ({ state: 'stopped', restartRequired: false })),
  mlxEngineModelsList: vi.fn(async () => ({ models: [] })),
  mlxEngineDownload: vi.fn(),
  mlxEngineDownloadCancel: vi.fn(),
  mlxEngineDownloadPause: vi.fn(),
  mlxEngineDownloadProgress: vi.fn(async () => null),
  mlxEngineDownloadResume: vi.fn(),
  mlxEngineModelDelete: vi.fn(),
}));
vi.mock('../../acp/mlx-replica', () => ({
  mlxEngineReplicaTargets: vi.fn(async () => ({ meshConnected: false, targets: [] })),
  mlxEngineReplicate: vi.fn(),
  mlxEngineReplicaProgress: vi.fn(async () => null),
  mlxEngineReplicaCancel: vi.fn(),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ leanzeroLink: true, mlxDistributed: true, mlxEngine: true }),
}));
const mockNodesRead = vi.fn();
const mockNodesWrite = vi.fn();
vi.mock('../../acp/nodes', () => ({
  nodesRead: (...a: unknown[]) => mockNodesRead(...a),
  nodesWrite: (...a: unknown[]) => mockNodesWrite(...a),
}));
const mockRefreshGlanceNodes = vi.fn();
vi.mock('../engineGlance/glanceStore', () => ({
  refreshGlanceNodes: () => mockRefreshGlanceNodes(),
}));

const MODEL = PLAN_27B.modelId;

/** The 27B plan with this Mac's single made to fit, so Run on this Mac is offered. */
const PLAN_LOCAL_FITS: PlacementPlan = {
  ...PLAN_27B,
  candidates: (PLAN_27B.candidates ?? []).map((c) =>
    c.id === 'single:local'
      ? { ...c, fit: { ...c.fit, status: 'fits', shortBytes: null }, outcome: { code: 'best' } }
      : c
  ),
};

const EMPTY: NodesConfig = { version: 1, defs: [], strategies: [], declined: [] };

function readOf(config: NodesConfig): NodesRead {
  return {
    config,
    nodes: (config.defs ?? []).map((def) => ({
      def,
      model: def.model,
      provider: def.provider,
      modelFrom: { kind: 'own' },
    })),
    stored: true,
    lmStudioHidden: 0,
  };
}

const RUNNING_LOCAL: MlxEngineStatus = {
  state: 'running',
  modelId: MODEL,
  restartRequired: false,
} as MlxEngineStatus;

function renderCard(props: Partial<Parameters<typeof PlacementCard>[0]> = {}) {
  const onMountHere = vi.fn();
  const view = render(
    <IntlTestWrapper>
      <PlacementCard
        modelId={MODEL}
        single={null}
        distributed={null}
        onMountHere={onMountHere}
        onStopHere={vi.fn()}
        mountBusy={false}
        distributedCapability
        {...props}
      />
    </IntlTestWrapper>
  );
  return { ...view, onMountHere };
}

beforeEach(() => {
  sessionStorage.clear();
  mockPlan.mockResolvedValue({ plans: [PLAN_27B], nodes: NODES, storeErrors: [], probeMs: 1 });
  mockNodesRead.mockResolvedValue(readOf(EMPTY));
  mockNodesWrite.mockImplementation(async (config: NodesConfig) => ({
    written: true,
    refusals: [],
    read: readOf(config),
  }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('Run it · Save as node', () => {
  it('writes ONE def for the split through nodes/write, names it by model and Macs, and says where it is', async () => {
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-save-node-split'));

    await waitFor(() => expect(mockNodesWrite).toHaveBeenCalledTimes(1));
    const written = mockNodesWrite.mock.calls[0][0] as NodesConfig;
    expect(written.defs).toHaveLength(1);
    const def = written.defs![0];
    expect(def).toMatchObject({
      kind: 'mlx',
      model: MODEL,
      goal: 'chat',
      origin: 'runIt',
      keepLoaded: false,
      name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
      id: 'qwen3-8-27b-atlassian-q8-mlx-both-macs',
    });
    // The split row is goose's plan row: its key (kind, Macs in rank order, link) is the way.
    const split0 = (PLAN_27B.candidates ?? []).find(
      (c) => c.id === 'tensor:jaccl:local+workhorse'
    )!;
    expect(def.placement).toEqual({ kind: 'tensor', macs: split0.key.nodes, link: 'jaccl' });
    expect(mockRefreshGlanceNodes).toHaveBeenCalledTimes(1);

    const saved = await within(split).findByTestId('placement-saved-split');
    expect(saved).toHaveTextContent(
      'Saved as the node “Qwen3.8-27B-Atlassian-Q8-mlx · both Macs”.'
    );
    expect(within(saved).getByRole('link', { name: 'Open in Nodes' })).toHaveAttribute(
      'href',
      '#/nodes?tab=nodes&node=qwen3-8-27b-atlassian-q8-mlx-both-macs'
    );
    // Saved: the row no longer offers the save.
    expect(within(split).queryByTestId('placement-save-node-split')).toBeNull();
  });

  it('a node that already names the model and the way is shown, never duplicated', async () => {
    const split0 = (PLAN_27B.candidates ?? []).find(
      (c) => c.id === 'tensor:jaccl:local+workhorse'
    )!;
    mockNodesRead.mockResolvedValue(
      readOf({
        ...EMPTY,
        defs: [
          {
            id: 'mine',
            name: '27B · both',
            kind: 'mlx',
            model: MODEL,
            placement: { kind: 'tensor', macs: split0.key.nodes, link: 'jaccl' },
            origin: 'user',
          },
        ],
      })
    );
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-save-node-split'));
    expect(await within(split).findByTestId('placement-saved-split')).toHaveTextContent(
      'Already a node: “27B · both”.'
    );
    expect(mockNodesWrite).not.toHaveBeenCalled();
  });

  it('a name another node carries gets the next free one', async () => {
    mockNodesRead.mockResolvedValue(
      readOf({
        ...EMPTY,
        defs: [
          {
            id: 'other',
            name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
            kind: 'cloud',
            model: 'x',
            provider: 'openrouter',
            origin: 'user',
          },
        ],
      })
    );
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-save-node-split'));
    await waitFor(() => expect(mockNodesWrite).toHaveBeenCalledTimes(1));
    const defs = (mockNodesWrite.mock.calls[0][0] as NodesConfig).defs!;
    expect(defs.map((d) => d.name)).toEqual([
      'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
      'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs (2)',
    ]);
  });

  it('a refusal is goose’s words, and nothing claims it saved', async () => {
    mockNodesWrite.mockResolvedValue({
      written: false,
      refusals: [{ code: 'badMacs', message: 'the Mac "workhorse" is not on LeanZero Link' }],
      read: readOf(EMPTY),
    });
    renderCard();
    const split = await screen.findByTestId('placement-way-split');
    await userEvent.click(within(split).getByTestId('placement-save-node-split'));
    expect(await within(split).findByTestId('placement-save-refused-split')).toHaveTextContent(
      'Not saved: the Mac "workhorse" is not on LeanZero Link'
    );
    expect(mockRefreshGlanceNodes).not.toHaveBeenCalled();
  });

  it('a way the planner judged too big is not offered as a node', async () => {
    renderCard();
    const local = await screen.findByTestId('placement-way-local');
    expect(within(local).queryByTestId('placement-save-node-local')).toBeNull();
  });

  it('after a start here, the running row asks once to keep the way as a node', async () => {
    mockPlan.mockResolvedValue({
      plans: [PLAN_LOCAL_FITS],
      nodes: NODES,
      storeErrors: [],
      probeMs: 1,
    });
    const { rerender, onMountHere } = renderCard();
    const local = await screen.findByTestId('placement-way-local');
    await userEvent.click(within(local).getByTestId('placement-run-local'));
    expect(onMountHere).toHaveBeenCalledTimes(1);
    rerender(
      <IntlTestWrapper>
        <PlacementCard
          modelId={MODEL}
          single={RUNNING_LOCAL}
          distributed={null}
          onMountHere={onMountHere}
          onStopHere={vi.fn()}
          mountBusy={false}
          distributedCapability
        />
      </IntlTestWrapper>
    );
    const offer = await screen.findByTestId('placement-save-offer-local');
    expect(offer).toHaveTextContent('Save this way as a node so chats and builds can pick it');
    await userEvent.click(within(offer).getByTestId('placement-save-offer-button-local'));
    await waitFor(() => expect(mockNodesWrite).toHaveBeenCalledTimes(1));
    expect((mockNodesWrite.mock.calls[0][0] as NodesConfig).defs![0]).toMatchObject({
      name: 'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac',
      placement: { kind: 'single', macs: ['local'] },
    });
    expect(screen.queryByTestId('placement-save-offer-local')).toBeNull();
  });

  it('a running way nobody started here offers only the quiet row button', async () => {
    mockPlan.mockResolvedValue({
      plans: [PLAN_LOCAL_FITS],
      nodes: NODES,
      storeErrors: [],
      probeMs: 1,
    });
    renderCard({ single: RUNNING_LOCAL });
    const local = await screen.findByTestId('placement-way-local');
    expect(within(local).getByTestId('placement-save-node-local')).toBeInTheDocument();
    expect(screen.queryByTestId('placement-save-offer-local')).toBeNull();
  });

  it('the setup strip’s "Save as a node" saves the running way once the plan has landed', async () => {
    mockPlan.mockResolvedValue({
      plans: [PLAN_LOCAL_FITS],
      nodes: NODES,
      storeErrors: [],
      probeMs: 1,
    });
    const onHandled = vi.fn();
    renderCard({
      single: RUNNING_LOCAL,
      saveRunningPending: true,
      onSaveRunningHandled: onHandled,
    });
    await waitFor(() => expect(mockNodesWrite).toHaveBeenCalledTimes(1));
    expect(onHandled).toHaveBeenCalledTimes(1);
    expect((mockNodesWrite.mock.calls[0][0] as NodesConfig).defs![0].placement).toEqual({
      kind: 'single',
      macs: ['local'],
    });
  });

  it('the strip’s save with nothing running on this card says so', async () => {
    const onHandled = vi.fn();
    renderCard({ saveRunningPending: true, onSaveRunningHandled: onHandled });
    expect(
      await screen.findByText(
        'Qwen3.8-27B-Atlassian-Q8-mlx is not running on any way shown here — pick the running model above.'
      )
    ).toBeInTheDocument();
    expect(mockNodesWrite).not.toHaveBeenCalled();
    expect(onHandled).toHaveBeenCalledTimes(1);
  });

  it('is Studio-clean with the save controls drawn', async () => {
    const { container } = renderCard();
    await screen.findByTestId('placement-save-node-split');
    assertStudioClean(container);
  });
});

describe('usePlacementPlans — a failed read is a state, never an empty map (D10)', () => {
  it('asks the planner for the goal it is given', async () => {
    mockPlan.mockResolvedValue({
      plans: [PLAN_27B],
      nodes: NODES,
      storeErrors: ['line 3'],
      probeMs: 1,
    });
    for (const goal of ['chat', 'longDocuments', 'manyRequests'] as const) {
      const { result, unmount } = renderHook(() => usePlacementPlans(goal, 'k'));
      expect(result.current).toEqual({ kind: 'reading' });
      await waitFor(() => expect(result.current.kind).toBe('read'));
      expect(mockPlan).toHaveBeenLastCalledWith(goal, undefined);
      const state = result.current;
      expect(state.kind === 'read' && state.plans.get(MODEL)).toBe(PLAN_27B);
      expect(state.kind === 'read' && state.storeErrors).toEqual(['line 3']);
      unmount();
    }
  });

  it('one model, when asked for one', async () => {
    const { result } = renderHook(() => usePlacementPlans('chat', 'k', MODEL));
    await waitFor(() => expect(result.current.kind).toBe('read'));
    expect(mockPlan).toHaveBeenLastCalledWith('chat', MODEL);
  });

  it('a failed read carries goose’s words', async () => {
    mockPlan.mockRejectedValue(
      Object.assign(new Error('Invalid params'), { data: 'Link is down' })
    );
    const { result } = renderHook(() => usePlacementPlans('chat', 'k'));
    await waitFor(() => expect(result.current).toEqual({ kind: 'failed', error: 'Link is down' }));
  });

  it('no goal: nothing is read', () => {
    const { result } = renderHook(() => usePlacementPlans(null, 'k'));
    expect(result.current).toEqual({ kind: 'idle' });
    expect(mockPlan).not.toHaveBeenCalled();
  });

  it('a re-read after a change keeps the last answer until the new one lands', async () => {
    const { result, rerender } = renderHook(({ key }) => usePlacementPlans('chat', key), {
      initialProps: { key: 'a' },
    });
    await waitFor(() => expect(result.current.kind).toBe('read'));
    let resolve: (v: unknown) => void = () => undefined;
    mockPlan.mockReturnValue(new Promise((r) => (resolve = r)));
    rerender({ key: 'b' });
    expect(result.current.kind).toBe('read');
    resolve({ plans: [], nodes: NODES, storeErrors: [], probeMs: 1 });
    await waitFor(() =>
      expect(result.current.kind === 'read' && result.current.plans.size).toBe(0)
    );
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { NodesRead, Residency } from '../../acp/nodes';
import type { ResolvedNodeDef } from '../nodes/model';
import {
  resetEngineGlanceForTests,
  servingReportOf,
  useGlanceNodes,
  type GlanceNodesState,
} from './glanceStore';
import { ENGINE_GLANCE_CHANNEL, type GlancePush } from '../../utils/engineGlance';
import { glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import { GENERATING_STATUS, PREFILL_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';

const acp = vi.hoisted(() => ({
  read: vi.fn(),
  residency: vi.fn(),
  servedLast: vi.fn(),
}));
vi.mock('../../acp/nodes', () => ({
  nodesRead: () => acp.read(),
  nodesResidency: () => acp.residency(),
  nodesServedLast: (id: string) => acp.servedLast(id),
}));

function def(
  id: string,
  placement: ResolvedNodeDef['def']['placement'],
  kind: 'mlx' | 'cloud' = 'mlx'
): ResolvedNodeDef {
  return {
    def: { id, name: `${id} name`, kind, placement, origin: 'user' },
    modelFrom: { kind: 'own' },
  };
}

const SPLIT = def('split', { kind: 'pipeline', macs: ['local', 'link:studio'], link: 'jaccl' });
const SPLIT_TENSOR = def('split-tensor', {
  kind: 'tensor',
  macs: ['local', 'link:studio'],
  link: 'jaccl',
});
const FOLLOWS = def('mihai-engine', { kind: 'follows' });
const CLOUD = def('sonnet', null, 'cloud');

function readOf(nodes: ResolvedNodeDef[]): NodesRead {
  return { config: { version: 1 }, nodes, stored: true, lmStudioHidden: 0 };
}

const SERVING_SPLIT: Residency = {
  loaderInstalled: false,
  serving: {
    kind: 'split',
    macs: [],
    link: 'jaccl',
    modelId: 'Mihai-LeanZero/Qwen3.8-27B',
    servedModelId: 'qwen-27b',
    macNames: ['Mihai Macbook', 'Work’s Mac Studio'],
  },
  nodes: [
    { node: 'mihai-engine', residency: { kind: 'serving' } },
    { node: 'split', residency: { kind: 'serving' } },
    { node: 'split-tensor', residency: { kind: 'serving' } },
    { node: 'sonnet', residency: { kind: 'alwaysReady' } },
  ],
};

const read = (
  nodes: ResolvedNodeDef[],
  residency: Residency,
  servedNode: string | null = null
): GlanceNodesState => ({ kind: 'read', read: readOf(nodes), residency, servedNode });

describe('servingReportOf — the node(s) a window reports for the serving way', () => {
  it('pinned nodes lead, nodes that follow this Mac’s engine come last; cloud never serves the engine', () => {
    expect(servingReportOf(read([FOLLOWS, SPLIT, CLOUD, SPLIT_TENSOR], SERVING_SPLIT))).toEqual({
      way: { kind: 'split', modelId: 'Mihai-LeanZero/Qwen3.8-27B', servedModelId: 'qwen-27b' },
      nodes: [
        { id: 'split', name: 'split name' },
        { id: 'split-tensor', name: 'split-tensor name' },
        { id: 'mihai-engine', name: 'mihai-engine name' },
      ],
    });
  });

  it('the chat’s own served node leads when it names the serving way', () => {
    const report = servingReportOf(
      read([FOLLOWS, SPLIT, SPLIT_TENSOR], SERVING_SPLIT, 'split-tensor')
    );
    expect(report && 'nodes' in report && report.nodes.map((n) => n.id)).toEqual([
      'split-tensor',
      'split',
      'mihai-engine',
    ]);
  });

  it('a node the loader is loading counts only while the serving way itself is loading', () => {
    const loaderLoading: Residency = {
      ...SERVING_SPLIT,
      nodes: [
        { node: 'split', residency: { kind: 'serving' } },
        { node: 'mihai-engine', residency: { kind: 'loading', phase: 'loading' } },
      ],
    };
    const report = servingReportOf(read([FOLLOWS, SPLIT], loaderLoading));
    expect(report && 'nodes' in report && report.nodes.map((n) => n.id)).toEqual(['split']);
    const wayLoading: Residency = {
      ...loaderLoading,
      serving: { ...SERVING_SPLIT.serving!, loadPhase: 'loading' },
      nodes: [{ node: 'split', residency: { kind: 'loading', phase: 'loading' } }],
    };
    const loading = servingReportOf(read([SPLIT], wayLoading));
    expect(loading && 'nodes' in loading && loading.nodes.map((n) => n.id)).toEqual(['split']);
  });

  it('nothing read yet, or nothing serving: no report — never an empty claim', () => {
    expect(servingReportOf({ kind: 'unread' })).toBeNull();
    expect(servingReportOf(read([SPLIT], { loaderInstalled: false, nodes: [] }))).toBeNull();
  });

  it('a failed read, or goosed not knowing which way serves, is reported in its own words', () => {
    expect(servingReportOf({ kind: 'failed', error: 'goosed unreachable' })).toEqual({
      error: 'goosed unreachable',
    });
    expect(
      servingReportOf(
        read([SPLIT], {
          loaderInstalled: false,
          nodes: [],
          servingError: 'the route record is unreadable',
        })
      )
    ).toEqual({ error: 'the route record is unreadable' });
  });
});

describe('the nodes read follows the glance — an event, never a clock', () => {
  let onPush: ((event: unknown, ...args: unknown[]) => void) | null = null;
  const holder = window as unknown as { electron: unknown };
  let original: unknown;

  beforeEach(() => {
    original = holder.electron;
    onPush = null;
    acp.read.mockResolvedValue(readOf([SPLIT]));
    acp.residency.mockResolvedValue(SERVING_SPLIT);
    acp.servedLast.mockResolvedValue({});
    holder.electron = {
      on: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => {
        if (channel === ENGINE_GLANCE_CHANNEL) onPush = fn;
      },
      off: vi.fn(),
      engineGlanceRead: vi.fn(async () => null),
    };
  });

  afterEach(() => {
    resetEngineGlanceForTests(null);
    holder.electron = original;
    vi.clearAllMocks();
  });

  const push = (p: GlancePush) => act(() => onPush?.(null, p));

  it('reads once on mount, again when the way’s stage changes, not for a figure that moved', async () => {
    const { result } = renderHook(() => useGlanceNodes());
    await waitFor(() => expect(result.current.kind).toBe('read'));
    expect(acp.read).toHaveBeenCalledTimes(1);

    push(glancePush(runningSnapshot(PREFILL_STATUS)));
    await waitFor(() => expect(acp.read).toHaveBeenCalledTimes(2));
    push(glancePush(runningSnapshot(GENERATING_STATUS)));
    await waitFor(() => expect(acp.read).toHaveBeenCalledTimes(3));
    // The same stage again (only its figures would differ): no read.
    push(glancePush(runningSnapshot(GENERATING_STATUS)));
    await Promise.resolve();
    expect(acp.read).toHaveBeenCalledTimes(3);
  });

  it('a failed read is a state carrying the words, not an empty list', async () => {
    acp.residency.mockRejectedValue(Object.assign(new Error('Invalid params'), { data: 'boom' }));
    const { result } = renderHook(() => useGlanceNodes());
    await waitFor(() => expect(result.current).toEqual({ kind: 'failed', error: 'boom' }));
  });
});

import { describe, expect, it } from 'vitest';
import type { NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import {
  displacedOf,
  measuredLoadOf,
  nodeBehindSwitchOf,
  nodeRefusalOf,
  nodeWaitOf,
  isNodeSwap,
  loadIsOthers,
  nodeSwapOf,
  routeNodeIds,
  swapOfReports,
  swapStopsEngine,
  turnCanWaitInLoader,
  type NodeSwap,
} from './nodeSwap';
import {
  J3_BUILD_NODE,
  J3_CHAT_NODE,
  J3_MODEL,
  J3_OWN_LOAD,
  J3_READ,
  J3_SERVING_SINGLE,
  J3_STRATEGY,
  J3_SWAP_TO_SINGLE,
  J3_SWAP_TO_SPLIT,
} from './nodeSwap.fixtures';

describe('nodeSwapOf — the loader’s own mark, by the node’s name', () => {
  it('J3 delegate swap: the split the loader loads, named as the Nodes page names it', () => {
    expect(nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT)).toEqual({
      target: {
        id: J3_BUILD_NODE.def.id,
        name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
        modelId: J3_MODEL,
        way: 'split',
      },
      phase: null,
      load: null,
      demandedBy: [],
    });
  });

  it('Q-434: the swap carries whom the loader loads it for; a load is another’s only by that', () => {
    const forDelegates = {
      ...J3_SWAP_TO_SPLIT,
      nodes: J3_SWAP_TO_SPLIT.nodes.map((r) =>
        r.residency.kind === 'loading'
          ? { ...r, residency: { ...r.residency, demandedBy: ['20260928_42'] } }
          : r
      ),
    };
    const swap = nodeSwapOf(J3_READ, forDelegates)!;
    expect(swap.demandedBy).toEqual(['20260928_42']);
    expect(loadIsOthers(swap, 'chat-1')).toBe(true);
    expect(loadIsOthers(swap, '20260928_42')).toBe(false);
    // No demand behind it (Run it, a restore): it is claimed for no one.
    expect(loadIsOthers(nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT)!, 'chat-1')).toBe(false);
  });

  it('a way two nodes name: the pinned node leads the one that follows this Mac', () => {
    const swap = nodeSwapOf(J3_READ, J3_SWAP_TO_SINGLE);
    expect(swap?.target.name).toBe('Qwen3.8-27B-Atlassian-Q8-mlx · this Mac');
    expect(swap?.target.way).toBe('single');
    expect(swap?.phase).toBe('loading');
  });

  it('nothing loading is no swap — between swaps the engines speak for themselves', () => {
    expect(nodeSwapOf(J3_READ, J3_SERVING_SINGLE)).toBeNull();
  });
});

describe('swapStopsEngine — the swap’s stop is not a failure; a real failure stays one', () => {
  const toSplit = nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT);
  const toSingle = nodeSwapOf(J3_READ, J3_OWN_LOAD);

  it('J3: the 27B single SIGTERMed (exit 143) while the split loads is the swap', () => {
    expect(swapStopsEngine(toSplit, { way: 'single', modelId: J3_MODEL, failed: true })).toBe(true);
  });

  it('a way stopped cleanly while any node loads is the swap', () => {
    expect(swapStopsEngine(toSingle, { way: 'single', modelId: J3_MODEL, failed: false })).toBe(
      true
    );
  });

  it('negative control: the node being loaded failing on its own way is a real failure', () => {
    expect(swapStopsEngine(toSingle, { way: 'single', modelId: J3_MODEL, failed: true })).toBe(
      false
    );
  });

  it('negative control: no swap, no excuse', () => {
    expect(swapStopsEngine(null, { way: 'single', modelId: J3_MODEL, failed: true })).toBe(false);
  });

  it('a model either side does not know keeps the failure', () => {
    const unknownModel: NodeSwap = {
      target: { ...toSingle!.target, modelId: null },
      phase: null,
      load: null,
      demandedBy: [],
    };
    expect(swapStopsEngine(unknownModel, { way: 'single', modelId: J3_MODEL, failed: true })).toBe(
      false
    );
  });
});

describe('routeNodeIds — what a chat’s model runs on', () => {
  it('a strategy runs on every node of its chains; a node on itself; others on none named', () => {
    expect(routeNodeIds(J3_READ, J3_STRATEGY)).toEqual([J3_CHAT_NODE.def.id, J3_BUILD_NODE.def.id]);
    expect(routeNodeIds(J3_READ, `${J3_STRATEGY}@build`)).toEqual([
      J3_CHAT_NODE.def.id,
      J3_BUILD_NODE.def.id,
    ]);
    expect(routeNodeIds(J3_READ, `node:${J3_CHAT_NODE.def.id}`)).toEqual([J3_CHAT_NODE.def.id]);
    expect(routeNodeIds(J3_READ, 'swarm')).toBeNull();
    expect(routeNodeIds(J3_READ, 'strategy:gone')).toEqual([]);
  });
});

describe('the IPC report', () => {
  it('a window’s swap passes the boundary check; a malformed one does not', () => {
    const swap = nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT);
    expect(isNodeSwap(swap)).toBe(true);
    expect(isNodeSwap({ target: { id: 'x', name: 'x', modelId: null, way: 'nowhere' } })).toBe(
      false
    );
    expect(swapOfReports([{ swap: null }, { swap }, {}])).toEqual(swap);
    expect(swapOfReports([{}, { swap: null }])).toBeNull();
  });
});

describe('Q-272: the loader’s facts, as every surface reads them', () => {
  const rows = (
    r: Record<string, NodesResidencyResponse_unstable['nodes'][number]>,
    extra: Partial<NodesResidencyResponse_unstable> = {}
  ): NodesResidencyResponse_unstable => ({
    ...J3_SERVING_SINGLE,
    nodes: J3_SERVING_SINGLE.nodes.map((row) => r[row.node] ?? row),
    ...extra,
  });
  const CHAT = J3_CHAT_NODE.def.id;
  const SPLIT = J3_BUILD_NODE.def.id;

  it('a wait for replies names the way by its node, else in the loader’s words', () => {
    const waiting = (wayNodes: string[]) =>
      rows({
        [SPLIT]: {
          node: SPLIT,
          residency: {
            kind: 'waiting',
            reason: 'r',
            replies: { way: "this Mac's engine", wayNodes, count: 2 },
          },
          load: { medianMs: 1000, count: 1 },
        },
      });
    expect(nodeWaitOf(J3_READ, waiting([CHAT]), [SPLIT])).toEqual({
      target: expect.objectContaining({ id: SPLIT }),
      reason: 'r',
      // An older goosed names no chats: an empty list, never a guessed one.
      replies: { way: 'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac', count: 2, chats: [] },
      servingOther: null,
      behind: null,
      load: { medianMs: 1000, count: 1 },
    });
    expect(nodeWaitOf(J3_READ, waiting(['gone']), [SPLIT])?.replies?.way).toBe("this Mac's engine");
    // A wait for a node the chat does not run on is not this chat's.
    expect(nodeWaitOf(J3_READ, waiting([CHAT]), [CHAT])).toBeNull();
  });

  it('Q-442: a reply behind a queued switch is its own chat’s, named by the switch’s node', () => {
    const behind = rows(
      {},
      {
        behindSwitches: [
          {
            session: 'chat-3',
            node: CHAT,
            switchTo: SPLIT,
            switchToName: 'split node',
            chats: ['Kickoff'],
          },
        ],
        nodes: J3_SERVING_SINGLE.nodes.map((row) =>
          row.node === SPLIT ? { ...row, load: { medianMs: 2000, count: 3 } } : row
        ),
      }
    );
    expect(nodeBehindSwitchOf(J3_READ, behind, 'chat-3')).toEqual({
      target: expect.objectContaining({ id: CHAT }),
      reason: '',
      replies: null,
      servingOther: null,
      behind: {
        switchTo: expect.objectContaining({
          id: SPLIT,
          name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
        }),
        chats: ['Kickoff'],
      },
      load: { medianMs: 2000, count: 3 },
    });
    // Another chat is not held by it, and neither is a chat with no session yet.
    expect(nodeBehindSwitchOf(J3_READ, behind, 'chat-1')).toBeNull();
    expect(nodeBehindSwitchOf(J3_READ, behind, null)).toBeNull();
  });

  it('a refusal carries its facts; a node that is not refused has none', () => {
    const refused = rows({
      [SPLIT]: {
        node: SPLIT,
        residency: {
          kind: 'refusedLastTime',
          reason: 'x',
          facts: { kind: 'heldByBuild', way: "this Mac's engine" },
        },
      },
    });
    expect(nodeRefusalOf(J3_READ, refused, SPLIT)?.facts).toEqual({
      kind: 'heldByBuild',
      way: "this Mac's engine",
    });
    expect(nodeRefusalOf(J3_READ, refused, CHAT)).toBeNull();
  });

  it('a displaced node: told to every chat but the one that asked, and gone once it serves', () => {
    const displaced = [{ node: CHAT, forNode: SPLIT, bySession: 'kickoff', byChat: 'K', atMs: 1 }];
    const stopped = rows(
      { [CHAT]: { node: CHAT, residency: { kind: 'notRunning' } } },
      { displaced }
    );
    expect(displacedOf(J3_READ, stopped, CHAT, 'chat-1', CHAT)).toMatchObject({
      node: { id: CHAT },
      other: { id: SPLIT, name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs' },
      chat: 'K',
      failed: null,
    });
    expect(displacedOf(J3_READ, stopped, CHAT, 'kickoff', CHAT)).toBeNull();
    // A read that raced its return: the node serves, nothing is said.
    expect(
      displacedOf(J3_READ, { ...J3_SERVING_SINGLE, displaced }, CHAT, 'chat-1', CHAT)
    ).toBeNull();
    // Q-435: only a chat the node was serving — never one that never ran (null), ran elsewhere,
    // or whose record was not read (undefined).
    expect(displacedOf(J3_READ, stopped, CHAT, 'chat-1', null)).toBeNull();
    expect(displacedOf(J3_READ, stopped, CHAT, 'chat-1', SPLIT)).toBeNull();
    expect(displacedOf(J3_READ, stopped, CHAT, 'chat-1', undefined)).toBeNull();
  });

  it('the swap carries its measured load across the IPC boundary; a malformed load does not pass', () => {
    const measured = rows({
      [SPLIT]: { node: SPLIT, residency: { kind: 'loading' }, load: { medianMs: 5, count: 2 } },
    });
    const swap = nodeSwapOf(J3_READ, measured);
    expect(swap?.load).toEqual({ medianMs: 5, count: 2 });
    expect(isNodeSwap(swap)).toBe(true);
    expect(isNodeSwap({ ...swap, load: { medianMs: '5' } })).toBe(false);
    expect(measuredLoadOf(measured, CHAT)).toBeNull();
  });
});

describe('Q-430 / Q-442: a routed turn on an MLX node can wait in the loader', () => {
  it('whether or not its node serves (Q-442: behind a queued switch, its own node serves)', () => {
    expect(turnCanWaitInLoader(J3_READ, `node:${J3_BUILD_NODE.def.id}`)).toBe(true);
    expect(turnCanWaitInLoader(J3_READ, `node:${J3_CHAT_NODE.def.id}`)).toBe(true);
    expect(turnCanWaitInLoader(J3_READ, J3_STRATEGY)).toBe(true);
    // Auto and cloud-only routes never wait on the loader.
    expect(turnCanWaitInLoader(J3_READ, 'swarm')).toBe(false);
  });
});

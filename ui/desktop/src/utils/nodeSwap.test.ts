import { describe, expect, it } from 'vitest';
import {
  isNodeSwap,
  nodeSwapOf,
  routeNodeIds,
  swapOfReports,
  swapStopsEngine,
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
    });
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

import { describe, expect, it } from 'vitest';
import {
  NODE_CLOUD,
  NODE_FLASH,
  NODE_LOCAL_27B,
  NODE_POOL,
  NODE_SPLIT,
  NODE_STUDIO,
} from './nodeGlance.fixtures';
import type { NodeRoleEntry, NodeStrategy, ResolvedNodeDef } from './model';
import { strategyFit, wayKeyOf, type MeasuredLoad } from './strategyFit';

const NODES = [NODE_SPLIT, NODE_STUDIO, NODE_LOCAL_27B, NODE_FLASH, NODE_POOL, NODE_CLOUD];
const MEASURED: Record<string, { medianMs: number; count: number }> = {
  [NODE_FLASH.def.id]: { medianMs: 48_000, count: 3 },
};
const measured: MeasuredLoad = (n: ResolvedNodeDef) => MEASURED[n.def.id] ?? null;

const entry = (
  nodes: ResolvedNodeDef[],
  when: NodeRoleEntry['when'] = 'failover',
  ifNotLoaded: NodeRoleEntry['ifNotLoaded'] = 'load'
): NodeRoleEntry => ({
  chain: nodes.map((n) => ({ node: n.def.id, weight: 1 })),
  when,
  ifNotLoaded,
});

const strategy = (roles: NodeStrategy['roles']): NodeStrategy => ({ id: 's', name: 'S', roles });

describe('strategyFit — one MLX way serves this Mac’s goose at a time, across all Macs', () => {
  it('one way on one Mac: no swaps, its measured load carried', () => {
    const fit = strategyFit(strategy({ chat: entry([NODE_FLASH]) }), NODES, measured);
    expect(fit.ways.map((w) => w.nodes.map((n) => n.def.id))).toEqual([[NODE_FLASH.def.id]]);
    expect(fit.ways[0].load).toEqual({ medianMs: 48_000, count: 3 });
    expect(fit.swaps).toEqual([]);
    expect(fit.delegate).toBeNull();
  });

  it('a split and a single on another Mac swap — the rule is one way at a time, not a shared Mac', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_SPLIT]), planning: entry([NODE_STUDIO]) }),
      NODES,
      measured
    );
    expect(fit.ways).toHaveLength(2);
    expect(fit.swaps).toHaveLength(1);
    expect(fit.swaps[0].map((w) => w.nodes[0].def.id)).toEqual([
      NODE_SPLIT.def.id,
      NODE_STUDIO.def.id,
    ]);
    // Neither is measured: said as such, never estimated.
    expect(fit.swaps[0].map((w) => w.load)).toEqual([null, null]);
  });

  it('two singles on DISJOINT Macs still swap (the Studio’s 27B and this Mac’s Flash)', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_FLASH]), testing: entry([NODE_STUDIO]) }),
      NODES,
      measured
    );
    expect(fit.swaps).toHaveLength(1);
  });

  it('the same model on another way is another way (27B split vs 27B on this Mac)', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_SPLIT, NODE_LOCAL_27B]) }),
      NODES,
      measured
    );
    expect(fit.ways).toHaveLength(2);
    expect(fit.swaps).toHaveLength(1);
  });

  it('three ways make every pair a swap', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_SPLIT, NODE_STUDIO, NODE_FLASH]) }),
      NODES,
      measured
    );
    expect(fit.swaps).toHaveLength(3);
  });

  it('Chat and Build on different MLX ways warn that every delegate call swaps twice', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_FLASH]), build: entry([NODE_SPLIT]) }),
      NODES,
      measured
    );
    expect(fit.delegate?.chat.nodes[0].def.id).toBe(NODE_FLASH.def.id);
    expect(fit.delegate?.build.nodes[0].def.id).toBe(NODE_SPLIT.def.id);
  });

  it('no delegate warning when Build uses the next node instead of loading', () => {
    const fit = strategyFit(
      strategy({
        chat: entry([NODE_FLASH]),
        build: entry([NODE_SPLIT, NODE_CLOUD], 'failover', 'useNext'),
      }),
      NODES,
      measured
    );
    expect(fit.delegate).toBeNull();
    expect(fit.swaps).toHaveLength(1);
  });

  it('no delegate warning when Build is unset (Same as Chat) — a new strategy never swaps', () => {
    const fit = strategyFit(strategy({ chat: entry([NODE_SPLIT]) }), NODES, measured);
    expect(fit.delegate).toBeNull();
    expect(fit.swaps).toEqual([]);
  });

  it('one MLX way plus a cloud node: no swaps, the cloud node always available', () => {
    const fit = strategyFit(
      strategy({
        chat: entry([NODE_SPLIT, NODE_CLOUD]),
        build: entry([NODE_SPLIT, NODE_CLOUD], 'share'),
      }),
      NODES,
      measured
    );
    expect(fit.ways).toHaveLength(1);
    expect(fit.ways[0].uses).toEqual([
      { role: 'chat', rank: 1 },
      { role: 'build', rank: 1 },
    ]);
    expect(fit.swaps).toEqual([]);
    expect(fit.cloud.map((n) => n.def.id)).toEqual([NODE_CLOUD.def.id]);
    expect(fit.shareTwoWays).toEqual([]);
  });

  it('cloud only: nothing loads on your Macs', () => {
    const fit = strategyFit(strategy({ chat: entry([NODE_CLOUD]) }), NODES, measured);
    expect(fit.ways).toEqual([]);
    expect(fit.swaps).toEqual([]);
    expect(fit.cloud).toHaveLength(1);
  });

  it('a node that follows this Mac’s engine names no way and never swaps', () => {
    const fit = strategyFit(
      strategy({ chat: entry([NODE_POOL]), build: entry([NODE_SPLIT]) }),
      NODES,
      measured
    );
    expect(wayKeyOf(NODE_POOL)).toBeNull();
    expect(fit.follows.map((n) => n.def.id)).toEqual([NODE_POOL.def.id]);
    expect(fit.ways).toHaveLength(1);
    expect(fit.swaps).toEqual([]);
    expect(fit.delegate).toBeNull();
  });

  it('share or overflow across two MLX ways is flagged (goosed refuses it); failover is not', () => {
    const shared = strategyFit(
      strategy({ build: entry([NODE_SPLIT, NODE_FLASH], 'share'), chat: entry([NODE_CLOUD]) }),
      NODES,
      measured
    );
    expect(shared.shareTwoWays).toEqual([
      { role: 'build', a: NODE_SPLIT.def.name, b: NODE_FLASH.def.name },
    ]);
    const overflow = strategyFit(
      strategy({ chat: entry([NODE_SPLIT, NODE_FLASH], 'overflow') }),
      NODES,
      measured
    );
    expect(overflow.shareTwoWays.map((s) => s.role)).toEqual(['chat']);
    const failover = strategyFit(
      strategy({ chat: entry([NODE_SPLIT, NODE_FLASH], 'failover') }),
      NODES,
      measured
    );
    expect(failover.shareTwoWays).toEqual([]);
  });

  it('two nodes naming the same way are one way (no swap between a node and its twin)', () => {
    const twin: ResolvedNodeDef = {
      ...NODE_FLASH,
      def: { ...NODE_FLASH.def, id: 'flash-twin', name: 'Flash twin' },
    };
    const fit = strategyFit(
      strategy({ chat: entry([NODE_FLASH]), build: entry([twin]) }),
      [...NODES, twin],
      measured
    );
    expect(fit.ways).toHaveLength(1);
    expect(fit.ways[0].nodes.map((n) => n.def.id)).toEqual([NODE_FLASH.def.id, 'flash-twin']);
    expect(fit.delegate).toBeNull();
  });

  it('a chain id no definition carries is named, never dropped', () => {
    const fit = strategyFit(
      strategy({ chat: { chain: [{ node: 'ghost', weight: 1 }], when: 'failover' } }),
      NODES,
      measured
    );
    expect(fit.unknown).toEqual(['ghost']);
    expect(fit.ways).toEqual([]);
  });
});

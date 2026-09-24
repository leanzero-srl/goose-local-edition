import { describe, expect, it } from 'vitest';
import type { MlxDistributedDiscovery } from '../../acp/mlx-distributed';
import { cleanConfig, splitConfigFor, splitPlan } from './mlxDistributed';
import DISCOVERY from './mlxDistributedDiscovery.fixture.json';

const QWEN = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
const CHOSEN = DISCOVERY.chosen27b as MlxDistributedDiscovery;

/** The owner's saved split of the Flash model over the same two Macs, with his own edits. */
const SAVED = {
  ...CHOSEN.config,
  modelId: FLASH,
  port: 9191,
  coordinatorPort: 9192,
  context: 32768,
  watchdogWarnRatio: 0.07,
  nodes: CHOSEN.config.nodes.map((n, i) => ({
    ...n,
    python: `/custom/python-${i}`,
    modelDir: `/models/${FLASH}`,
    freeMemoryAutomatically: false,
  })),
};

const withEnv = (state: string): MlxDistributedDiscovery => ({
  ...CHOSEN,
  nodes: CHOSEN.nodes.map((n) => ({ ...n, env: { python: '/p', state, detail: '' } })),
});

describe('splitConfigFor — the owner’s split, switched to another model', () => {
  it('the same Macs: the saved setup survives; only the model’s own values come from the discovery', () => {
    const next = splitConfigFor(CHOSEN, SAVED);
    expect(next.modelId).toBe(QWEN);
    expect(next.port).toBe(9191);
    expect(next.coordinatorPort).toBe(9192);
    expect(next.watchdogWarnRatio).toBe(0.07);
    expect(next.nodes.map((n) => n.python)).toEqual(['/custom/python-0', '/custom/python-1']);
    expect(next.nodes.every((n) => n.freeMemoryAutomatically === false)).toBe(true);
    expect(next.nodes.map((n) => n.modelDir)).toEqual(CHOSEN.config.nodes.map((n) => n.modelDir));
    // The old model's context is not carried onto a model it was never planned for.
    expect('context' in next).toBe(false);
  });

  it('other Macs, or nothing saved: the discovery’s config as found', () => {
    expect(splitConfigFor(CHOSEN, null)).toBe(CHOSEN.config);
    const elsewhere = {
      ...SAVED,
      nodes: SAVED.nodes.map((n) => (n.ssh ? { ...n, ssh: 'link:mini' } : n)),
    };
    expect(splitConfigFor(CHOSEN, elsewhere)).toBe(CHOSEN.config);
  });
});

describe('splitPlan — only a genuinely missing piece stops the start, by Mac', () => {
  it('every Mac ready: nothing to build', () => {
    const d = withEnv('ready');
    expect(splitPlan(d, QWEN, cleanConfig(splitConfigFor(d, SAVED)))).toEqual({ provision: [] });
  });

  it('a Mac without goose’s Python is built first, named', () => {
    const d = withEnv('absent');
    expect(splitPlan(d, QWEN, cleanConfig(d.config))).toEqual({
      provision: ['Mihai Macbook', 'Work’s Mac Studio'],
    });
  });

  it('no uv on a Mac: it cannot be built, and that Mac is named', () => {
    const d = withEnv('noUv');
    expect(splitPlan(d, QWEN, cleanConfig(d.config))).toEqual({
      blocker: { kind: 'noUv', nodes: ['Mihai Macbook', 'Work’s Mac Studio'] },
    });
  });

  it('the model absent on a Mac: that Mac is named', () => {
    const d = withEnv('ready');
    const missing: MlxDistributedDiscovery = {
      ...d,
      models: d.models.map((m) =>
        m.id === QWEN
          ? {
              ...m,
              onEveryNode: false,
              nodes: m.nodes.map((n) => (n.rank === 1 ? { ...n, state: 'absent' } : n)),
            }
          : m
      ),
    };
    expect(splitPlan(missing, QWEN, cleanConfig(missing.config))).toEqual({
      blocker: { kind: 'modelMissing', nodes: ['Work’s Mac Studio'] },
    });
  });

  it('a model goose has no runner for is said in goose’s words', () => {
    const d = withEnv('ready');
    const gap = { field: 'modelId', reason: 'no splittable model named that on this Mac' };
    expect(splitPlan({ ...d, gaps: [gap] }, 'someone/else', cleanConfig(d.config))).toEqual({
      blocker: { kind: 'notSplittable', reason: gap.reason },
    });
  });

  it('a value the discovery could not find, and the saved setup does not carry, is named', () => {
    const d = withEnv('ready');
    const config = cleanConfig({
      ...d.config,
      nodes: d.config.nodes.map((n, i) => (i === 1 ? { ...n, tbIp: '' } : n)),
    });
    const gapped: MlxDistributedDiscovery = {
      ...d,
      gaps: [{ node: 1, field: 'tbIp', reason: 'no IPv4 on en3' }],
    };
    expect(splitPlan(gapped, QWEN, config)).toEqual({
      blocker: {
        kind: 'notFound',
        items: [{ node: 'Work’s Mac Studio', field: 'tbIp', reason: 'no IPv4 on en3' }],
      },
    });
  });
});

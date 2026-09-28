import { describe, expect, it } from 'vitest';
import { buildEngineGlance, isGlanceSessions, nodesNavChip } from './engineGlance';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import { buildMlxTrayModel, trayTitleText } from './mlxTray';
import { toMlxDistributedReport } from './mlxDistributedReport';
import { nodeSwapOf } from './nodeSwap';
import {
  J3_EXIT_143,
  J3_MODEL,
  J3_OWN_LOAD,
  J3_READ,
  J3_SWAP_TO_SPLIT,
  REAL_FAILURE,
} from './nodeSwap.fixtures';
import { FLASH_READY } from '../components/leanzero-swarm/mlxDistributed.fixtures';

/**
 * Q-254, main's surfaces (the glance card and pill, the Nodes nav chip, the tray): live J3 on 3.0.65
 * showed "Failed · Single · this Mac · … exit status: 143" on the glance, "Failed" on the nav chip
 * and "MLX failed" in the tray while the loader swapped the 27B single out for the split — a swap
 * that worked. Each fixture is a state from those screenshots; the negative controls are real
 * failures, which must still read Failed with their words.
 */

/** 19-j3-delegate-swap.png: this Mac's 27B engine, SIGTERMed by the swap's stop. */
const STOPPED_BY_SWAP: MlxEngineSnapshot = {
  ...INITIAL_SNAPSHOT,
  mode: 'failed',
  modelId: J3_MODEL,
  failedError: J3_EXIT_143,
};
const DIED: MlxEngineSnapshot = { ...STOPPED_BY_SWAP, failedError: REAL_FAILURE };

const toSplit = nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT);
const ownLoad = nodeSwapOf(J3_READ, J3_OWN_LOAD);
const OPTS = { distributed: null, remote: null, served: [] };

describe('the glance during a swap (Q-254)', () => {
  it('J3: the single the swap stopped reads "swapping" to the split, amber, no exit-143 words', () => {
    const g = buildEngineGlance(STOPPED_BY_SWAP, { ...OPTS, swap: toSplit });
    expect(g).toMatchObject({
      present: true,
      busy: true,
      phase: 'loading',
      stage: 'swapping',
      swapTo: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
      modelId: J3_MODEL,
      detail: null,
    });
    expect(nodesNavChip(g)).toBe('loading');
  });

  it('a way stopped cleanly while the loader loads is the swap too — never "Off"', () => {
    const g = buildEngineGlance({ ...INITIAL_SNAPSHOT, mode: 'off' }, { ...OPTS, swap: toSplit });
    expect(g.stage).toBe('swapping');
    expect(g.present).toBe(true);
  });

  it('the split stopping or failing as its ranks take the stop, while the single loads, is the swap', () => {
    const failedSplit = toMlxDistributedReport({
      ...FLASH_READY,
      modelId: J3_MODEL,
      state: 'failed',
      lastError: 'rank 1 exited: signal 15',
    });
    const g = buildEngineGlance(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: { report: failedSplit, ageMs: 0 },
      swap: ownLoad,
    });
    expect(g).toMatchObject({ stage: 'swapping', phase: 'loading', detail: null });
    expect(g.swapTo).toBe('Qwen3.8-27B-Atlassian-Q8-mlx · this Mac');
  });

  it('negative control: with no swap, a failed engine is Failed with its own words', () => {
    const g = buildEngineGlance(DIED, OPTS);
    expect(g).toMatchObject({ stage: 'failed', phase: 'failed', detail: REAL_FAILURE });
    expect(nodesNavChip(g)).toBe('failed');
  });

  it('negative control: the node being loaded failing on its own way is Failed, not a swap', () => {
    const g = buildEngineGlance(DIED, { ...OPTS, swap: ownLoad });
    expect(g).toMatchObject({ stage: 'failed', detail: REAL_FAILURE });
    expect(g.swapTo).toBeUndefined();
  });

  it('a window’s sessions report carries its swap across IPC; a malformed one is refused', () => {
    expect(isGlanceSessions({ running: 1, needsYou: [], swap: toSplit })).toBe(true);
    expect(isGlanceSessions({ running: 1, needsYou: [], swap: { target: 'x' } })).toBe(false);
  });
});

describe('the tray during a swap (Q-254)', () => {
  const TRAY = { canAct: true, mountModelId: J3_MODEL, distributed: null };

  it('J3: "Swapping", amber, the node it loads — never "MLX failed" or the exit-143 error', () => {
    const model = buildMlxTrayModel(STOPPED_BY_SWAP, { ...TRAY, swap: toSplit });
    expect(trayTitleText(model)).toBe('🟡 Swapping');
    expect(model.items[0]).toMatchObject({
      label: 'LeanZero MLX: swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
      phase: 'loading',
    });
    expect(JSON.stringify(model.items)).not.toContain('143');
  });

  it('negative control: a real failure keeps "MLX failed" and its error', () => {
    const model = buildMlxTrayModel(DIED, { ...TRAY, swap: ownLoad });
    expect(trayTitleText(model)).toBe('🔴 MLX failed');
    expect(JSON.stringify(model.items)).toContain(`Error: ${REAL_FAILURE}`.slice(0, 40));
  });
});

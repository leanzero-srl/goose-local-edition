import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  latestPlacementPlans,
  measuredFigure,
  mlxPlacementPlan,
  rememberPlacementPlans,
  resetPlacementPlansSeen,
  type SpeedFigure,
} from '../../acp/mlx-placement';
import { engineWayOf, measuredRunsOf } from './measuredRuns';
import { measuredPlan } from './placement.fixtures';
import { FLASH_MODEL, FLASH_SERVING } from './mlxDistributed.fixtures';

const mockExtMethod = vi.fn();
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));

const MODEL = 'ai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const STUDIO = 'worksmacstudio-lan-6a972f';
const LOCAL = { kind: 'single' as const, nodes: ['local'] };
const ON_STUDIO = { kind: 'single' as const, nodes: [`link:${STUDIO}`] };
const SPLIT = {
  kind: 'pipeline' as const,
  nodes: ['local', 'workhorse'],
  link: 'jaccl',
};

beforeEach(() => {
  resetPlacementPlansSeen();
  mockExtMethod.mockReset();
});

describe('measuredFigure — one run is a run, never a range (Q-123)', () => {
  const figure = (value: number, low: number, high: number, runs: number, measured = true) =>
    ({ estimate: { value, low, high }, measured, runs }) as SpeedFigure;

  it('one run: its value, no spread — the "29.6–29.6" the Run it card drew is not a range', () => {
    expect(measuredFigure(figure(29.6, 29.6, 29.6, 1))).toEqual({
      runs: 1,
      median: 29.6,
      spread: undefined,
      lastMeasuredMs: null,
    });
  });

  it('several runs: the median, and the slowest–fastest only when they differ', () => {
    expect(measuredFigure(figure(24.1, 22.0, 51.4, 3))?.spread).toEqual({ low: 22.0, high: 51.4 });
    expect(measuredFigure(figure(20, 20, 20, 4))?.spread).toBeUndefined();
  });

  it('an estimate is never a measurement', () => {
    expect(measuredFigure(figure(21.9, 20.8, 23.0, 0, false))).toBeNull();
    expect(measuredFigure(null)).toBeNull();
  });
});

describe('measuredRunsOf — the tile reads the way the engine runs, from goose’s plan', () => {
  const plan = measuredPlan(MODEL, [
    { key: LOCAL, decode: [12.0, 11.0, 13.0, 4], prefill: [210, 200, 220, 4] },
    { key: ON_STUDIO, decode: [29.6, 29.6, 29.6, 1] },
    { key: SPLIT },
  ]);

  it('this Mac, a linked Mac and the split each read their OWN runs', () => {
    rememberPlacementPlans([plan]);
    const seen = latestPlacementPlans();
    const local = measuredRunsOf(seen, engineWayOf(null, null, MODEL));
    expect(local).toMatchObject({ kind: 'read', writing: { median: 12.0, runs: 4 } });
    const studio = measuredRunsOf(
      seen,
      engineWayOf(null, { state: 'ready', peer: STUDIO, modelId: MODEL }, 'some-other-local-model')
    );
    expect(studio).toMatchObject({
      kind: 'read',
      writing: { median: 29.6, runs: 1 },
      reading: null,
    });
    rememberPlacementPlans([measuredPlan(FLASH_MODEL, [{ key: SPLIT, decode: [9.5, 9, 10, 2] }])]);
    const split = measuredRunsOf(latestPlacementPlans(), engineWayOf(FLASH_SERVING, null, MODEL));
    expect(split).toMatchObject({ kind: 'read', writing: { median: 9.5, runs: 2 } });
  });

  it('not answered yet is PENDING; a failed call, a plan error or a missing way says why', () => {
    const way = engineWayOf(null, null, MODEL);
    expect(measuredRunsOf(latestPlacementPlans(), way)).toEqual({ kind: 'pending' });
    rememberPlacementPlans([{ ...plan, error: 'models folder unreadable' }]);
    expect(measuredRunsOf(latestPlacementPlans(), way)).toEqual({
      kind: 'unread',
      detail: 'models folder unreadable',
    });
    expect(measuredRunsOf(latestPlacementPlans(), engineWayOf(null, null, null))).toEqual({
      kind: 'unread',
      detail: 'goose names no model for this engine',
    });
    rememberPlacementPlans([plan]);
    const split = measuredRunsOf(
      latestPlacementPlans(),
      engineWayOf({ ...FLASH_SERVING, runner: 'somethingNew', modelId: MODEL }, null, MODEL)
    );
    expect(split).toMatchObject({ kind: 'unread' });
    expect(split.kind === 'unread' && split.detail).toContain('runner "somethingNew"');
  });

  it('writing is read from any goal’s plan; reading only from the chat goal’s', () => {
    rememberPlacementPlans([{ ...plan, goal: 'longDocuments' }]);
    expect(measuredRunsOf(latestPlacementPlans(), engineWayOf(null, null, MODEL))).toMatchObject({
      kind: 'read',
      writing: { median: 12.0 },
      reading: null,
    });
  });
});

describe('mlxPlacementPlan — every answer lands where the tile reads it', () => {
  it('a plan goose answered is kept by goal and model; a failed call is named, then cleared', async () => {
    mockExtMethod.mockResolvedValueOnce({
      plans: [measuredPlan(MODEL, [{ key: LOCAL }])],
      nodes: [],
    });
    await mlxPlacementPlan('chat', MODEL);
    expect(latestPlacementPlans().plans.get(`chat\n${MODEL}`)?.modelId).toBe(MODEL);

    mockExtMethod.mockRejectedValueOnce(new Error('goosed is restarting'));
    await expect(mlxPlacementPlan('chat', 'other')).rejects.toThrow('goosed is restarting');
    expect(latestPlacementPlans().failure).toBe('goosed is restarting');
    expect(measuredRunsOf(latestPlacementPlans(), engineWayOf(null, null, 'other'))).toEqual({
      kind: 'unread',
      detail: 'goosed is restarting',
    });
    // The plan already kept is not lost to a later failure.
    expect(latestPlacementPlans().plans.get(`chat\n${MODEL}`)).toBeDefined();

    mockExtMethod.mockResolvedValueOnce({ plans: [], nodes: [] });
    await mlxPlacementPlan('chat');
    expect(latestPlacementPlans().failure).toBeNull();
  });
});

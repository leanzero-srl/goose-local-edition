import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const extMethod = vi.fn();
vi.mock('./acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: (...args: unknown[]) => extMethod(...args) }),
}));

import {
  PLACEMENT_PLAN_LIMIT_MS,
  latestPlacementPlans,
  mlxPlacementPlan,
  resetPlacementPlansSeen,
} from './mlx-placement';

beforeEach(() => {
  vi.useFakeTimers();
  extMethod.mockReset();
  resetPlacementPlansSeen();
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Run it's "Measuring your Macs and the model…" stood for hours on the owner's mule (3.0.132): the
 * plan read a network-mounted models folder that stalled, and the card waited for it forever. A plan
 * that does not answer in its limit is a failure in words — the card's error and refresh, and every
 * other reader of the plan (New node, the Nodes facts) sees `failed`, never an endless "reading".
 */
describe('a placement plan goose never answers', () => {
  it('fails in words after its limit and publishes the failure', async () => {
    extMethod.mockReturnValue(new Promise(() => undefined));
    const plan = mlxPlacementPlan('chat', 'mlx-community/Qwen3-30B-A3B-4bit');
    const outcome = plan.then(
      () => 'answered',
      (e: Error) => e.message
    );
    await vi.advanceTimersByTimeAsync(PLACEMENT_PLAN_LIMIT_MS - 1);
    expect(latestPlacementPlans().failure).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    const words = await outcome;
    expect(words).toContain('goose did not answer the placement plan in 45 s');
    expect(latestPlacementPlans().failure).toBe(words);
  });

  it('an answer inside the limit is the plan, as before', async () => {
    extMethod.mockResolvedValue({ plans: [], nodes: [], storeErrors: [], probeMs: 5404 });
    await expect(mlxPlacementPlan('chat')).resolves.toMatchObject({ probeMs: 5404 });
    expect(latestPlacementPlans().failure).toBeNull();
  });
});

import { describe, expect, it } from 'vitest';
import type { PlacementCandidate, PlacementPlan } from '../../acp/mlx-placement';
import { PLAN_27B } from './placement.fixtures';
import { waysOf } from './PlacementCandidates';

const candidates = PLAN_27B.candidates ?? [];
const tensor = candidates.find(
  (c) => c.id === 'tensor:jaccl:local+workhorse'
) as PlacementCandidate;
const pipeline = candidates.find(
  (c) => c.id === 'pipeline:jaccl:local+workhorse'
) as PlacementCandidate;

/**
 * Q-312 (after Q-25): under Run it's split Details, "1 other split — pipeline split · JACCL — not
 * supported yet: goose splits qwen3_5 tensor-parallel only" listed an option goose never runs. A
 * split goose cannot run is no option anywhere; one it CAN run but not right now stays, with its
 * reason, as reference.
 */
describe('waysOf — the other splits under Details', () => {
  it('the real 27B plan: the unsupported pipeline split is not listed', () => {
    expect(pipeline.supported).toBe(false);
    const { ways, otherSplits } = waysOf(PLAN_27B, [], true);
    expect(ways.map((w) => w.key)).toContain('tensor:jaccl:local+workhorse');
    expect(otherSplits).toEqual([]);
  });

  it('a supported split that is not the offered one stays, with its reason', () => {
    const ringSplit: PlacementCandidate = {
      ...tensor,
      id: 'tensor:ring:local+workhorse',
      key: { ...tensor.key, link: 'ring' },
      action: { kind: 'unavailable', reason: 'the ring link is down' },
    };
    const plan: PlacementPlan = { ...PLAN_27B, candidates: [...candidates, ringSplit] };
    const { ways, otherSplits } = waysOf(plan, [], true);
    expect(ways.filter((w) => w.kind === 'split').map((w) => w.key)).toEqual([
      'tensor:jaccl:local+workhorse',
    ]);
    expect(otherSplits.map((c) => c.id)).toEqual(['tensor:ring:local+workhorse']);
  });
});

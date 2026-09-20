import { describe, expect, it } from 'vitest';
import { sb8CompositionSchema } from './sb8ScoreSchema';
import oldVerdict from './components/benchmark/sb8-perfect.fixture.json';

const weights = { A: 0.08, B: 0.17, C: 0.3, D: 0.15, E: 0.05, F: 0.25 };
const core_tiers = ['A', 'B', 'C', 'D', 'F'];
const current = { tiers: { ...oldVerdict.tiers, F: 0 }, weights, core_tiers };

describe('recorded SB8 composition', () => {
  it('keeps historical A–E verdict weights and core tiers', () => {
    expect(sb8CompositionSchema(oldVerdict)).toEqual({
      weights: { A: 0.1, B: 0.2, C: 0.45, D: 0.2, E: 0.05 },
      coreTiers: ['A', 'B', 'C', 'D'],
    });
  });
  it('uses the recorded planning weight and includes F in excellence gating', () => {
    expect(sb8CompositionSchema(current)).toEqual({ weights, coreTiers: core_tiers });
  });
  it.each([
    { ...current, weights: undefined },
    { ...current, core_tiers: undefined },
    { ...current, tiers: oldVerdict.tiers },
    { ...current, weights: undefined, core_tiers: undefined },
    { ...current, weights: { ...weights, F: Number.NaN } },
    { ...current, core_tiers: ['A', 'missing'] },
  ])('refuses incomplete composition evidence %j', (verdict) => {
    expect(sb8CompositionSchema(verdict)).toBeNull();
  });
});

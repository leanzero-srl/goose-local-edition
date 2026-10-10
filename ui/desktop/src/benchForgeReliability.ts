import { forgeEra, type ForgeEra } from './components/benchmark/baselines';

/**
 * Forge 2.0's reliability rule as the scorer records it (score_forge2.py `reliability()`): final = what the
 * tests earned × critical multiplier × reliability, where each GROUP of tests (every tier except E)
 * multiplies the score by its worst test — reliability = max(floor, Π over the groups of
 * (1 − k × (1 − the group's worst counted score))). The app never computes any of it: the publish body and
 * the score view read `verdict.reliability`, and a verdict of a reliability era without that block is
 * stated as such — never read as "nothing failed".
 */
export const FORGE_RELIABILITY_ERAS: readonly ForgeEra[] = ['forge-2.0'];

export function forgeHasReliability(scorerVersion: string | undefined): boolean {
  const era = forgeEra(scorerVersion);
  return era !== null && FORGE_RELIABILITY_ERAS.includes(era);
}

/** A group whose worst test is below 1: the group's tier letter, that worst test, its score, and the
 *  factor the group multiplies the score by. */
export interface ForgeReliabilityDefect {
  tier: string;
  check: string;
  score: number;
  factor: number;
}

export interface ForgeReliabilityRecord {
  multiplier: number;
  k: number;
  floor: number;
  floored: boolean;
  /** One entry per group that multiplies the score. */
  defects: ForgeReliabilityDefect[];
  /** Per tier letter: the group's other failed tests, already paid for by its worst one. */
  folded: Record<string, string[]>;
  /** Tests whose critical defect fired: the critical multiplier prices them, so no group counts them. */
  priced_as_critical: string[];
}

const finite = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);
const strings = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every((item) => typeof item === 'string');

function readDefect(value: unknown): ForgeReliabilityDefect | null {
  if (!value || typeof value !== 'object') return null;
  const row = value as Record<string, unknown>;
  if (typeof row.tier !== 'string' || typeof row.check !== 'string') return null;
  if (!finite(row.score) || !finite(row.factor)) return null;
  return { tier: row.tier, check: row.check, score: row.score, factor: row.factor };
}

/** `verdict.reliability` exactly as score_forge2.py wrote it, or null when the block is absent or is not
 *  that shape. Nothing is filled in: a caller states the absence. */
export function readForgeReliability(value: unknown): ForgeReliabilityRecord | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const block = value as Record<string, unknown>;
  if (!finite(block.multiplier) || !finite(block.k) || !finite(block.floor)) return null;
  if (typeof block.floored !== 'boolean' || !Array.isArray(block.defects)) return null;
  const defects = block.defects.map(readDefect);
  if (defects.some((defect) => defect === null)) return null;
  const folded = block.folded;
  if (!folded || typeof folded !== 'object' || Array.isArray(folded)) return null;
  if (!Object.values(folded).every(strings) || !strings(block.priced_as_critical)) return null;
  return {
    multiplier: block.multiplier,
    k: block.k,
    floor: block.floor,
    floored: block.floored,
    defects: defects as ForgeReliabilityDefect[],
    folded: folded as Record<string, string[]>,
    priced_as_critical: block.priced_as_critical,
  };
}

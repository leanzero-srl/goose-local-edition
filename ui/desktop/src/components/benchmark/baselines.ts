/**
 * Baked baseline results. Captured on our fleet against the frozen spec and shipped as data, so a
 * user never pays to run frontier models and every board is directly comparable.
 *
 * Regenerated only when we ship: `evals/swarm-bench/bench/sweep.py` writes the verdicts,
 * and each row carries the scorer version it was measured by — rows from different versions are not
 * comparable and must not sit in one table.
 */

export type Tier = 'A' | 'B' | 'C' | 'D';

export interface BenchmarkRow {
  label: string;
  score: number; // 0..1 overall
  tiers: Record<Tier, number>; // 0..1 per tier
  nodes?: number;
  mine?: boolean;
  scorerVersion: string;
}

export const TIER_LABELS: Record<Tier, string> = {
  A: 'A structure',
  B: 'B behaviour',
  C: 'C vendor contract',
  D: 'D finesse',
};

export const TIER_WEIGHTS: Record<Tier, number> = { A: 0.25, B: 0.3, C: 0.25, D: 0.2 };

export const BASELINES: BenchmarkRow[] = [
  {
    label: 'Claude Opus 5',
    score: 0.98,
    tiers: { A: 1.0, B: 1.0, C: 1.0, D: 0.9 },
    scorerVersion: 'sb-3',
  },
  {
    label: 'Claude Sonnet 5',
    score: 0.92,
    tiers: { A: 1.0, B: 1.0, C: 0.86, D: 0.78 },
    scorerVersion: 'sb-3',
  },
  {
    label: 'Claude Haiku 4.5',
    score: 0.73,
    tiers: { A: 0.83, B: 0.85, C: 0.43, D: 0.8 },
    scorerVersion: 'sb-3',
  },
  {
    label: 'Local 27b, single agent',
    score: 0.841,
    tiers: { A: 0.83, B: 0.88, C: 0.86, D: 0.78 },
    scorerVersion: 'sb-3',
  },
];

/** Solid, saturated, and distinct — mirrors --color-node-* rather than tinting one hue. */
export const TIER_COLORS: Record<Tier, string> = {
  A: 'var(--color-node-1)',
  B: 'var(--color-node-2)',
  C: 'var(--color-node-4)',
  D: 'var(--color-node-5)',
};

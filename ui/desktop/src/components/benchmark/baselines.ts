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
  wallSecs?: number;
}

/**
 * The comparability rail. Rows measured by a different scorer are NOT comparable and must not sit
 * in the same chart — the view keeps a mismatched user row off the board and says why.
 */
export const COMPARABLE_SCORER = 'sb-5.2';

export const TIER_LABELS: Record<Tier, string> = {
  A: 'A structure',
  B: 'B behaviour',
  C: 'C vendor contract',
  D: 'D finesse',
};

export const TIER_WEIGHTS: Record<Tier, number> = { A: 0.25, B: 0.3, C: 0.25, D: 0.2 };

// The FROZEN sb-5.2 ladder (cloud, Bedrock, v2 spec) — the contract's baseline docs. Tier means and
// wall times come from the frozen verdicts (runs/build/{opus-5-r2,sonnet-5-r2,haiku-4.5-r2}); the
// overall scores are the contract's canonical 3-decimal figures.
export const BASELINES: BenchmarkRow[] = [
  {
    label: 'Claude Opus 5',
    score: 0.975,
    tiers: { A: 1.0, B: 0.9688, C: 1.0, D: 0.975 },
    scorerVersion: COMPARABLE_SCORER,
    wallSecs: 1170,
  },
  {
    label: 'Claude Sonnet 5',
    score: 0.969,
    tiers: { A: 1.0, B: 1.0, C: 1.0, D: 0.875 },
    scorerVersion: COMPARABLE_SCORER,
    wallSecs: 410,
  },
  {
    label: 'Claude Haiku 4.5',
    score: 0.786,
    tiers: { A: 0.8333, B: 0.8747, C: 0.5, D: 0.7875 },
    scorerVersion: COMPARABLE_SCORER,
    wallSecs: 318,
  },
];

/** Solid, saturated, and distinct — mirrors --color-node-* rather than tinting one hue. */
export const TIER_COLORS: Record<Tier, string> = {
  A: 'var(--color-node-1)',
  B: 'var(--color-node-2)',
  C: 'var(--color-node-4)',
  D: 'var(--color-node-5)',
};

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
 * in the same chart — the view keeps a mismatched user row off the board and says why. The app
 * offers two runnable tiers; each carries its own frozen scorer version and baseline set, and the
 * website's board keeps every era viewable behind its scorer selector.
 */
export type BenchTier = 'sb-5.3' | 'sb-6';
export const TIERS: BenchTier[] = ['sb-5.3', 'sb-6'];
export const DEFAULT_TIER: BenchTier = 'sb-5.3';
export const TIER_SCORER: Record<BenchTier, string> = { 'sb-5.3': 'sb-5.3', 'sb-6': 'sb-6.0' };
export const COMPARABLE_SCORER = TIER_SCORER[DEFAULT_TIER];

export const TIER_LABELS: Record<Tier, string> = {
  A: 'A structure',
  B: 'B behaviour',
  C: 'C vendor contract',
  D: 'D finesse',
};

export const TIER_WEIGHTS: Record<Tier, number> = { A: 0.25, B: 0.3, C: 0.25, D: 0.2 };

// The FROZEN baselines per tier, measured on this harness's own Bedrock entrant pipeline.
// sb-5.3 = the rendered-means-seen rescore of the archived cloud trees (median rep per model);
// sb-6.0 = the hard-tier calibration runs against the frozen thresholds file. The sb-5.2-era
// numbers live on the website's historic board, not here — the app only offers runnable tiers.
export const BASELINES_BY_TIER: Record<BenchTier, BenchmarkRow[]> = {
  'sb-5.3': [
    {
      // The stable rep (r0: identical across two probe passes; the rep spread is 0.889-0.960,
      // so treat any gap under ~0.05 to this number as within the baseline's own noise).
      label: 'Claude Opus 5',
      score: 0.9142,
      tiers: { A: 1.0, B: 1.0, C: 0.9, D: 0.875 },
      scorerVersion: 'sb-5.3',
      wallSecs: 1170,
    },
    {
      label: 'Claude Sonnet 5',
      score: 0.4971,
      tiers: { A: 1.0, B: 0.438, C: 0.3, D: 0.638 },
      scorerVersion: 'sb-5.3',
      wallSecs: 410,
    },
    {
      label: 'Claude Haiku 4.5',
      score: 0.4615,
      tiers: { A: 0.833, B: 0.365, C: 0.1, D: 0.625 },
      scorerVersion: 'sb-5.3',
      wallSecs: 318,
    },
  ],
  // sb-6 board (2026-08-19): serial hermetic re-scores at each tree's advertised vendor
  // port (version stays sb-6.0 until declared stable — fix provenance is F895-F897).
  // Luna/Terra are engine-kill floors (session cut by the compaction bug fixed in
  // 19b4ed6ef) — floors, not ceilings; disclosed on the website run pages.
  'sb-6': [
    {
      label: 'GPT-5.6 Sol',
      score: 0.869,
      tiers: { A: 1.0, B: 1.0, C: 1.0, D: 1.0 },
      scorerVersion: 'sb-6.0',
      wallSecs: 1890,
    },
    {
      label: 'Claude Opus 5',
      score: 0.8281,
      tiers: { A: 1.0, B: 1.0, C: 1.0, D: 1.0 },
      scorerVersion: 'sb-6.0',
      wallSecs: 2208,
    },
    {
      label: 'GPT-5.6 Luna',
      score: 0.7887,
      tiers: { A: 1.0, B: 1.0, C: 1.0, D: 1.0 },
      scorerVersion: 'sb-6.0',
      wallSecs: 237,
    },
    {
      label: 'Claude Sonnet 5',
      score: 0.7666,
      tiers: { A: 1.0, B: 1.0, C: 1.0, D: 0.86 },
      scorerVersion: 'sb-6.0',
      wallSecs: 1790,
    },
    {
      label: 'GPT-5.6 Terra',
      score: 0.7257,
      tiers: { A: 1.0, B: 1.0, C: 1.0, D: 1.0 },
      scorerVersion: 'sb-6.0',
      wallSecs: 390,
    },
    {
      label: 'Claude Haiku 4.5',
      score: 0.4597,
      tiers: { A: 0.8786, B: 0.9468, C: 0.8809, D: 0.86 },
      scorerVersion: 'sb-6.0',
      wallSecs: 463,
    },
  ],
};
export const BASELINES: BenchmarkRow[] = BASELINES_BY_TIER[DEFAULT_TIER];

/** Solid, saturated, and distinct — mirrors --color-node-* rather than tinting one hue. */
export const TIER_COLORS: Record<Tier, string> = {
  A: 'var(--color-node-1)',
  B: 'var(--color-node-2)',
  C: 'var(--color-node-4)',
  D: 'var(--color-node-5)',
};

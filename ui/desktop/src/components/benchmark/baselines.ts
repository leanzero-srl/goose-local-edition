/**
 * Shared benchmark vocabulary: the tier taxonomy, the scorer-version rail, and the row shape the
 * charts render. The BAKED baseline boards that used to live here are GONE (2026-08-31): every
 * comparison row is now RETRIEVED from the site's catalog (`benchmarkCatalog` IPC, typed in
 * ./bridge.ts) — a hardcoded score can silently outlive the board it came from, and the sb-7 row
 * baked here did exactly that. When the catalog is unreachable the view states the absence
 * loudly; it never invents rows.
 */

export type Tier = 'A' | 'B' | 'C' | 'D';

export interface BenchmarkRow {
  label: string;
  score: number; // 0..1 overall
  /** 0..1 per tier — absent on catalog baselines, which publish only the overall number. */
  tiers?: Partial<Record<Tier, number>>;
  nodes?: number;
  mine?: boolean;
  scorerVersion: string;
  wallSecs?: number;
}

/**
 * The runnable-tier vocabulary main.ts's spec/probe mapping is keyed by (benchTierPayload.ts).
 * Which benchmark the user can RUN is the catalog's `current` flag, not a choice made here —
 * the view offers no tier chooser.
 */
export type BenchTier = 'sb-5.3' | 'sb-6' | 'sb-7';
export const TIERS: BenchTier[] = ['sb-5.3', 'sb-6', 'sb-7'];
export const TIER_SCORER: Record<BenchTier, string> = {
  'sb-5.3': 'sb-5.3',
  'sb-6': 'sb-6.0',
  // sb-7 ships UNCALIBRATED — score_sb7.py reports itself as sb-7.0-rc and sb7-thresholds.json
  // carries "calibrated": false. The rc identity is kept so an rc number is never quietly
  // compared against a calibrated one.
  'sb-7': 'sb-7.0-rc',
};

export const TIER_LABELS: Record<Tier, string> = {
  A: 'A structure',
  B: 'B behaviour',
  C: 'C vendor contract',
  D: 'D finesse',
};

/** Solid, saturated, and distinct — mirrors --color-node-* rather than tinting one hue. */
export const TIER_COLORS: Record<Tier, string> = {
  A: 'var(--color-node-1)',
  B: 'var(--color-node-2)',
  C: 'var(--color-node-4)',
  D: 'var(--color-node-5)',
};

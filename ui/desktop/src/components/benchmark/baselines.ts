/**
 * Shared benchmark vocabulary: the tier taxonomy, the scorer-version rail, and the row shape the
 * charts render. The BAKED baseline boards that used to live here are GONE (2026-08-31): every
 * comparison row is now RETRIEVED from the site's catalog (`benchmarkCatalog` IPC, typed in
 * ./bridge.ts) — a hardcoded score can silently outlive the board it came from, and the sb-7 row
 * baked here did exactly that. When the catalog is unreachable the view states the absence
 * loudly; it never invents rows.
 */

export type Tier = 'A' | 'B' | 'C' | 'D' | 'E' | 'F';

export interface BenchmarkRow {
  label: string;
  score: number; // 0..1 overall
  /** 0..1 per tier letter (A–F, and the sb-7 family's J…M) — absent on catalog baselines, which
   *  publish only the overall number. */
  tiers?: Partial<Record<string, number>>;
  nodes?: number;
  mine?: boolean;
  scorerVersion: string;
  wallSecs?: number;
}

/**
 * The runnable-tier vocabulary main.ts's spec/probe mapping is keyed by (benchTierPayload.ts).
 * The stable default is shared with the launcher; historical scorer identities stay distinct.
 */
export type BenchTier = 'sb-5.3' | 'sb-6' | 'sb-7' | 'sb-7.1' | 'sb-7.2' | 'sb-8';
export const TIERS: BenchTier[] = ['sb-5.3', 'sb-6', 'sb-7', 'sb-7.1', 'sb-7.2', 'sb-8'];
export const TIER_SCORER: Record<BenchTier, string> = {
  'sb-5.3': 'sb-5.3',
  'sb-6': 'sb-6.0',
  // sb-7 ships UNCALIBRATED — score_sb7.py reports itself as sb-7.0-rc and sb7-thresholds.json
  // carries "calibrated": false. The rc identity is kept so an rc number is never quietly
  // compared against a calibrated one.
  'sb-7': 'sb-7.0-rc',
  'sb-7.1': 'sb-7.1',
  'sb-7.2': 'sb-7.2',
  'sb-8': 'sb-8.0-rc',
};

/**
 * The payments family that runs on the ISOLATED runtime: the optional Python/Node/video download,
 * the public starter, one graded browser recording per published result, and the completed-build
 * receipt that makes a scoring retry possible. SB7.1 introduced that path and SB7.2 shares all of
 * it; every site that used to test `startsWith('sb-7.1')` asks this list instead, so the next
 * payments tier is one entry here rather than a grep across main, the scorer UI and the publisher.
 */
export const ISOLATED_PAYMENTS_TIERS = ['sb-7.1', 'sb-7.2'] as const satisfies readonly BenchTier[];
export type IsolatedPaymentsTier = (typeof ISOLATED_PAYMENTS_TIERS)[number];

/** The isolated payments tier a recorded scorer version belongs to (`sb-7.1`, `sb-7.1-rc`, `sb-7.2`…). */
export function isolatedPaymentsTier(
  scorerVersion: string | undefined
): IsolatedPaymentsTier | null {
  if (!scorerVersion) return null;
  return (
    ISOLATED_PAYMENTS_TIERS.find((tier) => {
      const scorer = TIER_SCORER[tier];
      return (
        scorerVersion === scorer ||
        scorerVersion.startsWith(`${scorer}-`) ||
        scorerVersion.startsWith(`${scorer}.`)
      );
    }) ?? null
  );
}

export const isIsolatedPaymentsScorer = (scorerVersion: string | undefined) =>
  isolatedPaymentsTier(scorerVersion) !== null;

/** Every tier letter a verdict can carry, in the order the scorer reports them. */
export const VERDICT_TIER_ORDER = [
  'A',
  'B',
  'C',
  'D',
  'E',
  'F',
  'J',
  'V',
  'P',
  'T',
  'X',
  'R',
  'S',
  'Q',
  'M',
] as const;

export const VERDICT_TIER_INFO: Record<string, { name: string; desc: string }> = {
  A: { name: 'Structure', desc: 'The files and structure the spec names' },
  B: { name: 'Behaviour', desc: 'Does the app DO what the spec says — probed by running it' },
  C: {
    name: 'Vendor contract',
    desc: 'The vendor API contract — sync, idempotency, conditional fetch',
  },
  D: { name: 'Finesse', desc: 'Formats, edge cases, polish' },
  J: { name: 'Journey', desc: 'The user journey in a real browser' },
  V: { name: 'Visual', desc: 'Visual/design quality of the served page' },
  P: { name: 'Performance', desc: 'Measured performance budgets' },
  T: { name: 'Transactions', desc: 'Cross-service transaction correctness' },
  X: { name: 'Concurrency', desc: 'Concurrent requests and consistent reads' },
  R: { name: 'Recovery', desc: 'Restart and failure recovery' },
  E: { name: 'Excellence', desc: 'Scorer-recorded excellence checks' },
  S: { name: '3D structure', desc: 'Required payment scene geometry and mapping' },
  Q: { name: 'Presentation', desc: 'Readable hierarchy and interaction' },
  M: { name: 'Animation', desc: 'Motion grounded in payment state transitions' },
};

export const TIER_LABELS: Record<Tier, string> = {
  A: 'A structure',
  B: 'B behaviour',
  C: 'C vendor contract',
  D: 'D finesse',
  E: 'E excellence',
  F: 'F route planning',
};

export const isSb8 = (version: string) => /^sb-8(?:\.|$)/.test(version);

export const SB8_TIERS: Record<string, { name: string; desc: string }> = {
  A: { name: 'Backend foundation', desc: 'Seeded scene, movement and durable state' },
  B: {
    name: 'Transactional correctness',
    desc: 'Geometry, atomic commands and receipt persistence',
  },
  C: {
    name: '3D scene',
    desc: 'Observed WebGL geometry and backend-driven movement',
  },
  D: { name: 'Interaction', desc: 'Picking, controls and live state in the browser' },
  E: { name: 'Excellence', desc: 'Clean console, gated by core correctness' },
  F: { name: 'Route planning', desc: 'Optimal collision-free routes without changing live state' },
};

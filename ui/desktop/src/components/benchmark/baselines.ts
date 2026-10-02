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
export type BenchTier = 'sb-5.3' | 'sb-6' | 'sb-7' | 'sb-7.1' | 'sb-7.2' | 'sb-8' | 'forge-1.0';
export const TIERS: BenchTier[] = ['sb-5.3', 'sb-6', 'sb-7', 'sb-7.1', 'sb-7.2', 'sb-8', 'forge-1.0'];
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
  // The era this app bundles (forge/release-manifest.json `scorerVersion`). Until the freeze pins
  // forge-thresholds.json, score_forge.py reports its verdicts as forge-1.0-rc — that identity is
  // recorded on the result and refused at publish, never rewritten to the era's.
  'forge-1.0': 'forge-1.0',
};

/**
 * The benchmark FAMILIES (evals/swarm-bench/forge/INTEGRATION.md): SB (the payments/VendorSync eras)
 * and Forge (forge-1.0 first). Each family has its own current era on leanzero.net; nothing in one
 * changes the other's current/frozen state.
 */
export type BenchFamily = 'sb' | 'forge';
export const BENCH_FAMILIES: readonly BenchFamily[] = ['sb', 'forge'];

export const isBenchFamily = (value: unknown): value is BenchFamily =>
  value === 'sb' || value === 'forge';

/** The family a recorded scorer version belongs to: Forge's scorers are `forge-*`, every other era is SB. */
export const familyOfScorer = (scorerVersion: string | undefined): BenchFamily =>
  /^forge-/.test(scorerVersion ?? '') ? 'forge' : 'sb';

/**
 * A catalog entry's family. The site states it (`family`); an entry without one is SB by the contract
 * (apps ≤ 3.0.88 read no family) — unless its scorer version is a Forge scorer, which no SB era can be.
 */
export const catalogFamily = (entry: { family?: unknown; scorerVersion?: string }): BenchFamily =>
  isBenchFamily(entry.family) ? entry.family : familyOfScorer(entry.scorerVersion);

export const isForge = (scorerVersion: string | undefined) => familyOfScorer(scorerVersion) === 'forge';

/** score_forge.py's TIER_ORDER: nine weighted tiers and the E excellence slice. */
export const FORGE_TIER_ORDER = ['L', 'K', 'T', 'R', 'S', 'B', 'U', 'V', 'A', 'E'] as const;

/** The Forge tiers (forge/DESIGN.md §8.1–§8.2). The letters overlap SB's; the meanings do not. */
export const FORGE_TIERS: Record<string, { name: string; desc: string }> = {
  L: { name: 'Lint', desc: 'Forge lint clean, every function bundles and loads' },
  K: { name: 'Platform currency', desc: 'The newest modules and APIs: dashboards widget, Rovo skill' },
  T: { name: 'Event pipeline', desc: 'Trigger → queue → consumer: duplicates, order, retries' },
  R: { name: 'Reconcile', desc: 'Scheduled backfill and heal: pagination, rate limits' },
  S: { name: 'Storage', desc: 'KVS custom entities, indexes and scope' },
  B: { name: 'Resolvers', desc: 'Backend resolvers: permissions and exactly-once side effects' },
  U: { name: 'UI function', desc: 'The widget, its edit view and the sprint action, driven in a browser' },
  V: { name: 'Visual', desc: 'Theme tokens, dark mode, CSP and console' },
  A: { name: 'Rovo', desc: 'The Rovo action and agent answer with the right numbers' },
  E: { name: 'Excellence', desc: 'Economy and polish, gated by the core' },
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

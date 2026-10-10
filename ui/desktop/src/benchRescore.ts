import type { BenchSessionRow } from './benchSessions';
import {
  FORGE_ERAS,
  ISOLATED_PAYMENTS_TIERS,
  TIER_SCORER,
  eraDisplayName,
  familyOfScorer,
} from './components/benchmark/baselines';

/** Exactly the scorer identities a completed-build receipt can carry — never an rc or a sibling. The
 *  isolated payments tiers and every Forge era (bench_rescore.py replays the receipt's own era scorer from
 *  its own seed). Whether THIS app may re-grade an era is the launch gate's call (history only). */
const RESCORABLE: readonly string[] = [
  ...ISOLATED_PAYMENTS_TIERS.map((tier) => TIER_SCORER[tier]),
  ...FORGE_ERAS.map((era) => TIER_SCORER[era]),
];
const RESCORABLE_WORDS = RESCORABLE.map(eraDisplayName).reduce(
  (words, name, i, all) =>
    i === 0 ? name : `${words}${i === all.length - 1 ? ' and ' : ', '}${name}`,
  ''
);

export interface BuildCompletionReceipt {
  schemaVersion: 1;
  /** One of RESCORABLE, equal to the session row's own scorer. */
  scorerVersion: string;
  runId: string;
  startedAt: string;
  fixture_seed: string;
  vendor_port: number;
  provider: string | null;
  model: string | null;
  agent: { exit: number; timed_out: boolean; secs: number; usage?: unknown };
  sourceInventory: Record<string, string>;
  contracts: Record<string, string>;
}

export function retryScoringEligibility(
  row: BenchSessionRow,
  value: unknown,
  /** A FINISHED Forge run whose surfaces were graded but whose graded clip could not be verified — a
   *  scoring problem the saved build can fix (the site refuses it clip-less). Its row carries the rc
   *  identity its era's scorer reported; the receipt names the tier (forge-2.0). */
  forgeClipMissing = false
): { ready: boolean; reason?: string } {
  const rescoring =
    forgeClipMissing && row.outcome === 'finished' && familyOfScorer(row.scorerVersion) === 'forge';
  const version = rescoring ? row.scorerVersion.replace(/-rc$/, '') : row.scorerVersion;
  if (!RESCORABLE.includes(version))
    return {
      ready: false,
      reason: `Only ${RESCORABLE_WORDS} runs can be rescored; this run is ${eraDisplayName(row.scorerVersion)}.`,
    };
  if (row.outcome !== 'did_not_finish' && !rescoring)
    return { ready: false, reason: 'This session is not awaiting a scoring retry.' };
  const problem = receiptProblem(row, version, value);
  return problem ? { ready: false, reason: problem } : { ready: true };
}

/** Why this receipt does not prove THIS session's build completed with its scoring inputs intact, or
 *  null when it does. `version` is the tier the receipt must name (an rc row's base tier for Forge). */
function receiptProblem(row: BenchSessionRow, version: string, value: unknown): string | null {
  if (!value || typeof value !== 'object')
    return 'No completed-build receipt was recorded for this run.';
  const receipt = value as Partial<BuildCompletionReceipt>;
  if (
    receipt.schemaVersion !== 1 ||
    receipt.scorerVersion !== version ||
    receipt.runId !== row.runId ||
    receipt.startedAt !== row.startedAt ||
    receipt.agent?.exit !== 0 ||
    receipt.agent.timed_out !== false ||
    !Number.isFinite(receipt.agent.secs) ||
    receipt.agent.secs < 0
  )
    return 'The saved receipt does not prove this build completed.';
  if (
    !receipt.sourceInventory ||
    !receipt.contracts ||
    !/^[a-f0-9]{16}$/i.test(receipt.fixture_seed ?? '') ||
    !Number.isInteger(receipt.vendor_port) ||
    receipt.vendor_port! < 1 ||
    receipt.vendor_port! > 65535
  )
    return 'The original scoring inputs are incomplete.';
  return null;
}

/**
 * Whether a FINISHED run's saved build can be re-graded by this app's scorer (bench_rescore.py, no model
 * calls). The receipt is the same proof a scoring retry needs; the outcome is the opposite one — the run
 * already carries a score, which the re-score replaces only when it succeeds.
 */
export function rescoreEligibility(
  row: BenchSessionRow,
  value: unknown
): { ready: boolean; reason?: string } {
  if (row.outcome === 'running')
    return { ready: false, reason: 'This run is in progress — re-score it once it has ended.' };
  if (row.outcome !== 'finished')
    return { ready: false, reason: 'Only a finished run with a score can be re-scored.' };
  // A Forge scorer reports its era's rc (`forge-2.0-rc`) until its thresholds freeze; the receipt names the tier.
  const version =
    familyOfScorer(row.scorerVersion) === 'forge'
      ? row.scorerVersion.replace(/-rc$/, '')
      : row.scorerVersion;
  if (!RESCORABLE.includes(version))
    return {
      ready: false,
      reason: `Only ${RESCORABLE_WORDS} runs can be re-scored; this run is ${eraDisplayName(row.scorerVersion)}.`,
    };
  const problem = receiptProblem(row, version, value);
  return problem ? { ready: false, reason: problem } : { ready: true };
}

/** A stored result row's run meta as recorded by main (buildBenchResultRow). */
export interface StoredRunMeta {
  startedAt: string;
  finishedAt: string;
  engineEvents: number;
  repairRounds: number;
  /** Present when this result came from re-grading a run that already had one. */
  rescoredAt?: string;
}

/**
 * The publish envelope's runMeta, key by key for BOTH families (the site's allowlist refuses unknown
 * keys). `buildId` is the run's own id — the cloud-<uuid> / engine id the session row carries — so the
 * site recognises a re-score of a build it already shows and replaces that entry; `rescoredAt` says
 * the posted result is a re-grade. A row without a run id carries no buildId: the site then matches on
 * startedAt, as it did before.
 */
export function publishRunMeta(stored: {
  runMeta?: unknown;
  runId?: unknown;
}): (StoredRunMeta & { buildId?: string }) | null {
  const meta = stored.runMeta as StoredRunMeta | undefined;
  if (!meta || typeof meta !== 'object') return null;
  return {
    startedAt: meta.startedAt,
    finishedAt: meta.finishedAt,
    engineEvents: meta.engineEvents,
    repairRounds: meta.repairRounds,
    ...(typeof stored.runId === 'string' && stored.runId ? { buildId: stored.runId } : {}),
    ...(typeof meta.rescoredAt === 'string' ? { rescoredAt: meta.rescoredAt } : {}),
  };
}

/**
 * Whether the board already shows THIS result: posted, and not re-scored since. A re-score after the
 * post (or of a run only matched on the board, whose post time is unknown) makes the run publishable
 * again, so the new result can replace the old entry.
 */
export function publishedIsCurrent(
  published: { publishedAt: string | null } | null | undefined,
  rescoredAt: string | null | undefined
): boolean {
  if (!published) return false;
  if (!rescoredAt) return true;
  if (!published.publishedAt) return false;
  return Date.parse(published.publishedAt) >= Date.parse(rescoredAt);
}

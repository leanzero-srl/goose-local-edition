import type { BenchSessionRow } from './benchSessions';
import {
  ISOLATED_PAYMENTS_TIERS,
  TIER_SCORER,
  eraDisplayName,
} from './components/benchmark/baselines';

/** Exactly the scorer identities a completed-build receipt can carry — never an rc or a sibling. The
 *  isolated payments tiers and Forge (bench_rescore.py replays score_forge from the receipt's own seed). */
const RESCORABLE: readonly string[] = [
  ...ISOLATED_PAYMENTS_TIERS.map((tier) => TIER_SCORER[tier]),
  TIER_SCORER['forge-1.0'],
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
  value: unknown
): { ready: boolean; reason?: string } {
  if (!RESCORABLE.includes(row.scorerVersion))
    return {
      ready: false,
      reason: `Only ${RESCORABLE_WORDS} runs can be rescored; this run is ${eraDisplayName(row.scorerVersion)}.`,
    };
  if (row.outcome !== 'did_not_finish')
    return { ready: false, reason: 'This session is not awaiting a scoring retry.' };
  if (!value || typeof value !== 'object')
    return { ready: false, reason: 'No completed-build receipt was recorded for this run.' };
  const receipt = value as Partial<BuildCompletionReceipt>;
  if (
    receipt.schemaVersion !== 1 ||
    receipt.scorerVersion !== row.scorerVersion ||
    receipt.runId !== row.runId ||
    receipt.startedAt !== row.startedAt ||
    receipt.agent?.exit !== 0 ||
    receipt.agent.timed_out !== false ||
    !Number.isFinite(receipt.agent.secs) ||
    receipt.agent.secs < 0
  )
    return { ready: false, reason: 'The saved receipt does not prove this build completed.' };
  if (
    !receipt.sourceInventory ||
    !receipt.contracts ||
    !/^[a-f0-9]{16}$/i.test(receipt.fixture_seed ?? '') ||
    !Number.isInteger(receipt.vendor_port) ||
    receipt.vendor_port! < 1 ||
    receipt.vendor_port! > 65535
  )
    return { ready: false, reason: 'The original scoring inputs are incomplete.' };
  return { ready: true };
}

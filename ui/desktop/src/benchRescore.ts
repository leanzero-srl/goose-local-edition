import type { BenchSessionRow } from './benchSessions';

export interface BuildCompletionReceipt {
  schemaVersion: 1;
  scorerVersion: 'sb-7.1';
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
  if (row.scorerVersion !== 'sb-7.1')
    return {
      ready: false,
      reason: `Only SB7.1 runs can be rescored; this run is ${row.scorerVersion}.`,
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

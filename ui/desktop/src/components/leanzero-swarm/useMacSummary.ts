import { useSyncExternalStore } from 'react';
import {
  latestMlxDistributedStatus,
  subscribeMlxDistributedStatus,
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import type { Mac } from './macs';
import { summarizeMac, type MacSummary } from './macSummary';
import type { MacFacts } from './useMacs';
import { useMacs } from './useMacs';

/**
 * What a Mac serves, gathered the ONE way every reader gathers it (Q-149): the Mac's engine facts,
 * and — for this Mac — the split it supervises, from the latest distributed read any surface took
 * (acp/mlx-distributed.ts). My Macs, the Models table, Sampling and the tray's Mac lines all read
 * `summarizeMac` through this, so a Mac serving a split never reads "No model loaded" / "On disk" /
 * "no model mounted" on one surface while another says "Split" (3.0.52 live round).
 */
export function macSummaryInput(
  mac: Pick<Mac, 'isSelf'>,
  facts: Pick<MacFacts, 'status' | 'statusError' | 'activity' | 'decodeTps'>,
  distributed: MlxDistributedStatus | null
) {
  return {
    status: facts.status,
    statusError: facts.statusError,
    activity: facts.activity,
    decodeTps: facts.decodeTps,
    distributed: mac.isSelf ? distributed : null,
  };
}

export function useMacSummary(mac: Mac): MacSummary {
  const ctx = useMacs();
  const distributed = useSyncExternalStore(
    subscribeMlxDistributedStatus,
    latestMlxDistributedStatus
  );
  return summarizeMac(mac, macSummaryInput(mac, ctx.factsOf(mac.key), distributed));
}

/**
 * What the Mac does with ONE model, from its summary: serving it split across Macs, holding part of
 * another Mac's split, loaded or loading on its own engine — null when that model only sits on disk.
 */
export type ModelRole = 'split' | 'hosting' | 'loaded' | 'loading';

export function modelRoleOn(summary: MacSummary, modelId: string): ModelRole | null {
  if (summary.modelId !== modelId) return null;
  switch (summary.state) {
    case 'split':
      return 'split';
    case 'hosting':
      return 'hosting';
    case 'loading':
      return 'loading';
    case 'idle':
    case 'reading':
    case 'writing':
    case 'held':
      return 'loaded';
    default:
      return null;
  }
}

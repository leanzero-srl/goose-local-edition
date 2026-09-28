import type { ProviderDetails } from '../../types/providers';
import {
  isLocalEditionCloudProvider,
  isUserEndpoint,
} from '../settings/models/leanzeroSelectorPolicy';

/**
 * THE rule for what a provider row is on the Cloud Providers tab — one home, read by the row's chip,
 * by which list the row renders in, and by the header count. The count once used `is_configured`
 * while the Configured list used `is_configured || connection_error`, so a provider whose check
 * failed (a stale key answering 401) sat under CONFIGURED while the header said "0 of 14 configured".
 *
 * - `failed`: the last connection check returned an error — shown verbatim on the row.
 * - `connected`: the check ran this session and passed (a model ran with the saved settings).
 * - `unchecked`: settings are saved but no check has run since the app started.
 * - `not-set-up`: nothing saved. Never an OpenAI-compatible endpoint: its row exists only because
 *   the person saved it, so it is at worst `unchecked`.
 *
 * Every state but `not-set-up` renders under CONFIGURED and counts as configured.
 */
export type ProviderRowState = 'failed' | 'connected' | 'unchecked' | 'not-set-up';

type RowFacts = Pick<
  ProviderDetails,
  | 'name'
  | 'provider_type'
  | 'is_configured'
  | 'credentials_saved'
  | 'connection_checked'
  | 'connection_error'
>;

export function providerRowState(provider: RowFacts): ProviderRowState {
  if (provider.connection_error) return 'failed';
  if (provider.connection_checked) return 'connected';
  if (provider.is_configured || provider.credentials_saved || isUserEndpoint(provider)) {
    return 'unchecked';
  }
  return 'not-set-up';
}

/**
 * Which Cloud Providers list a provider belongs to: `endpoint` (an OpenAI-compatible server the
 * person added), `cloud` (one of the key-based cloud providers the tab offers), or null — a local
 * engine (oMLX, Goose Swarm, LM Studio), which the tab never lists and no count may include (Q-429:
 * New node's "A cloud model · 2 set up" counted oMLX and Goose Swarm while the tab said 0 of 14).
 */
export type ProviderListKind = 'cloud' | 'endpoint';

export function providerListKind(
  provider: Pick<ProviderDetails, 'name' | 'provider_type'>
): ProviderListKind | null {
  if (isUserEndpoint(provider)) return 'endpoint';
  if (isLocalEditionCloudProvider(provider.name)) return 'cloud';
  return null;
}

export function isListedConfigured(provider: RowFacts): boolean {
  return providerRowState(provider) !== 'not-set-up';
}

/** The two lists the tab renders, and the header count derived from exactly those lists. */
export function partitionProviderRows<P extends RowFacts>(
  rows: readonly P[]
): { configured: P[]; available: P[]; count: { configured: number; total: number } } {
  const configured = rows.filter(isListedConfigured);
  const available = rows.filter((row) => !isListedConfigured(row));
  return {
    configured,
    available,
    count: { configured: configured.length, total: configured.length + available.length },
  };
}

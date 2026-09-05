/**
 * Goose Swarm (local edition) provider POLICY — presentation only, nothing is unregistered.
 *
 * Owner's rule (2026-09-05): "in the providers, we can only ever get the providers we have
 * defined — which is only the defined cloud ones and Swarm. If you choose Swarm you tap into the
 * nodes automatically." So in the local edition every provider picker (the chat selector, the
 * onboarding selector, the credentials grids) offers EXACTLY `LOCAL_EDITION_PROVIDER_IDS` — an
 * allow-list of registry ids derived from the swarm's CLOUD_PROVIDERS table plus the Goose Swarm
 * provider — and nothing else: no omlx/lmstudio/ollama rows, no upstream catalog, no
 * "other provider" escape. The standard edition is untouched.
 */
import { SWARM_PROVIDER_ID } from '../../../branding';
import {
  LOCAL_EDITION_CLOUD_PROVIDER_IDS,
  LOCAL_EDITION_PROVIDER_IDS,
} from '../../leanzero-swarm/cloudProviders';
import type { Edition } from '../../../contexts/EditionContext';

/** The provider id the MLX engine serves chat through (sessions on it still exist; the bottom bar
 *  keeps them truthful — the picker never offers it). */
export const MLX_PROVIDER_ID = 'omlx';

/** The selector label for the engine entry (public naming; do not derive from provider metadata). */
export const MLX_ENTRY_LABEL = 'Leanzero MLX';

/** The Goose Swarm provider's two model ids: `swarm` chats on an idle node of the pool (default);
 *  `swarm-build` plans and fans out a build from the brief. */
export const SWARM_CHAT_MODEL_ID = 'swarm';
export const SWARM_BUILD_MODEL_ID = 'swarm-build';

/**
 * Mirrors `LOCAL_PROVIDER_FRAGMENTS` in crates/goose-cli/src/edition.rs, plus the exact
 * built-in `local` inference provider (which the fragment list does not match by design).
 * Used ONLY for edition/brand derivation (mainBrand.ts) — never to decide what a picker shows.
 */
const LOCAL_PROVIDER_FRAGMENTS = ['lmstudio', 'ollama', 'swarm', 'llama', 'localai', 'mlx'];

export function isLocalProviderName(name: string): boolean {
  const p = name.toLowerCase();
  return p === 'local' || LOCAL_PROVIDER_FRAGMENTS.some((frag) => p.includes(frag));
}

/** Exact allow-list test on the registry id: the four swarm cloud families + Goose Swarm. */
export function keepProviderInLocalEdition(registryId: string): boolean {
  return LOCAL_EDITION_PROVIDER_IDS.includes(registryId);
}

/** The credential-bearing subset of the allow-list (Swarm needs no key). */
export function isLocalEditionCloudProvider(registryId: string): boolean {
  return LOCAL_EDITION_CLOUD_PROVIDER_IDS.includes(registryId);
}

/** The providers a migrated install may still carry as its ACTIVE provider: the MLX sidecar and the
 *  LM Studio fleet provider. Both reach the same engines the Goose Swarm provider reaches through
 *  the pool, so switching the active provider to Swarm loses nothing. */
export const LEGACY_LOCAL_PROVIDER_IDS: readonly string[] = [MLX_PROVIDER_ID, 'lmstudio'];

/**
 * One-time startup migration decision: in the local edition, an active provider that is omlx or
 * lmstudio becomes Goose Swarm (`swarm` / model `swarm`). Any other edition or provider: null.
 */
export function legacyProviderMigration(
  edition: Edition,
  activeProvider: string | null | undefined
): { provider: string; model: string } | null {
  if (edition !== 'local' || !activeProvider) return null;
  if (!LEGACY_LOCAL_PROVIDER_IDS.includes(activeProvider)) return null;
  return { provider: SWARM_PROVIDER_ID, model: SWARM_CHAT_MODEL_ID };
}

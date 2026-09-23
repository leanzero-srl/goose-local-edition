/** Provider visibility follows the engine registry; local engines are selected through Swarm. */
import { SWARM_PROVIDER_ID } from '../../../branding';
import { providerIsLocal, type Edition } from '../../../contexts/EditionContext';

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
 * A local backend by name: the edition's fragment list (`providerIsLocal`, which mirrors
 * `LOCAL_PROVIDER_FRAGMENTS` in crates/goose-cli/src/edition.rs — ONE list, never a copy) plus the
 * exact built-in `local` inference provider, which the fragment list does not match by design.
 */
export function isLocalProviderName(name: string): boolean {
  return name.toLowerCase() === 'local' || providerIsLocal(name);
}

/** Only the supported key-based cloud providers appear in the Goose Swarm edition. */
export function keepProviderInLocalEdition(registryId: string): boolean {
  return registryId === SWARM_PROVIDER_ID || isLocalEditionCloudProvider(registryId);
}

export const CLOUD_PROVIDER_LABELS: Readonly<Record<string, string>> = {
  aws_bedrock: 'Amazon Bedrock',
  azure_openai: 'Azure Foundry',
  openai: 'OpenAI',
  anthropic: 'Claude',
  google: 'Gemini',
  alibaba: 'Qwen',
  openrouter: 'OpenRouter',
  ollama_cloud: 'Ollama Cloud',
  minimax: 'MiniMax',
  mistral: 'Mistral AI',
  zai: 'Z.AI',
  xai: 'xAI',
  moonshot: 'Moonshot',
  custom_deepseek: 'DeepSeek',
};

export function isLocalEditionCloudProvider(registryId: string): boolean {
  return Object.prototype.hasOwnProperty.call(CLOUD_PROVIDER_LABELS, registryId);
}

/** An endpoint the person added themselves — an OpenAI-compatible server from Cloud Providers. The
 *  registry says so by type (`Custom`: a file in the custom-provider store), never by a name pattern:
 *  a bundled declarative provider can carry a `custom_` id too (DeepSeek does). Chat may select one;
 *  a swarm node cannot (the engine's CLOUD_DEFS are a fixed roster). */
export function isUserEndpoint(provider: { name: string; provider_type: string }): boolean {
  return provider.provider_type === 'Custom' && !isLocalEditionCloudProvider(provider.name);
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

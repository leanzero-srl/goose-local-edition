/**
 * Leanzero edition UI POLICY for the chat model selector — presentation only.
 *
 * When the agent advertises the `mlxEngine` capability, the selector lists the cloud
 * providers PLUS a single "Leanzero MLX" entry for the in-house engine, and HIDES every
 * other local provider entry (ollama, lmstudio, localai, llama-cpp variants, swarm, and the
 * built-in `local` inference provider). Nothing is removed from code: the providers stay
 * registered, configured, and reachable — this predicate only decides what the selector
 * SHOWS in this edition. Capability absent -> no filtering, the selector is exactly as before.
 */

/** The provider id the MLX engine serves chat through. */
export const MLX_PROVIDER_ID = 'omlx';

/** The selector label for the engine entry (public naming; do not derive from provider metadata). */
export const MLX_ENTRY_LABEL = 'Leanzero MLX';

/**
 * Mirrors `LOCAL_PROVIDER_FRAGMENTS` in crates/goose-cli/src/edition.rs, plus the exact
 * built-in `local` inference provider (which the fragment list does not match by design).
 */
const LOCAL_PROVIDER_FRAGMENTS = ['lmstudio', 'ollama', 'swarm', 'llama', 'localai', 'mlx'];

export function isLocalProviderName(name: string): boolean {
  const p = name.toLowerCase();
  return p === 'local' || LOCAL_PROVIDER_FRAGMENTS.some((frag) => p.includes(frag));
}

/**
 * True when the provider should appear as a plain row in the Leanzero selector: every
 * cloud provider passes; local providers are hidden. The MLX engine itself is NOT a plain
 * row — it gets the dedicated "Leanzero MLX" entry — so it is excluded here too.
 */
export function keepProviderInLeanzeroSelector(name: string): boolean {
  return !isLocalProviderName(name);
}

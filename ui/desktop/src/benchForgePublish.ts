import { FORGE_TIER_ORDER } from './components/benchmark/baselines';

/**
 * The Forge half of benchmark-publish (forge/INTEGRATION.md: same routes, same envelope, scorerVersion
 * forge-1.0, tier letters L K T R S B U V A + E). The server validates per family and stays the authority;
 * these refuse locally what score_forge.py itself marks as not board-grade, in its own words.
 */
export function forgePublishProblem(stored: {
  scorerVersion?: unknown;
  verdict?: unknown;
}): string | null {
  const version = typeof stored.scorerVersion === 'string' ? stored.scorerVersion : '';
  const verdict = (stored.verdict ?? {}) as {
    status?: unknown;
    publishable?: unknown;
    unpublishable_reasons?: unknown;
    harness_missing?: unknown;
  };
  if (verdict.status === 'held') {
    const missing = Array.isArray(verdict.harness_missing) ? verdict.harness_missing.length : 0;
    return `This Forge result is held: the emulator met ${missing || 'some'} call${missing === 1 ? '' : 's'} it does not model. It is rescored once they are modelled, never published as is.`;
  }
  if (verdict.publishable === false) {
    const reasons = Array.isArray(verdict.unpublishable_reasons)
      ? verdict.unpublishable_reasons.map(String).filter(Boolean)
      : [];
    return `The scorer marked this Forge result unpublishable${reasons.length ? `: ${reasons.join('; ')}` : ''}.`;
  }
  if (verdict.publishable !== true)
    return 'This Forge result carries no publishability verdict from its scorer — run the benchmark again to publish.';
  if (/-rc$/.test(version))
    return `Scored by ${version}: the Forge thresholds are not frozen yet, so the result is not board-grade. Forge results publish once the forge-1.0 freeze pins them.`;
  return null;
}

/** Every Forge tier letter with its mean, an unrecorded tier as 0 — the SB publisher's A–D rule, per family. */
export function forgePublishTiers(tiers: Record<string, unknown>): Record<string, number> {
  return Object.fromEntries(
    FORGE_TIER_ORDER.map((tier) => [tier, typeof tiers[tier] === 'number' ? tiers[tier] : 0])
  );
}

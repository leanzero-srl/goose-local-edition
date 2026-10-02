import { FORGE_TIER_ORDER } from './components/benchmark/baselines';
import { publicScoreDetails } from './benchPublicScore';

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
  // The site recomputes the caps from the posted rows and refuses a body without them (INTEGRATION.md).
  const full = verdict as { admission?: unknown; rawScore?: unknown; checks?: unknown };
  const admission = full.admission as
    | { ceiling?: unknown; reasons?: unknown; failedChecksByBand?: unknown }
    | undefined;
  if (
    !admission ||
    typeof admission.ceiling !== 'number' ||
    !Array.isArray(admission.reasons) ||
    !Array.isArray(admission.failedChecksByBand) ||
    typeof full.rawScore !== 'number' ||
    !Array.isArray(full.checks) ||
    full.checks.length === 0
  )
    return 'This Forge result lacks its admission record, earned score or check rows — run the benchmark again to publish.';
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

interface ForgeCheckRow {
  check?: unknown;
  tier?: unknown;
  score?: unknown;
  detail?: unknown;
}

/**
 * The Forge-specific body of a publish (forge/INTEGRATION.md, "Publish — as implemented on the site"):
 * `tiers` exactly L K T R S B U V A E (per-tier means, E the excellence slice), `checksSummary` with every
 * forge row and its tier letter, `admission` {ceiling, reasons, failedChecksByBand} and `rawScore` as the
 * scorer recorded them (the site recomputes the caps from the rows and refuses a mismatch), the recorded
 * composition inputs with each excellence condition's `share` as `gateConditions[].value`, and NO
 * `composition`. main.ts merges it over the shared envelope (title, model, poster, runMeta, shots).
 */
export function forgePublishBody(stored: {
  tiers?: unknown;
  verdict?: unknown;
}): Record<string, unknown> {
  const verdict = (stored.verdict ?? {}) as Record<string, unknown> & {
    checks?: ForgeCheckRow[];
    admission?: { ceiling: number; reasons: string[]; failedChecksByBand: unknown[] };
    rawScore?: number;
    excellence?: { conditions?: Array<{ name?: unknown; ok?: unknown; share?: unknown }> };
  };
  const checksSummary = (Array.isArray(verdict.checks) ? verdict.checks : [])
    .filter(
      (c) =>
        typeof c?.check === 'string' &&
        typeof c?.tier === 'string' &&
        (FORGE_TIER_ORDER as readonly string[]).includes(c.tier) &&
        typeof c?.score === 'number'
    )
    .map((c) => ({
      check: (c.check as string).slice(0, 60),
      tier: c.tier as string,
      score: c.score as number,
      detail: (typeof c.detail === 'string' ? c.detail : '').slice(0, 220),
    }));
  const details = publicScoreDetails(verdict);
  const conditions = verdict.excellence?.conditions;
  if (Array.isArray(conditions))
    details.gateConditions = conditions.map(({ name, ok, share }) => ({ name, ok, value: share }));
  return {
    tiers: forgePublishTiers((stored.tiers ?? {}) as Record<string, unknown>),
    ...details,
    ...(verdict.admission
      ? {
          admission: {
            ceiling: verdict.admission.ceiling,
            reasons: verdict.admission.reasons,
            failedChecksByBand: verdict.admission.failedChecksByBand,
          },
        }
      : {}),
    ...(typeof verdict.rawScore === 'number' ? { rawScore: verdict.rawScore } : {}),
    checksSummary,
  };
}

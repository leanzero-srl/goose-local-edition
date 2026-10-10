import {
  FORGE_ERA_TIER_ORDER,
  eraDisplayName,
  forgeEra,
  type ForgeEra,
} from './components/benchmark/baselines';
import { forgeHasReliability, readForgeReliability } from './benchForgeReliability';
import { publicScoreDetails } from './benchPublicScore';

/**
 * What a reliability era's verdict (forge-2.0) must carry for leanzero.net to accept it, or null. The site
 * recomputes reliability from the posted rows and refuses a rawScore that is not tier means × critical
 * multiplier × that reliability, and it requires `criticalMultiplier`. A verdict scored before the rule has
 * no such block and the app cannot know its factor without computing it; posting "reliability 1" would be a
 * claim the scorer never made, so it is refused here in words and a re-score writes the block.
 */
function forgeReliabilityProblem(version: string, verdict: Record<string, unknown>): string | null {
  if (!forgeHasReliability(version)) return null;
  const era = eraDisplayName(forgeEra(version) ?? version);
  if (verdict.reliability === undefined)
    return `This ${era} result has no \`reliability\` record: it was scored before failed tests multiplied the score. Re-score the saved build to publish it.`;
  if (!readForgeReliability(verdict.reliability))
    return `This ${era} result's \`reliability\` record is not what its scorer writes (the multiplier, and each group's worst test with its factor). Re-score the saved build to publish it.`;
  if (typeof (verdict.critical as { multiplier?: unknown } | undefined)?.multiplier !== 'number')
    return `This ${era} result has no \`critical.multiplier\`, which leanzero.net requires beside the reliability factor. Re-score the saved build to publish it.`;
  return null;
}

/**
 * The Forge half of benchmark-publish (forge/INTEGRATION.md: same routes, same envelope, the result's own
 * scorerVersion and its era's tier letters — forge-1.0's L K T R S B U V A + E, forge-2.0's ten plus R1–R9).
 * The server validates per era and stays the authority; these refuse locally what the era's scorer itself
 * marks as not board-grade, in its own words.
 */
export function forgePublishProblem(stored: {
  scorerVersion?: unknown;
  verdict?: unknown;
}): string | null {
  const version = typeof stored.scorerVersion === 'string' ? stored.scorerVersion : '';
  if (!forgeEra(version))
    return `${version || 'This result'} is not a Forge benchmark this app knows, so it cannot publish from here.`;
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
  // An rc result is refused as rc whatever else it lacks: a re-score under an rc scorer stays rc, so the
  // missing-reliability sentence (which a re-score does fix) is for a frozen era's result.
  if (/-rc$/.test(version))
    return `Scored by ${version}: the Forge thresholds are not frozen yet, so the result is not board-grade. Forge results publish once the ${version.replace(/-rc$/, '')} freeze pins them.`;
  return forgeReliabilityProblem(version, verdict as Record<string, unknown>);
}

/** Every tier letter of the era with its mean, an unrecorded tier as 0 — the SB publisher's A–D rule, per era. */
export function forgePublishTiers(
  tiers: Record<string, unknown>,
  era: ForgeEra
): Record<string, number> {
  return Object.fromEntries(
    FORGE_ERA_TIER_ORDER[era].map((tier) => [
      tier,
      typeof tiers[tier] === 'number' ? tiers[tier] : 0,
    ])
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
 * `tiers` exactly the era's letters (per-tier means, E the excellence slice), `checksSummary` with every
 * forge row and its tier letter, `admission` {ceiling, reasons, failedChecksByBand} and `rawScore` as the
 * scorer recorded them (the site recomputes the caps from the rows and refuses a mismatch), the recorded
 * composition inputs with each excellence condition's `share` as `gateConditions[].value`, and NO
 * `composition`. main.ts merges it over the shared envelope (title, model, poster, runMeta, shots) after
 * forgePublishProblem passed, which refuses a scorer that names no era this app knows.
 *
 * A reliability era (forge-2.0; the site's route v2.9) also posts the scorer's own evidence from
 * `verdict.reliability`, passed through and never recomputed: `reliability` (its multiplier) and
 * `reliabilityDefects` (the scorer's `defects`: one entry per group of tests that multiplies the score —
 * the group's tier, its worst test, that test's score and the group's factor). The site recomputes both from
 * the posted rows and refuses a mismatch; `criticalMultiplier` (`critical.multiplier`, with the other
 * recorded inputs) is required there. Each row's `detail` is the scorer's text from its first character —
 * the site reads a row's state from how it begins — cut only at the site's 220-character limit, and omitted
 * when the scorer wrote none (the site refuses an empty one). A verdict without the block throws: the body
 * is never built with a default factor.
 */
export function forgePublishBody(stored: {
  scorerVersion?: unknown;
  tiers?: unknown;
  verdict?: unknown;
}): Record<string, unknown> {
  const era = forgeEra(typeof stored.scorerVersion === 'string' ? stored.scorerVersion : undefined);
  if (!era)
    throw new Error(`${String(stored.scorerVersion)} is not a Forge benchmark this app knows.`);
  const letters = FORGE_ERA_TIER_ORDER[era];
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
        letters.includes(c.tier) &&
        typeof c?.score === 'number'
    )
    .map((c) => {
      const detail = typeof c.detail === 'string' ? c.detail.slice(0, 220) : '';
      return {
        check: (c.check as string).slice(0, 60),
        tier: c.tier as string,
        score: c.score as number,
        ...(detail.trim() ? { detail } : {}),
      };
    });
  const details = publicScoreDetails(verdict);
  const conditions = verdict.excellence?.conditions;
  if (Array.isArray(conditions))
    details.gateConditions = conditions.map(({ name, ok, share }) => ({ name, ok, value: share }));
  const version = stored.scorerVersion as string;
  const problem = forgeReliabilityProblem(version, verdict);
  if (problem) throw new Error(problem);
  const reliability = forgeHasReliability(version)
    ? readForgeReliability(verdict.reliability)
    : null;
  return {
    tiers: forgePublishTiers((stored.tiers ?? {}) as Record<string, unknown>, era),
    ...details,
    ...(reliability
      ? { reliability: reliability.multiplier, reliabilityDefects: reliability.defects }
      : {}),
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

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
    return `This ${era} result was scored before failed tests multiplied the score, so it has no reliability step. Re-score the saved build to publish it.`;
  if (!readForgeReliability(verdict.reliability))
    return `This ${era} result's reliability step is not in the form its scorer writes (the multiplier, and each group's worst test with its factor), so it cannot be checked. Re-score the saved build to publish it.`;
  if (typeof (verdict.critical as { multiplier?: unknown } | undefined)?.multiplier !== 'number')
    return `This ${era} result does not record its critical-defect multiplier, which leanzero.net requires beside the reliability step. Re-score the saved build to publish it.`;
  return null;
}

/** "3 Forge or Jira calls", "1 Forge or Jira call"; an unreadable count is "some", never a number. */
const heldCalls = (count: number) =>
  `${count || 'some'} Forge or Jira call${count === 1 ? '' : 's'}`;

/**
 * A held Forge verdict in plain words (score_forge*.py: status `held`, `harness_missing` — the built app used
 * calls the benchmark's simulated Forge platform does not model; the score is kept, never zeroed, and the
 * saved build is re-scored once the platform models them). `sentence` is what the score view states; `refusal`
 * is the whole publish refusal, with the next step.
 */
export function forgeHeldWords(verdict: { harness_missing?: unknown }): {
  sentence: string;
  refusal: string;
} {
  const count = Array.isArray(verdict.harness_missing) ? verdict.harness_missing.length : 0;
  const what = `the built app uses ${heldCalls(count)} that the benchmark's simulated platform does not support yet`;
  return {
    sentence: `On hold: ${what}. The score is not zeroed and cannot be published as it is.`,
    refusal: `This Forge result is on hold: ${what}. It cannot be published as it is. When a Goose Swarm update supports ${count === 1 ? 'that call' : 'those calls'}, re-score the saved build.`,
  };
}

/**
 * Why a Forge scorer marked its verdict unpublishable, per reason it records (compose_from_rows:
 * `N unavailable row(s)` — tests the harness itself could not run — and `runtime <name> (…)` — scored on a
 * runtime other than Atlassian's pinned one): `what` happened in plain words, and the `next` step. The score
 * view states what happened; the publish refusal adds the step. A reason in a form this app does not know is
 * quoted as the scorer wrote it, never dropped; no reason at all is said as that.
 */
export function forgeUnpublishableWords(verdict: {
  unpublishable_reasons?: unknown;
}): Array<{ what: string; next: string }> {
  const reasons = Array.isArray(verdict.unpublishable_reasons)
    ? verdict.unpublishable_reasons.map(String).filter(Boolean)
    : [];
  if (reasons.length === 0)
    return [
      { what: 'its scorer recorded no reason.', next: 'Run the benchmark again to publish.' },
    ];
  return reasons.map((reason) => {
    const tests = /^(\d+) unavailable row\(s\)$/.exec(reason);
    if (tests)
      return {
        what: `the scorer could not run ${tests[1]} of its tests, which is a scoring problem and not the model's.`,
        next: 'Re-score the saved build; if the same tests still cannot run, it takes a Goose Swarm update that fixes the scorer.',
      };
    const runtime = /^runtime (\S+) \(/.exec(reason);
    if (runtime)
      return {
        what: `it was scored on the ${runtime[1]} runtime, not Atlassian's pinned Forge runtime.`,
        next: 'Run the benchmark again with the Forge kit prepared to publish.',
      };
    return {
      what: `the scorer's reason reads "${reason}".`,
      next: 'Re-scoring the saved build or a new run may clear it.',
    };
  });
}

/**
 * The Forge half of benchmark-publish (forge/INTEGRATION.md: same routes, same envelope, the result's own
 * scorerVersion and its era's tier letters — forge-1.0's L K T R S B U V A + E, forge-2.0's ten plus R1–R9).
 * The server validates per era and stays the authority; these refuse locally what the era's scorer itself
 * marks as not board-grade — in plain words: what happened, then the next step. No scorer field name, id or
 * internal term ("rc", "row", "admission") reaches the person.
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
  if (verdict.status === 'held') return forgeHeldWords(verdict).refusal;
  if (verdict.publishable === false)
    return `This Forge result cannot be published: ${forgeUnpublishableWords(verdict)
      .map(({ what, next }) => `${what} ${next}`)
      .join(' Also: ')}`;
  if (verdict.publishable !== true)
    return 'This Forge result was saved without its scorer saying whether it may be published. Run the benchmark again to publish.';
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
    return 'This Forge result is missing its score caps, its earned score or its test results. Run the benchmark again to publish.';
  // A result scored before its era's thresholds were final is refused as that, whatever else it lacks. The
  // next step is a new run, never a re-score: bench_rescore.py refuses a build whose task text differs from
  // the installed one, and the task text moved after every pre-final scorer.
  if (/-rc$/.test(version))
    return `Scored before ${eraDisplayName(version.replace(/-rc$/, ''))}'s thresholds were final, so this result is a measurement, not a board result. Run the benchmark again to publish.`;
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

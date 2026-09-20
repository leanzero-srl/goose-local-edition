import { sb8CompositionSchema } from './sb8ScoreSchema';

/** The shared scorer-to-desktop projection used at run completion and slot recovery. */
export function projectBenchScore(v: {
  tiers?: Record<string, number | { mean?: number }>;
  scorerVersion?: string;
  scorer_version?: string;
  checks?: unknown[];
  [key: string]: unknown;
}) {
  const scorerVersion = v.scorerVersion ?? v.scorer_version ?? 'unknown';
  const tiers = Object.fromEntries(
    Object.entries(v.tiers ?? {}).flatMap(([tier, value]) => {
      const mean = typeof value === 'number' ? value : value.mean;
      return typeof mean === 'number' ? [[tier, mean]] : [];
    })
  );
  return {
    scorerVersion,
    tiers,
    verdict: {
      scorerVersion,
      checks: Array.isArray(v.checks) ? v.checks : [],
      tiers: v.tiers ?? {},
      ...(typeof v.scoreInner === 'number' ? { scoreInner: v.scoreInner } : {}),
      ...(typeof v.criticalMultiplier === 'number'
        ? { criticalMultiplier: v.criticalMultiplier }
        : {}),
      ...(typeof v.calibrated === 'boolean' ? { calibrated: v.calibrated } : {}),
      ...(typeof v.core === 'number' ? { core: v.core } : {}),
      ...(typeof v.hard === 'number' ? { hard: v.hard } : {}),
      ...(typeof v.excellent === 'boolean' ? { excellent: v.excellent } : {}),
      ...(typeof v.solid === 'boolean' ? { solid: v.solid } : {}),
      ...Object.fromEntries(
        [
          'fixture_seed',
          'calibration',
          'inner',
          'excellence_gate',
          'excellence',
          'critical',
          'gamma',
          'k_p',
          'probe_unavailable',
          'vacuous',
          'harness_missing',
          'sched_unreached',
        ]
          .filter((key) => key in v)
          .map((key) => [key, v[key]])
      ),
      root_causes: v.root_causes ?? {},
      ...('admission' in v ? { admission: v.admission } : {}),
      ...(typeof v.rawScore === 'number' ? { rawScore: v.rawScore } : {}),
      ...('weights' in v ? { weights: v.weights } : {}),
      ...('core_tiers' in v ? { core_tiers: v.core_tiers } : {}),
      ...('scorer_files_sha256' in v ? { scorer_files_sha256: v.scorer_files_sha256 } : {}),
      ...(v.provenance && typeof v.provenance === 'object' ? { provenance: v.provenance } : {}),
    },
  };
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

/** Recover only from the same scorer result; a reused slot must never lend its verdict to a row. */
export function recoverStoredSb8Score(stored: unknown, canonical: unknown): unknown {
  if (!isRecord(stored) || !isRecord(canonical)) return stored;
  if (typeof stored.scorerVersion !== 'string' || !/^sb-8(?:\.|$)/.test(stored.scorerVersion))
    return stored;
  const version = canonical.scorerVersion ?? canonical.scorer_version;
  if (
    version !== stored.scorerVersion ||
    (canonical.scorer_version !== undefined && canonical.scorer_version !== version) ||
    typeof canonical.score !== 'number' ||
    !Number.isFinite(canonical.score) ||
    canonical.score !== stored.score
  )
    return stored;
  const cloud = typeof canonical.model === 'string' || canonical.provider === 'google';
  if (cloud && (typeof canonical.model !== 'string' || canonical.model !== stored.modelId))
    return stored;
  const tiers = canonical.tiers;
  if (
    !isRecord(tiers) ||
    !sb8CompositionSchema(canonical) ||
    !Array.isArray(canonical.checks) ||
    typeof canonical.scoreInner !== 'number' ||
    !Number.isFinite(canonical.scoreInner) ||
    typeof canonical.criticalMultiplier !== 'number' ||
    !Number.isFinite(canonical.criticalMultiplier)
  )
    return stored;
  const projected = projectBenchScore({
    ...canonical,
    tiers: canonical.tiers as Record<string, number>,
  });
  return {
    ...stored,
    ...projected,
    verdict: {
      ...(isRecord(stored.verdict) ? stored.verdict : {}),
      ...projected.verdict,
    },
  };
}

/** Historical SB8 verdicts predate recorded composition metadata. These are their original weights. */
const LEGACY_WEIGHTS = { A: 0.1, B: 0.2, C: 0.45, D: 0.2, E: 0.05 };
const LEGACY_CORE = ['A', 'B', 'C', 'D'];

export interface Sb8CompositionSchema {
  weights: Record<string, number>;
  coreTiers: string[];
}

const record = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);

/** A partial/new schema is missing evidence, never permission to apply historical weights. */
export function sb8CompositionSchema(verdict: {
  tiers?: unknown;
  weights?: unknown;
  core_tiers?: unknown;
}): Sb8CompositionSchema | null {
  const tiers = verdict.tiers;
  if (!record(tiers)) return null;
  const legacy = verdict.weights === undefined && verdict.core_tiers === undefined;
  const weights = legacy ? LEGACY_WEIGHTS : verdict.weights;
  const core = legacy ? LEGACY_CORE : verdict.core_tiers;
  if (!record(weights) || !Array.isArray(core) || core.length === 0) return null;
  const keys = Object.keys(weights);
  if (
    !keys.includes('E') ||
    Object.keys(tiers).length !== keys.length ||
    !keys.every(
      (key) =>
        typeof weights[key] === 'number' &&
        Number.isFinite(weights[key]) &&
        weights[key] >= 0 &&
        typeof tiers[key] === 'number' &&
        Number.isFinite(tiers[key])
    ) ||
    Math.abs(
      Object.values(weights).reduce<number>((sum, weight) => sum + (weight as number), 0) - 1
    ) > 1e-9 ||
    !core.every((tier) => typeof tier === 'string' && tier !== 'E' && keys.includes(tier)) ||
    new Set(core).size !== core.length
  )
    return null;
  return { weights: weights as Record<string, number>, coreTiers: core as string[] };
}

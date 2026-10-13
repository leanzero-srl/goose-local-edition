import { createHash } from 'node:crypto';

/** Stable private bindings; publication continues through the existing per-build allowlist. */
export function canonicalBenchJson(value: unknown): string {
  if (value === null || typeof value !== 'object') {
    const serialized = JSON.stringify(value);
    if (serialized === undefined || (typeof value === 'number' && !Number.isFinite(value)))
      throw new Error('Benchmark identity contains an unsupported value.');
    return serialized;
  }
  if (Array.isArray(value)) return `[${value.map(canonicalBenchJson).join(',')}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${canonicalBenchJson(record[key])}`)
    .join(',')}}`;
}

export function benchIdentityHash(value: unknown): string {
  return createHash('sha256').update(canonicalBenchJson(value)).digest('hex');
}

const COHORT_FIELDS = [
  'taskFingerprint',
  'scorerVersion',
  'scorer_files_sha256',
  'spec_sha256',
  'kit_lock_sha256',
  'wrapper_sha256',
  'kit_code_sha256',
  'dev_seed',
  'input_manifest',
  'reasoning_effort',
  'fixture_seeds',
];
const record = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

/** Read the raw canonical verdict, rather than the display projection which omits fixture identity. */
export function forgeSeriesComparison(verdict: unknown): {
  identity?: Record<string, unknown>;
  problem?: string;
} {
  if (
    !record(verdict) ||
    verdict.scorerVersion !== 'forge-2.0' ||
    verdict.status !== 'scored' ||
    verdict.publishable !== true
  )
    return { problem: 'Only a fully observed canonical Forge 2.0 result is comparable.' };
  const identity = Object.fromEntries(COHORT_FIELDS.map((key) => [key, verdict[key]]));
  if (
    Object.values(identity).some(
      (value) =>
        value == null ||
        (Array.isArray(value) && value.length === 0) ||
        (record(value) && Object.keys(value).length === 0)
    )
  )
    return { problem: 'The canonical task, scorer, kit or fixture identity is incomplete.' };
  if (
    typeof verdict.taskFingerprint !== 'string' ||
    !/^[a-f0-9]{64}$/.test(verdict.taskFingerprint) ||
    typeof verdict.buildId !== 'string' ||
    !verdict.buildId ||
    !record(verdict.input_manifest) ||
    !Array.isArray(verdict.fixture_seeds) ||
    verdict.fixture_seeds.length !== 3 ||
    new Set(verdict.fixture_seeds).size !== 3 ||
    !record(verdict.seed_scores) ||
    canonicalBenchJson(Object.keys(verdict.seed_scores).sort()) !==
      canonicalBenchJson([...verdict.fixture_seeds].sort())
  )
    return { problem: 'The build identity or three common fixture results are missing.' };
  const checks = Array.isArray(verdict.checks) ? verdict.checks : [];
  const names = checks.map((row) => (record(row) ? row.check : undefined));
  if (
    !names.length ||
    names.some((name) => typeof name !== 'string' || !name) ||
    new Set(names).size !== names.length
  )
    return { problem: 'The canonical check registry is missing or repeats a check.' };
  const agent = record(verdict.agent) ? verdict.agent : {};
  const budget = record(agent.budget) ? agent.budget : {};
  if (['wallet_guard', 'wallet_accounting_failure'].includes(String(budget.stopped_by)))
    return { problem: 'An interrupted build cannot be included in the two-build comparison.' };
  identity.input_manifest = Object.fromEntries(
    Object.entries(verdict.input_manifest).filter(([key]) => key !== 'model-limits.json')
  );
  identity.checks = (names as string[]).sort();
  return { identity };
}

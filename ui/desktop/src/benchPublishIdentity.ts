import { createHash } from 'node:crypto';

/**
 * What a published run needs so leanzero.net shows it the way the owner expects with no operator script
 * (Mihai 2026-10-18: "the App can take care of this itself or self service please"): a readable NAME, a Forge
 * 2.0 COUPLING receipt so a series' two builds become one entry, and whether the run used a LOCAL engine
 * (a local run is never priced from a cloud price list and never sent as a LeanZero cloud baseline).
 */

/**
 * Python's `json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)` (ensure_ascii on), byte
 * for byte — the bytes forge2_compare.py hashes into a cohort sha. JSON.stringify differs on non-ASCII (raw vs
 * \uXXXX) and on floats (Python prints 1.0, JS 1), so a non-integer number is refused rather than guessed.
 */
export function pythonCanonicalJson(value: unknown): string {
  if (value === null) return 'null';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value))
      throw new Error('The cohort holds a non-integer number; its Python bytes cannot be reproduced.');
    return String(value);
  }
  if (typeof value === 'string')
    return JSON.stringify(value).replace(
      /[\u007f-￿]/g,
      (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0')
    );
  if (Array.isArray(value)) return `[${value.map(pythonCanonicalJson).join(',')}]`;
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record)
      .sort()
      .map((key) => `${pythonCanonicalJson(key)}:${pythonCanonicalJson(record[key])}`)
      .join(',')}}`;
  }
  throw new Error('The cohort holds a value JSON cannot carry.');
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
] as const;
const isRecord = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);
const empty = (v: unknown) =>
  v == null || (Array.isArray(v) && v.length === 0) || (isRecord(v) && Object.keys(v).length === 0);

export interface Forge2CohortReceipt {
  cohortCanonicalJson: string;
  comparisonCohortSha: string;
}

/**
 * forge2_compare.compare()'s cohort for ONE build, from its raw canonical verdict.json: the task/kit/scorer/
 * fixture identity, input manifest without model-limits.json, the sorted check registry, the engine binary's
 * sha256 and the settings forge2_cohort.py records (call allowance, reasoning effort). Every build of one
 * setup hashes to the same sha, so the site couples a series' builds as they are published (website
 * src/lib/forge2CohortReceipt.ts verifies it). Null with a reason when the verdict cannot carry one.
 */
export function forge2CohortReceipt(
  verdict: unknown,
  engineSha256: string
): { receipt: Forge2CohortReceipt } | { problem: string } {
  if (!isRecord(verdict) || verdict.scorerVersion !== 'forge-2.0')
    return { problem: 'Not a Forge 2.0 verdict.' };
  if (verdict.status !== 'scored' || verdict.publishable !== true)
    return { problem: 'Only a fully observed, publishable Forge 2.0 build is comparable.' };
  if (!/^[a-f0-9]{64}$/.test(engineSha256)) return { problem: 'The engine binary hash is missing.' };
  const identity: Record<string, unknown> = {};
  for (const key of COHORT_FIELDS) {
    if (empty(verdict[key])) return { problem: `The verdict omits ${key}.` };
    identity[key] = verdict[key];
  }
  if (!isRecord(identity.input_manifest)) return { problem: 'The verdict input manifest is malformed.' };
  identity.input_manifest = Object.fromEntries(
    Object.entries(identity.input_manifest).filter(([k]) => k !== 'model-limits.json')
  );
  const seeds = identity.fixture_seeds;
  if (
    !Array.isArray(seeds) ||
    new Set(seeds).size !== 3 ||
    !isRecord(verdict.seed_scores) ||
    pythonCanonicalJson(Object.keys(verdict.seed_scores).sort()) !==
      pythonCanonicalJson([...seeds].map(String).sort())
  )
    return { problem: 'Three common fixture results are required.' };
  const checks = Array.isArray(verdict.checks) ? verdict.checks : [];
  const names = checks.map((row) => (isRecord(row) ? row.check : undefined));
  if (!names.length || names.some((n) => typeof n !== 'string' || !n) || new Set(names).size !== names.length)
    return { problem: 'The full, unique check registry is required.' };
  const budget = isRecord(verdict.budget) ? verdict.budget : {};
  const effort = isRecord(verdict.reasoning_effort) ? verdict.reasoning_effort : {};
  if (!Number.isSafeInteger(budget.max_calls) || typeof effort.value !== 'string')
    return { problem: 'The call allowance or reasoning effort the build ran with is missing.' };
  const agentBudget = isRecord(verdict.agent) && isRecord(verdict.agent.budget) ? verdict.agent.budget : {};
  if (['wallet_guard', 'wallet_accounting_failure'].includes(String(agentBudget.stopped_by)))
    return { problem: 'A resource-interrupted build is not comparable.' };
  identity.engine_sha256 = engineSha256;
  identity.settings = { nativeCallAllowance: budget.max_calls, localPolicyEffort: effort.value };
  identity.checks = [...(names as string[])].sort();
  let canonical: string;
  try {
    canonical = pythonCanonicalJson(identity);
  } catch (error) {
    return { problem: error instanceof Error ? error.message : String(error) };
  }
  return {
    receipt: {
      cohortCanonicalJson: canonical,
      comparisonCohortSha: createHash('sha256').update(canonical).digest('hex'),
    },
  };
}

const LOOPBACK = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?(\/|$)/i;

/** Whether a run used a model on THIS Mac: the harness's isolation record lists its loopback provider under
 *  `direct_providers` (a cloud run reaches its provider only through the relay, so that list is empty). The
 *  provider NAME cannot tell: a local engine is served through the built-in `openai` provider. */
export function runUsedLocalEngine(isolation: unknown): boolean {
  const direct = isRecord(isolation) ? isolation.direct_providers : undefined;
  return Array.isArray(direct) && direct.length > 0 && direct.every((u) => typeof u === 'string' && LOOPBACK.test(u));
}

/** How the board's title names the route: "single model via OpenRouter". */
export const PROVIDER_TITLE_LABELS: Readonly<Record<string, string>> = {
  openrouter: 'OpenRouter',
  google: 'Google',
  anthropic: 'Anthropic',
  aws_bedrock: 'Amazon Bedrock',
  openai: 'OpenAI',
  azure_openai: 'Azure',
  alibaba: 'Alibaba Cloud',
  minimax: 'MiniMax',
  ollama_cloud: 'Ollama Cloud',
};

/** OpenRouter's catalog `name` without its vendor ("DeepSeek: DeepSeek V4.1 Flash 1017" → "DeepSeek V4.1
 *  Flash 1017") for the run's model: the exact id, else for a direct provider the one entry whose id ends with
 *  "/<model>" (gemini-4-argon → google/gemini-4-argon). */
export function catalogDisplayName(
  catalog: ReadonlyArray<{ id?: unknown; name?: unknown }>,
  model: string,
  provider: string
): string | null {
  const id = model.trim().replace(/^~/, '');
  const named = (x: { id?: unknown; name?: unknown }) =>
    typeof x.id === 'string' && typeof x.name === 'string' && x.name.trim() ? x : null;
  let hit = catalog.find((x) => named(x) && (x.id as string).replace(/^~/, '') === id);
  if (!hit && provider !== 'openrouter') {
    const tails = catalog.filter(
      (x) => named(x) && !(x.id as string).includes(':') && (x.id as string).endsWith('/' + id)
    );
    if (tails.length === 1) hit = tails[0];
  }
  if (!hit) return null;
  const name = (hit.name as string).split(': ').slice(-1)[0].replace(/\s+/g, ' ').trim();
  return name.length > 0 && name.length <= 60 ? name : null;
}

/** The publish title the card prefills (still editable, still required): the board names a row by the text
 *  before the first comma, so that part is a human model name, never an id. */
export function suggestedPublishTitle(args: {
  model: string;
  provider: string | null;
  local: boolean;
  displayName: string | null;
}): string {
  const bare = args.model.trim().replace(/^~/, '').replace(/^[^/]+\//, '');
  if (args.local) return `${args.displayName ?? bare}, run locally on a Mac`.slice(0, 80);
  const via = (args.provider && PROVIDER_TITLE_LABELS[args.provider]) || args.provider || 'its provider';
  return `${args.displayName ?? bare}, single model via ${via}`.slice(0, 80);
}

/** goose config.yaml key (and environment variable) holding the LeanZero operator key on our own installs. */
export const BENCH_OPERATOR_KEY_CONFIG = 'LEANZERO_BENCH_OPERATOR_KEY';

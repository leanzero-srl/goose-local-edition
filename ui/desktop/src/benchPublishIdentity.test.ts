import { describe, expect, it } from 'vitest';
import {
  catalogDisplayName,
  forge2CohortReceipt,
  pythonCanonicalJson,
  runUsedLocalEngine,
  suggestedPublishTitle,
} from './benchPublishIdentity';

// A fixture verdict and the cohort sha forge2_compare.compare() gave for it (Python 3, 2026-10-18), with
// engine sha "9"*64 and settings {nativeCallAllowance:300, localPolicyEffort:"medium"} as forge2_cohort.py records.
const verdict = {
  scorerVersion: 'forge-2.0',
  status: 'scored',
  publishable: true,
  buildId: 'cloud-fixture-1',
  model: 'vendor/model',
  score: 0.5,
  taskFingerprint: 'a'.repeat(64),
  scorer_files_sha256: { 'score_forge2.py': 'b'.repeat(64) },
  spec_sha256: 'c'.repeat(64),
  kit_lock_sha256: 'd'.repeat(64),
  wrapper_sha256: 'e'.repeat(64),
  kit_code_sha256: 'f'.repeat(64),
  dev_seed: 'seed-é',
  input_manifest: { 'SPEC.md': '1'.repeat(64), 'model-limits.json': '2'.repeat(64) },
  reasoning_effort: { value: 'medium', source: 'forge-2.0 pin' },
  fixture_seeds: ['s1', 's2', 's3'],
  seed_scores: { s1: 0.5, s2: 0.4, s3: 0.6 },
  budget: { max_calls: 300 },
  checks: [
    { check: 'z_last', score: 1 },
    { check: 'a_first', score: 0.5 },
  ],
  agent: { budget: { stopped_by: null } },
};
const PYTHON_SHA = '7ddc28fa14e2485db8fa8afb47bb72df1a80c71be3319c77d9b93efa1c609cbe';

describe('pythonCanonicalJson', () => {
  it('writes json.dumps(sort_keys, compact, ensure_ascii) bytes', () => {
    expect(pythonCanonicalJson({ b: 'é☃\u007f', a: [1, 'x\n', null, true] })).toBe(
      '{"a":[1,"x\\n",null,true],"b":"\\u00e9\\u2603\\u007f"}'
    );
  });
  it('refuses a float, whose Python bytes (1.0) JavaScript cannot reproduce', () => {
    expect(() => pythonCanonicalJson({ a: 0.5 })).toThrow(/non-integer/);
  });
});

describe('forge2CohortReceipt', () => {
  it('reproduces forge2_compare.py exactly: the sha a series shares with its other build', () => {
    const r = forge2CohortReceipt(verdict, '9'.repeat(64));
    expect('receipt' in r && r.receipt.comparisonCohortSha).toBe(PYTHON_SHA);
    if ('receipt' in r) {
      expect(r.receipt.cohortCanonicalJson).not.toContain('model-limits.json');
      expect(r.receipt.cohortCanonicalJson.startsWith('{"checks":["a_first","z_last"]')).toBe(true);
    }
  });
  it('a second build of the same setup (other id and score) gets the same sha; another engine does not', () => {
    const two = { ...verdict, buildId: 'cloud-fixture-2', score: 0.7, seed_scores: { s1: 0.7, s2: 0.6, s3: 0.8 } };
    const a = forge2CohortReceipt(two, '9'.repeat(64));
    const b = forge2CohortReceipt(verdict, '8'.repeat(64));
    expect('receipt' in a && a.receipt.comparisonCohortSha).toBe(PYTHON_SHA);
    expect('receipt' in b && b.receipt.comparisonCohortSha).not.toBe(PYTHON_SHA);
  });
  it('says why when a verdict cannot carry a receipt', () => {
    expect(forge2CohortReceipt({ ...verdict, publishable: false }, '9'.repeat(64))).toHaveProperty('problem');
    expect(forge2CohortReceipt({ ...verdict, fixture_seeds: ['s1', 's2'] }, '9'.repeat(64))).toHaveProperty('problem');
    expect(forge2CohortReceipt({ ...verdict, budget: {} }, '9'.repeat(64))).toHaveProperty('problem');
    expect(forge2CohortReceipt(verdict, '')).toHaveProperty('problem');
    expect(
      forge2CohortReceipt({ ...verdict, agent: { budget: { stopped_by: 'wallet_guard' } } }, '9'.repeat(64))
    ).toHaveProperty('problem');
    expect(forge2CohortReceipt({ ...verdict, scorerVersion: 'sb-7.2' }, '9'.repeat(64))).toHaveProperty('problem');
  });
});

describe('runUsedLocalEngine', () => {
  it('reads the isolation record, not the provider name', () => {
    expect(runUsedLocalEngine({ direct_providers: ['http://127.0.0.1:8090'] })).toBe(true);
    expect(runUsedLocalEngine({ direct_providers: [] })).toBe(false);
    expect(runUsedLocalEngine({ direct_providers: ['https://api.example.com'] })).toBe(false);
    expect(runUsedLocalEngine(null)).toBe(false);
  });
});

describe('titles', () => {
  const catalog = [
    { id: 'deepseek/deepseek-v4.1-flash-1017', name: 'DeepSeek: DeepSeek V4.1 Flash 1017' },
    { id: 'google/gemini-4-argon', name: 'Google: Gemini 4 Argon' },
    { id: 'google/gemini-4-argon:free', name: 'Google: Gemini 4 Argon (free)' },
  ];
  it('uses the catalog name without its vendor, exact id first, a unique /tail for a direct provider', () => {
    expect(catalogDisplayName(catalog, 'deepseek/deepseek-v4.1-flash-1017', 'openrouter')).toBe('DeepSeek V4.1 Flash 1017');
    expect(catalogDisplayName(catalog, 'gemini-4-argon', 'google')).toBe('Gemini 4 Argon');
    expect(catalogDisplayName(catalog, 'gemini-4-argon', 'openrouter')).toBeNull();
    expect(catalogDisplayName(catalog, 'unknown/model', 'openrouter')).toBeNull();
  });
  it('prefills a name the board shows before its first comma, never an id', () => {
    expect(
      suggestedPublishTitle({ model: 'deepseek/deepseek-v4.1-flash-1017', provider: 'openrouter', local: false, displayName: 'DeepSeek V4.1 Flash 1017' })
    ).toBe('DeepSeek V4.1 Flash 1017, single model via OpenRouter');
    expect(suggestedPublishTitle({ model: 'gemini-4-argon', provider: 'google', local: false, displayName: 'Gemini 4 Argon' })).toBe(
      'Gemini 4 Argon, single model via Google'
    );
    expect(
      suggestedPublishTitle({ model: 't12-candidates/Muse-Glimmer-30B-8bit', provider: 'openai', local: true, displayName: null })
    ).toBe('Muse-Glimmer-30B-8bit, run locally on a Mac');
  });
});

import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { benchPublishBilling, refusalIsAboutBilling } from './benchPublishBilling';

const bill = {
  provider: 'openrouter',
  source: 'https://openrouter.ai/api/v1/generation (usage per generation id)',
  status: 'complete',
  billed_usd: 0.2959,
  request_count: 263,
  missing: [],
  incomplete_reasons: [],
};
const usage = { sessions: [{ accumulated_input_tokens: 1_000_000, accumulated_output_tokens: 20_000, accumulated_cache_read_tokens: 0 }] };
const cachedUsage = { sessions: [{ accumulated_input_tokens: 1_000_000, accumulated_output_tokens: 20_000, accumulated_cache_read_tokens: 900_000 }] };
const files = (map: Record<string, unknown>) => async (file: string) => {
  if (!(file in map)) throw Object.assign(new Error('ENOENT'), { code: 'ENOENT' });
  return Buffer.from(JSON.stringify(map[file]));
};
const sha = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
const prices = { inputUsdPerMTok: 0.5, outputUsdPerMTok: 2 };

describe('benchPublishBilling', () => {
  it("sends the run's own OpenRouter bill, hashing the exact verdict bytes it read", async () => {
    const verdict = { agent: { billed_cost: bill, usage } };
    const r = await benchPublishBilling({ workdir: '/run', billedCost: bill }, undefined, files({ '/run/verdict.json': verdict }));
    expect(r.record).toMatchObject({ status: 'complete', source: 'openrouter-generation-records', billedUsd: 0.2959, nativeVerdictSha256: sha(verdict) });
    expect(r.verdict).toEqual(verdict);
  });
  it("reads a re-score's verdict one folder up, and never another run's verdict whose bill differs", async () => {
    const mine = { agent: { billed_cost: bill } };
    const other = { agent: { billed_cost: { ...bill, billed_usd: 9 } } };
    const r = await benchPublishBilling(
      { workdir: '/run/evidence', billedCost: bill },
      undefined,
      files({ '/run/evidence/verdict.json': other, '/run/verdict.json': mine })
    );
    expect(r.record?.nativeVerdictSha256).toBe(sha(mine));
  });
  it('refuses rather than silently dropping a recorded bill or typed prices', async () => {
    expect((await benchPublishBilling({ workdir: '/gone', billedCost: bill }, undefined, files({}))).error).toMatch(/bill is recorded/);
    expect((await benchPublishBilling({ billedCost: bill }, undefined, files({}))).error).toMatch(/bill is recorded/);
    expect((await benchPublishBilling({}, prices, files({}))).error).toMatch(/declared price cannot be attached/);
    expect((await benchPublishBilling({ workdir: '/gone' }, prices, files({}))).error).toMatch(/price cannot be attached/);
    expect(await benchPublishBilling({}, undefined, files({}))).toEqual({});
  });
  it('turns declared prices into an estimate only without a bill, and requires a cached price for cached reads', async () => {
    const local = { agent: { billed_cost: { status: 'unavailable', provider: 'openai' }, usage } };
    const r = await benchPublishBilling({ workdir: '/run' }, prices, files({ '/run/verdict.json': local }));
    expect(r.record).toMatchObject({ status: 'estimated', source: 'declared-prices', billedUsd: (1_000_000 * 0.5 + 20_000 * 2) / 1e6, promptTokens: 1_000_000 });
    const cached = { agent: { billed_cost: { status: 'unavailable' }, usage: cachedUsage } };
    expect((await benchPublishBilling({ workdir: '/run' }, prices, files({ '/run/verdict.json': cached }))).error).toMatch(/cached input price/);
    const ok = await benchPublishBilling({ workdir: '/run' }, { ...prices, cachedInputUsdPerMTok: 0.05 }, files({ '/run/verdict.json': cached }));
    expect(ok.record?.billedUsd).toBeCloseTo((100_000 * 0.5 + 900_000 * 0.05 + 20_000 * 2) / 1e6, 12);
    const billed = { agent: { billed_cost: bill, usage } };
    expect((await benchPublishBilling({ workdir: '/run', billedCost: bill }, prices, files({ '/run/verdict.json': billed }))).record?.source).toBe(
      'openrouter-generation-records'
    );
    expect((await benchPublishBilling({ workdir: '/run' }, { inputUsdPerMTok: -1, outputUsdPerMTok: 1 }, files({}))).error).toMatch(/USD per 1M/);
  });
  it("prices the engine's own telemetry totals when the run has them (the counts the site checks)", async () => {
    const local = { agent: { billed_cost: { status: 'unavailable' }, usage } };
    const r = await benchPublishBilling(
      { workdir: '/run', telemetry: { prompt_tokens: 1_200_000, completion_tokens: 30_000 } },
      prices,
      files({ '/run/verdict.json': local })
    );
    expect(r.record).toMatchObject({ promptTokens: 1_200_000, completionTokens: 30_000 });
  });
});

describe('refusalIsAboutBilling', () => {
  it('retries without the price only for a 4xx about billing', () => {
    expect(refusalIsAboutBilling(400, 'billing must be a valid recorded OpenRouter billing summary.')).toBe(true);
    expect(refusalIsAboutBilling(422, 'billing: a declared-prices estimate must price the run telemetry token counts')).toBe(true);
    expect(refusalIsAboutBilling(422, 'score must be a number')).toBe(false);
    expect(refusalIsAboutBilling(500, 'billing')).toBe(false);
  });
});

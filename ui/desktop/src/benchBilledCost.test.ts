import { describe, expect, it } from 'vitest';
import {
  billedCostLine,
  billedCostRowField,
  declaredPricesFromFields,
  declaredPriceUsd,
  declaredPublishBilling,
  hasOpenRouterBill,
  openRouterPublishBilling,
  readBilledCost,
  runTokens,
} from './benchBilledCost';

const complete = {
  status: 'complete',
  billed_usd: 0.62431,
  tokens: { prompt: 1_204_311, cached: 812_004, completion: 30_118, reasoning: 9_002 },
  hosts: { Together: 43, DeepInfra: 100 },
  requests: 143,
  missing: [],
};

describe('the billed cost the Benchmark view states', () => {
  it('states a complete bill with its source, request count, tokens and hosts', () => {
    expect(billedCostLine(complete, 'openrouter')).toEqual({
      amount: '$0.6243',
      sentence: 'Billed $0.6243 (OpenRouter, 143 requests)',
      tone: null,
      detail:
        'prompt 1,204,311 (cached 812,004) · completion 30,118 · reasoning 9,002 · served by DeepInfra ×100, Together ×43',
    });
  });

  it('states an incomplete bill as a floor with the calls it could not find', () => {
    const line = billedCostLine(
      { ...complete, status: 'incomplete', billed_usd: 0.5, missing: ['gen-1', 'gen-2'] },
      'openrouter'
    );
    expect(line?.sentence).toBe('At least $0.5000 — 2 calls not found');
    expect(line?.amount).toBe('≥ $0.5000');
    expect(line?.tone).toBe('warn');
    expect(
      billedCostLine({ ...complete, status: 'incomplete', missing: ['gen-1'] }, 'openrouter')
        ?.sentence
    ).toBe('At least $0.6243 — 1 call not found');
  });

  it('states an unavailable bill by provider, with the harness reason, and no amount', () => {
    expect(
      billedCostLine({ status: 'unavailable', reason: 'google has no generation API' }, 'google')
    ).toEqual({
      amount: null,
      sentence: 'Billing not available for google.',
      tone: 'stopped',
      detail: 'google has no generation API',
    });
  });

  it('never presents goose’s accumulated_cost estimate as the bill', () => {
    // A session-counter estimate is not a billing record: the shape is refused, not read.
    expect(readBilledCost({ accumulated_cost: 1.23 })).toEqual({ kind: 'malformed' });
    expect(billedCostLine({ accumulated_cost: 1.23 }, 'openrouter')?.sentence).toBe(
      'The billing record saved with this result is unreadable.'
    );
    expect(billedCostLine(undefined, 'openrouter')?.amount).toBeNull();
  });

  it('reports a malformed or missing record instead of a smaller bill', () => {
    for (const bad of [
      'free',
      { status: 'complete' },
      { status: 'complete', billed_usd: -1 },
      { status: 'complete', billed_usd: Number.NaN },
      { status: 'incomplete', billed_usd: 0.1 },
      { status: 'estimated', billed_usd: 0.1 },
    ])
      expect(billedCostLine(bad, 'openrouter')).toMatchObject({ amount: null, tone: 'err' });
    expect(billedCostLine(undefined, 'openrouter')?.sentence).toBe(
      'No billing record was saved with this result.'
    );
    // A swarm without a record bills nothing locally: no line at all.
    expect(billedCostLine(undefined, null)).toBeNull();
  });

  it('persists the record verbatim from the verdict agent block, and nothing when absent', () => {
    expect(billedCostRowField({ secs: 1, billed_cost: complete })).toEqual({
      billedCost: complete,
    });
    expect(billedCostRowField({ billed_cost: { status: 'unavailable' } })).toEqual({
      billedCost: { status: 'unavailable' },
    });
    for (const agent of [undefined, null, 'agent', { usage: { accumulated_cost: 2 } }])
      expect(billedCostRowField(agent)).toEqual({});
  });
});

// The `billing` record benchmark-publish sends (website validateBenchmarkBilling). The OpenRouter mapping is the
// one the 2026-10-18 backfill applied to DeepSeek V4.1 Flash 1017's pair (loop-state forge2_billing.mjs).
const SHA = 'f'.repeat(64);
const orBill = {
  provider: 'openrouter',
  source: 'https://openrouter.ai/api/v1/generation (usage per generation id)',
  model: 'deepseek/deepseek-v4.1-flash-1017',
  status: 'complete',
  billed_usd: 0.295860456,
  request_count: 263,
  missing: [],
  incomplete_reasons: [],
  hosts: { DeepSeek: 263 },
  budget: { calls_other_models: {} },
};

describe('the billing record a publish carries', () => {
  it('maps a complete OpenRouter bill exactly as the backfill did', () => {
    expect(openRouterPublishBilling(orBill, SHA)).toEqual({
      status: 'complete',
      source: 'openrouter-generation-records',
      billedUsd: 0.295860456,
      requestCount: 263,
      missingCallCount: 0,
      includesAuxiliaryCalls: null,
      reasons: [],
      nativeVerdictSha256: SHA,
    });
  });

  it('keeps an incomplete bill a lower bound, with the 404 sentence when every miss is a 404', () => {
    const b = openRouterPublishBilling(
      {
        ...orBill,
        status: 'incomplete',
        missing: [{ id: 'gen-1', reason: 'HTTP 404' }],
        incomplete_reasons: ['1 generation id(s) have no billed record'],
        hosts: { DeepSeek: 262, Google: 1 },
        budget: { calls_other_models: { 'gemini-3.8-flash': 1 } },
      },
      SHA
    );
    expect(b?.status).toBe('incomplete');
    expect(b && 'missingCallCount' in b ? b.missingCallCount : null).toBe(1);
    expect(b?.includesAuxiliaryCalls).toBe(true);
    expect(b?.reasons).toEqual([
      '1 generation id(s) have no billed record',
      'OpenRouter returned HTTP 404 for every missing generation record. Missing charges are unknown, not zero.',
    ]);
  });

  it('states an OpenRouter record with no amount as unavailable, never a zero bill', () => {
    const b = openRouterPublishBilling({ ...orBill, billed_usd: null }, SHA);
    expect(b?.status).toBe('unavailable');
    expect(b && 'billedUsd' in b).toBe(false);
  });

  it('never states a bill with missing calls as complete', () => {
    const b = openRouterPublishBilling(
      { ...orBill, missing: [{ id: 'gen-9', reason: 'timeout' }] },
      SHA
    );
    expect(b?.status).toBe('incomplete');
  });

  it('sends nothing for a bill that is not OpenRouter, or that breaks its shape', () => {
    expect(
      openRouterPublishBilling({ status: 'unavailable', provider: 'google', reason: 'x' }, SHA)
    ).toBeNull();
    expect(openRouterPublishBilling({ ...orBill, missing: undefined }, SHA)).toBeNull();
    expect(openRouterPublishBilling({ ...orBill, billed_usd: -1 }, SHA)).toBeNull();
    expect(openRouterPublishBilling(orBill, 'not-a-sha')).toBeNull();
    expect(openRouterPublishBilling(undefined, SHA)).toBeNull();
    expect(hasOpenRouterBill(orBill)).toBe(true);
    expect(hasOpenRouterBill({ status: 'unavailable', provider: 'google' })).toBe(false);
  });
});

describe('a publisher-declared price when goose has no bill', () => {
  const usage = {
    sessions: [
      {
        accumulated_input_tokens: 54_217_535,
        accumulated_output_tokens: 203_647,
        accumulated_cache_read_tokens: 52_893_080,
      },
    ],
  };

  it("reads the run's own token counts and refuses counters that do not add up", () => {
    expect(runTokens(usage)).toEqual({
      promptTokens: 54_217_535,
      cachedTokens: 52_893_080,
      completionTokens: 203_647,
    });
    expect(
      runTokens({ sessions: [{ accumulated_input_tokens: null, accumulated_output_tokens: 1 }] })
    ).toBeNull();
    expect(
      runTokens({
        sessions: [
          {
            accumulated_input_tokens: 1,
            accumulated_output_tokens: 1,
            accumulated_cache_read_tokens: 2,
          },
        ],
      })
    ).toBeNull();
    expect(runTokens({ sessions: [] })).toBeNull();
    expect(runTokens(undefined)).toBeNull();
  });

  it('parses the fields: all blank sends nothing, half-filled or junk is refused with the reason', () => {
    expect(declaredPricesFromFields({ input: '', output: '', cached: '' })).toEqual({
      prices: null,
      problem: null,
    });
    expect(declaredPricesFromFields({ input: '1.25', output: '10', cached: '' })).toEqual({
      prices: { inputUsdPerMTok: 1.25, outputUsdPerMTok: 10 },
      problem: null,
    });
    expect(
      declaredPricesFromFields({ input: '1.25', output: '10', cached: '0.125' }).prices
        ?.cachedInputUsdPerMTok
    ).toBe(0.125);
    expect(declaredPricesFromFields({ input: '1', output: '', cached: '' }).problem).toMatch(
      /both/
    );
    expect(declaredPricesFromFields({ input: '-1', output: '2', cached: '' }).problem).toMatch(
      /USD per 1M/
    );
    expect(declaredPricesFromFields({ input: '1e3', output: '2', cached: '' }).problem).toMatch(
      /USD per 1M/
    );
    expect(declaredPricesFromFields({ input: '20000', output: '2', cached: '' }).problem).toMatch(
      /USD per 1M/
    );
  });

  it('reads a decimal comma as the decimal point and refuses thousands separators', () => {
    expect(declaredPricesFromFields({ input: '1,25', output: '10', cached: '' }).prices).toEqual({
      inputUsdPerMTok: 1.25,
      outputUsdPerMTok: 10,
    });
    expect(
      declaredPricesFromFields({ input: '1,250.5', output: '10', cached: '' }).problem
    ).toMatch(/USD per 1M/);
  });

  it('never guesses a comma that could be a thousands separator ("1,250")', () => {
    const typed = declaredPricesFromFields({ input: '1,250', output: '10', cached: '' });
    expect(typed.prices).toBeNull();
    expect(typed.problem).toMatch(/could mean 1\.250 or 1250/);
    expect(declaredPricesFromFields({ input: '0,125', output: '1,5', cached: '' }).prices).toEqual({
      inputUsdPerMTok: 0.125,
      outputUsdPerMTok: 1.5,
    });
    expect(declaredPricesFromFields({ input: '1', output: '2', cached: '12,500' }).problem).toMatch(
      /could mean 12\.500 or 12500/
    );
  });

  it('requires a cached input price when the run read cached tokens', () => {
    const tokens = { promptTokens: 1_000_000, cachedTokens: 900_000, completionTokens: 1 };
    expect(
      declaredPricesFromFields({ input: '1', output: '2', cached: '' }, tokens).problem
    ).toMatch(/cached input price/);
    expect(
      declaredPricesFromFields({ input: '1', output: '2', cached: '0,1' }, tokens).prices
    ).toEqual({
      inputUsdPerMTok: 1,
      outputUsdPerMTok: 2,
      cachedInputUsdPerMTok: 0.1,
    });
    expect(
      declaredPricesFromFields(
        { input: '1', output: '2', cached: '' },
        { ...tokens, cachedTokens: 0 }
      ).prices
    ).not.toBeNull();
  });

  it('prices uncached prompt, cached prompt and completion the way the site recomputes it', () => {
    const tokens = runTokens(usage)!;
    const prices = { inputUsdPerMTok: 1.25, outputUsdPerMTok: 10, cachedInputUsdPerMTok: 0.125 };
    const usd = (1_324_455 * 1.25 + 52_893_080 * 0.125 + 203_647 * 10) / 1e6;
    expect(declaredPriceUsd(tokens, prices)).toBeCloseTo(usd, 12);
    expect(declaredPriceUsd(tokens, { inputUsdPerMTok: 1, outputUsdPerMTok: 0 })).toBeCloseTo(
      54.217535,
      9
    );
    expect(declaredPublishBilling(tokens, prices, SHA)).toEqual({
      status: 'estimated',
      source: 'declared-prices',
      billedUsd: declaredPriceUsd(tokens, prices),
      inputUsdPerMTok: 1.25,
      outputUsdPerMTok: 10,
      cachedInputUsdPerMTok: 0.125,
      promptTokens: 54_217_535,
      cachedTokens: 52_893_080,
      completionTokens: 203_647,
      includesAuxiliaryCalls: null,
      reasons: [],
      nativeVerdictSha256: SHA,
    });
    expect(
      declaredPublishBilling(tokens, { inputUsdPerMTok: -1, outputUsdPerMTok: 1 }, SHA)
    ).toBeNull();
  });
});

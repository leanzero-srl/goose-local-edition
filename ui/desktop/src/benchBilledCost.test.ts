import { describe, expect, it } from 'vitest';
import { billedCostLine, billedCostRowField, readBilledCost } from './benchBilledCost';

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

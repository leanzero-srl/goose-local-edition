import { describe, expect, it } from 'vitest';
import fixture from './benchTokenUsage.fixture.json';
import {
  formatTokenCount,
  parseTelemetryCalls,
  runGenerationIds,
  tokenUsageFromCalls,
  tokenUsageFromHarnessBill,
  tokenUsageFromProviderRecords,
  tokenUsageLine,
  tokenUsageProblem,
  type GenerationRecord,
  type TokenUsage,
} from './benchTokenUsage';

const sonnet = fixture.sonnet;
const records = sonnet.records as unknown as Record<string, GenerationRecord>;
const calls = parseTelemetryCalls(sonnet.telemetry).calls;
const bill = sonnet.agent.billed_cost;

describe('token usage from OpenRouter generation records (real run: Sonnet 5.5, Forge 1.0)', () => {
  it('sums every generation record by the model goose requested and matches OpenRouter exactly', () => {
    const ids = runGenerationIds(calls, bill);
    expect(ids).toHaveLength(51);
    const usage = tokenUsageFromProviderRecords({ calls, ids, records });
    expect(usage).toEqual({
      promptTokens: 2_610_671,
      completionTokens: 90_481,
      cachedTokens: 2_509_317,
      reasoningTokens: 63_297,
      totalTokens: 2_701_152,
      calls: 51,
      source: 'provider-records',
      exact: true,
      missingCalls: 0,
      note: null,
      byModel: [
        {
          model: 'anthropic/claude-sonnet-5.5',
          promptTokens: 2_609_788,
          completionTokens: 90_477,
          cachedTokens: 2_509_317,
          reasoningTokens: 63_297,
          calls: 50,
        },
        {
          model: 'google/gemini-2.5-flash',
          promptTokens: 883,
          completionTokens: 4,
          cachedTokens: 0,
          reasoningTokens: 0,
          calls: 1,
        },
      ],
    });
    // The independent cross-check: OpenRouter's native sums equal goose's own session counters for the entrant.
    const counters = sonnet.agent.usage.sessions[0];
    expect(usage!.byModel[0].promptTokens).toBe(counters.accumulated_input_tokens);
    expect(usage!.byModel[0].completionTokens).toBe(counters.accumulated_output_tokens);
    expect(usage!.byModel[0].cachedTokens).toBe(counters.accumulated_cache_read_tokens);
    expect(tokenUsageProblem(usage!)).toBeNull();
  });

  it('with the records the harness could not read at run end, it is a lower bound equal to the harness bill', () => {
    const lost = new Set((bill.missing as Array<{ id: string }>).map((m) => m.id));
    expect(lost.size).toBe(8);
    const ids = runGenerationIds(calls, bill);
    const partial = Object.fromEntries(ids.map((id) => [id, lost.has(id) ? null : records[id]]));
    const usage = tokenUsageFromProviderRecords({ calls, ids, records: partial })!;
    expect(usage.exact).toBe(false);
    expect(usage.missingCalls).toBe(8);
    expect(usage.calls).toBe(51);
    expect([
      usage.promptTokens,
      usage.cachedTokens,
      usage.completionTokens,
      usage.reasoningTokens,
    ]).toEqual([
      bill.tokens.prompt,
      bill.tokens.cached,
      bill.tokens.completion,
      bill.tokens.reasoning,
    ]);
    expect(usage.note).toMatch(/8 calls of the run have no readable OpenRouter record/);
    expect(tokenUsageProblem(usage)).toBeNull();
  });

  it('a record without a cached count makes cached a lower bound, never a guessed zero', () => {
    const ids = runGenerationIds(calls, bill);
    const first = ids.find((id) => (records[id].native_tokens_cached as number) > 0)!;
    const usage = tokenUsageFromProviderRecords({
      calls,
      ids,
      records: { ...records, [first]: { ...records[first], native_tokens_cached: null } },
    })!;
    expect(usage.exact).toBe(false);
    expect(usage.byModel[0].cachedTokens).toBeNull();
    expect(usage.cachedTokens).toBe(0); // only gemini's known 0 remains
    expect(tokenUsageProblem(usage)).toBeNull();
  });

  it('the harness bill alone does not split an incomplete two-model bill (it cannot prove the split)', () => {
    expect(tokenUsageFromHarnessBill({ calls, billedCost: bill, reason: 'offline' })).toBeNull();
  });
});

describe('token usage from the harness bill (real run: Pareto 26.10, Gauntlet 7.2)', () => {
  const pareto = fixture.pareto;
  const paretoCalls = parseTelemetryCalls(pareto.telemetry).calls;
  it('splits a complete bill by its own reconciliation: entrant counters + the helper call billed outside them', () => {
    const usage = tokenUsageFromHarnessBill({
      calls: paretoCalls,
      billedCost: pareto.modelCost,
      reason: 'no key',
    })!;
    expect(usage).toMatchObject({
      promptTokens: 10_932_017,
      completionTokens: 206_869,
      cachedTokens: 10_598_081,
      reasoningTokens: 0,
      calls: 140,
      exact: true,
      missingCalls: 0,
      source: 'provider-records',
    });
    expect(usage.byModel).toEqual([
      {
        model: 'unbiased/pareto-26.10',
        promptTokens: 10_931_045,
        completionTokens: 206_864,
        cachedTokens: 10_598_081,
        reasoningTokens: 0,
        calls: 139,
      },
      {
        model: 'google/gemini-2.5-flash',
        promptTokens: 972,
        completionTokens: 5,
        cachedTokens: 0,
        reasoningTokens: 0,
        calls: 1,
      },
    ]);
    expect(usage.note).toMatch(/could not be read again at publish \(no key\)/);
    expect(tokenUsageProblem(usage)).toBeNull();
  });

  it("goose's per-call usage: cached known for the entrant only is a lower bound (mixed), helper left null", () => {
    const usage = tokenUsageFromCalls({
      calls: paretoCalls,
      usage: {
        sessions: [
          {
            accumulated_input_tokens: 10_931_045,
            accumulated_output_tokens: 206_864,
            accumulated_cache_read_tokens: 10_598_081,
          },
        ],
      },
      source: 'session-counters',
    })!;
    expect(usage.promptTokens).toBe(10_932_017);
    expect(usage.byModel.map((m) => [m.model, m.cachedTokens, m.calls])).toEqual([
      ['unbiased/pareto-26.10', 10_598_081, 139],
      ['google/gemini-2.5-flash', null, 1],
    ]);
    expect(usage.cachedTokens).toBe(10_598_081);
    expect(usage.reasoningTokens).toBeNull();
    expect(usage.exact).toBe(false);
    expect(tokenUsageProblem(usage)).toBeNull();
  });
});

describe('token usage from a local engine run', () => {
  const line = (prompt: number, completion: number, extra: Record<string, unknown> = {}) =>
    JSON.stringify({
      model: 'local/qwen',
      node: 'local',
      prompt_tokens: prompt,
      completion_tokens: completion,
      usage: true,
      response_id: `chatcmpl-${prompt}`,
      ended: 'completed',
      ...extra,
    });
  it('counts the engine usage per call; a field the engine never reports stays null and the record stays exact', () => {
    const local = parseTelemetryCalls(
      [line(1417, 5), line(2604, 430), line(9000, 70)].join('\n')
    ).calls;
    const usage = tokenUsageFromCalls({
      calls: local,
      // goose's counters cover the agent session, not the session-naming call: no exact owner, so cached is not claimed.
      usage: {
        sessions: [
          {
            accumulated_input_tokens: 11_604,
            accumulated_output_tokens: 500,
            accumulated_cache_read_tokens: 2_000,
          },
        ],
      },
      source: 'engine-telemetry',
    })!;
    expect(usage).toMatchObject({
      promptTokens: 13_021,
      completionTokens: 505,
      cachedTokens: null,
      reasoningTokens: null,
      calls: 3,
      exact: true,
      missingCalls: 0,
      source: 'engine-telemetry',
    });
    expect(tokenUsageProblem(usage)).toBeNull();
  });
  it('a dropped stream is a missing call: counted in calls, not in tokens, not exact', () => {
    const local = parseTelemetryCalls(
      [
        line(100, 1),
        JSON.stringify({ model: 'local/qwen', ended: 'incomplete', usage: false }),
      ].join('\n')
    ).calls;
    const usage = tokenUsageFromCalls({ calls: local, usage: null, source: 'engine-telemetry' })!;
    expect(usage).toMatchObject({ promptTokens: 100, calls: 2, missingCalls: 1, exact: false });
  });
});

describe('the site rule (website benchmark-token-usage.ts) mirrored', () => {
  const good = (): TokenUsage =>
    tokenUsageFromProviderRecords({ calls, ids: runGenerationIds(calls, bill), records })!;
  it('refuses inconsistent records', () => {
    expect(tokenUsageProblem({ ...good(), totalTokens: 1 })).toMatch(/totalTokens/);
    expect(
      tokenUsageProblem({
        ...good(),
        promptTokens: good().promptTokens + 1,
        totalTokens: good().totalTokens + 1,
      })
    ).toMatch(/byModel sums/);
    expect(tokenUsageProblem({ ...good(), missingCalls: 1 })).toMatch(/exact with missing/);
    expect(tokenUsageProblem({ ...good(), source: 'estimate' })).toMatch(/estimate/);
    expect(tokenUsageProblem({ ...good(), cachedTokens: 1 })).toMatch(/cached or reasoning sums/);
    expect(tokenUsageProblem({ ...good(), extra: 1 } as unknown as TokenUsage)).toMatch(
      /unknown field/
    );
    const g = good();
    expect(
      tokenUsageProblem({ ...g, byModel: [g.byModel[0], { ...g.byModel[1], cachedTokens: null }] })
    ).toMatch(/cached or reasoning sums|partly known/);
  });
  it('formats the publish line the site shows', () => {
    expect(tokenUsageLine(good(), 'openrouter')).toBe(
      'Tokens: 2.6M in · 90.5k out · 2.5M cached (exact, OpenRouter records)'
    );
    expect(tokenUsageLine({ ...good(), exact: false }, 'openrouter')).toMatch(
      /\(estimated, OpenRouter records\)$/
    );
    expect([999, 12_345, 206_869, 10_932_017, 1_234_000_000].map(formatTokenCount)).toEqual([
      '999',
      '12.3k',
      '207k',
      '10.9M',
      '1.2B',
    ]);
  });
});

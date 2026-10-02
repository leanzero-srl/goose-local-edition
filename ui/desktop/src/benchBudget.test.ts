import { describe, expect, it } from 'vitest';
import { budgetLines, budgetRowField, maxUsdLaunchEnv, parseMaxUsd } from './benchBudget';

describe('the budget record from the harness (bench_budget.py)', () => {
  it('is carried verbatim from the verdict agent block, and absent when the harness wrote none', () => {
    const budget = { max_calls: 150, calls_used: 150, stopped_by: 'call_budget' };
    expect(budgetRowField({ exit: 0, budget })).toEqual({ budget });
    expect(budgetRowField({ exit: 0 })).toEqual({});
    expect(budgetRowField(undefined)).toEqual({});
  });

  it('a call-budget stop names the budget the record carries', () => {
    const [line] = budgetLines({ max_calls: 150, calls_used: 151, stopped_by: 'call_budget' });
    expect(line.sentence).toBe('Stopped at the 150-call budget');
    expect(line.tone).toBe('stopped');
  });

  it('a wallet stop names the user limit and what OpenRouter had billed', () => {
    const lines = budgetLines({
      max_calls: 150,
      calls_used: 40,
      stopped_by: 'wallet_guard',
      wallet: { status: 'tripped', max_usd: 5, tripped: { spent_usd: 5.0312 } },
    });
    expect(lines).toHaveLength(1);
    expect(lines[0].sentence).toBe('Stopped by your $5.00 limit');
    expect(lines[0].detail).toBe(
      'OpenRouter had billed $5.03 when it stopped · 40 model calls · what it had built was scored'
    );
  });

  it('a run that finished on its own says how much of the budget it used', () => {
    expect(budgetLines({ max_calls: 150, calls_used: 62, stopped_by: 'model_finished' })).toEqual([
      { sentence: 'Finished on its own after 62 of 150 model calls', tone: null, detail: null },
    ]);
  });

  it('a limit that could not be enforced is said loudly, with the reason', () => {
    const lines = budgetLines({
      max_calls: 150,
      calls_used: null,
      stopped_by: 'model_finished',
      wallet: { status: 'unavailable', max_usd: 3, reason: 'Google exposes no per-call bill' },
    });
    expect(lines).toEqual([
      {
        sentence: 'Your $3.00 limit was not enforced on this run',
        tone: 'warn',
        detail: 'Google exposes no per-call bill',
      },
    ]);
  });

  it('says nothing for a run without a record or an unexplained engine exit', () => {
    expect(budgetLines(undefined)).toEqual([]);
    expect(budgetLines({ max_calls: 150, calls_used: 3, stopped_by: 'engine_exit' })).toEqual([]);
  });
});

describe('the spend limit setting', () => {
  it('accepts a positive dollar amount and nothing else', () => {
    expect(parseMaxUsd('')).toEqual({ kind: 'empty' });
    expect(parseMaxUsd(' $2.50 ')).toEqual({ kind: 'ok', usd: 2.5 });
    for (const bad of ['0', '-1', 'abc', '1e3', '5.', 'NaN']) {
      expect(parseMaxUsd(bad)).toEqual({ kind: 'invalid' });
    }
  });

  it('is forwarded to run_build verbatim when set, so a malformed value is refused there, loudly', () => {
    expect(maxUsdLaunchEnv({ BENCH_MAX_USD: 5 })).toEqual({ BENCH_MAX_USD: '5' });
    expect(maxUsdLaunchEnv({ BENCH_MAX_USD: '2.5' })).toEqual({ BENCH_MAX_USD: '2.5' });
    expect(maxUsdLaunchEnv({ BENCH_MAX_USD: 'abc' })).toEqual({ BENCH_MAX_USD: 'abc' });
    expect(maxUsdLaunchEnv({ BENCH_MAX_USD: '' })).toEqual({});
    expect(maxUsdLaunchEnv({ OTHER: 1 })).toEqual({});
    expect(maxUsdLaunchEnv(null)).toEqual({});
  });
});

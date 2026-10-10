import { describe, expect, it } from 'vitest';
import {
  benchElapsed,
  benchModelShortName,
  benchTrayEndOf,
  benchTrayEndTitle,
  benchTrayLines,
  benchTrayTitle,
  lastBenchLine,
  parseBenchBudgetLine,
  parseCallBudget,
  telemetryEntrantCalls,
  type BenchTrayRun,
} from './benchTray';

const START = Date.parse('2026-10-03T10:00:00Z');

const cloudRun = (over: Partial<BenchTrayRun> = {}): BenchTrayRun => ({
  scorerVersion: 'sb-7.2',
  model: 'openai/gpt-6.1-sol',
  nodes: 1,
  phase: 'build',
  rescore: false,
  startedAtMs: START,
  callsUsed: 64,
  callBudget: 150,
  ...over,
});

describe('benchmark tray title', () => {
  it('leads with the model and its calls of the budget while it builds', () => {
    expect(benchTrayTitle(cloudRun())).toBe('Benchmark · gpt-6.1-sol · 64/150');
  });

  it('states calls without a budget it does not know', () => {
    expect(benchTrayTitle(cloudRun({ callBudget: null }))).toBe(
      'Benchmark · gpt-6.1-sol · 64 calls'
    );
  });

  it('says the phase when no calls are counted, and follows the phase', () => {
    expect(benchTrayTitle(cloudRun({ callsUsed: null }))).toBe(
      'Benchmark · gpt-6.1-sol · building'
    );
    expect(benchTrayTitle(cloudRun({ phase: 'boot', callsUsed: null }))).toBe(
      'Benchmark · gpt-6.1-sol · preparing'
    );
    expect(benchTrayTitle(cloudRun({ phase: 'score' }))).toBe('Benchmark · gpt-6.1-sol · scoring');
  });

  it('names a swarm entrant by its nodes, never by an invented model', () => {
    expect(benchTrayTitle(cloudRun({ model: null, nodes: 3, callsUsed: null }))).toBe(
      'Benchmark · Swarm 3 nodes · building'
    );
  });
});

describe('benchmark tray menu lines', () => {
  it('carries the era by its display name, the model, phase, elapsed and calls', () => {
    expect(benchTrayLines(cloudRun(), START + 64 * 60_000 + 30_000)).toEqual([
      'Benchmark running: Gauntlet 7.2',
      'Model: openai/gpt-6.1-sol',
      'Phase: Model build',
      'Elapsed: 1h 04m',
      'Calls: 64 of 150',
    ]);
  });

  it('omits the calls line when nothing was counted, and names Forge and a rescore', () => {
    expect(
      benchTrayLines(
        cloudRun({ scorerVersion: 'forge-1.0', phase: 'score', rescore: true, callsUsed: null }),
        START + 59_000
      )
    ).toEqual([
      'Rescoring: Forge 1.0',
      'Model: openai/gpt-6.1-sol',
      'Phase: Scoring',
      'Elapsed: 0m',
    ]);
  });

  it('shows a swarm by its nodes', () => {
    expect(benchTrayLines(cloudRun({ model: null, nodes: 1, callsUsed: null }), START)[1]).toBe(
      'Swarm: 1 node'
    );
  });
});

describe('how a run ended', () => {
  const facts = { scorerVersion: 'sb-7.2', model: 'openai/gpt-6.1-sol', nodes: 1, rescore: false };

  it('reads a scored row: its score, its model and its bill', () => {
    const last = benchTrayEndOf(
      facts,
      {
        row: {
          score: 0.41234,
          modelId: 'openai/gpt-6.1-sol',
          scorerVersion: 'sb-7.2',
          nodes: 1,
          provider: 'openrouter',
          billedCost: { status: 'complete', billed_usd: 2.5 },
        },
      },
      START
    );
    expect(lastBenchLine(last)).toBe(
      'Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · 41.2% · $2.5000 billed'
    );
    expect(benchTrayEndTitle(last)).toBe('Benchmark · gpt-6.1-sol · 41.2%');
  });

  it('prints a Forge 2.0 score in the app’s one format for it — four decimals, as leanzero.net does', () => {
    const last = benchTrayEndOf(
      { ...facts, scorerVersion: 'forge-2.0' },
      { row: { score: 0.745, scorerVersion: 'forge-2.0', provider: 'openrouter' } },
      START
    );
    expect(lastBenchLine(last)).toBe('Last benchmark: gpt-6.1-sol · Forge 2.0 · 0.7450');
    expect(benchTrayEndTitle(last)).toBe('Benchmark · gpt-6.1-sol · 0.7450');
  });

  it('states a partial bill as a floor and omits an absent one', () => {
    const partial = benchTrayEndOf(
      facts,
      {
        row: {
          score: 0.5,
          provider: 'openrouter',
          billedCost: { status: 'incomplete', billed_usd: 1, missing: ['g1'] },
        },
      },
      START
    );
    expect(lastBenchLine(partial)).toBe(
      'Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · 50.0% · ≥ $1.0000 billed'
    );
    const none = benchTrayEndOf(facts, { row: { score: 0.5, provider: 'openrouter' } }, START);
    expect(lastBenchLine(none)).toBe('Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · 50.0%');
  });

  it('says did not finish for an error and cancelled for a cancel — never a score', () => {
    expect(lastBenchLine(benchTrayEndOf(facts, { error: 'no verdict produced.' }, START))).toBe(
      'Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · did not finish'
    );
    expect(lastBenchLine(benchTrayEndOf(facts, { cancelled: true }, START))).toBe(
      'Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · cancelled'
    );
    expect(lastBenchLine(benchTrayEndOf(facts, { row: { score: 'NaN' } }, START))).toBe(
      'Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · did not finish'
    );
  });

  it("names a rescore's failure as the scoring's", () => {
    expect(
      lastBenchLine(
        benchTrayEndOf({ ...facts, rescore: true }, { error: 'x', details: 'y' }, START)
      )
    ).toBe('Last benchmark: gpt-6.1-sol · Gauntlet 7.2 · scoring did not finish');
  });

  it('keeps a swarm a swarm even when its row derives a pool model', () => {
    const last = benchTrayEndOf(
      { scorerVersion: 'sb-7.2', model: null, nodes: 3, rescore: false },
      { row: { score: 0.3, modelId: 'qwen3.8-27b', nodes: 2 } },
      START
    );
    expect(lastBenchLine(last)).toBe('Last benchmark: Swarm 2 nodes · Gauntlet 7.2 · 30.0%');
  });
});

describe('harness and telemetry reads', () => {
  it("parses run_build's BENCH_BUDGET record and refuses anything else", () => {
    expect(
      parseBenchBudgetLine(
        'BENCH_BUDGET {"max_calls": 150, "calls_used": 151, "stopped_by": "call_budget"}'
      )
    ).toEqual({ callsUsed: 151, maxCalls: 150 });
    expect(parseBenchBudgetLine('BENCH_BUDGET {"calls_used": null}')).toEqual({
      callsUsed: null,
      maxCalls: null,
    });
    expect(parseBenchBudgetLine('BENCH_BUDGET {broken')).toBeNull();
    expect(parseBenchBudgetLine('BENCH_PHASE {"phase": "score"}')).toBeNull();
  });

  it("counts only the entrant model's calls, as bench_budget.entrant_calls does", () => {
    const lines = [
      '{"model": "openai/gpt-6.1-sol", "response_id": "gen-1"}',
      '{"model": "openai/gpt-5-nano", "response_id": "gen-2"}',
      'not json',
      '',
      '{"model": "openai/gpt-6.1-sol", "response_id": "gen-3"}',
    ].join('\n');
    expect(telemetryEntrantCalls(lines, 'openai/gpt-6.1-sol')).toBe(2);
  });

  it('reads the call budget only as a positive integer', () => {
    expect(parseCallBudget('150\n')).toBe(150);
    expect(parseCallBudget('')).toBeNull();
    expect(parseCallBudget('Traceback')).toBeNull();
    expect(parseCallBudget('0')).toBeNull();
  });

  it('shortens a provider-qualified model id to its own name', () => {
    expect(benchModelShortName('openai/gpt-6.1-sol')).toBe('gpt-6.1-sol');
    expect(benchModelShortName('gemini-3.8-flash')).toBe('gemini-3.8-flash');
    expect(benchModelShortName('odd/')).toBe('odd/');
  });

  it('formats elapsed at minute grain', () => {
    expect(benchElapsed(0)).toBe('0m');
    expect(benchElapsed(59 * 60_000)).toBe('59m');
    expect(benchElapsed(2 * 3600_000 + 5 * 60_000)).toBe('2h 05m');
  });
});

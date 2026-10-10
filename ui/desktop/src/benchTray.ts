import { billedCostLine } from './benchBilledCost';
import type { BenchmarkPhase } from './benchPhase';
import { eraDisplayName, scoreText } from './components/benchmark/baselines';

/**
 * The menu-bar tray's benchmark presence (owner 2026-10-03: "when benchmarks run the status bar of
 * the application does not display the current status! AT ALL"). Every value comes from main's own
 * benchmark bookkeeping — `activeBenchRun`, the harness's stdout markers, the entrant's telemetry sink
 * and the scored row — and a value main does not know is omitted, never filled with a placeholder.
 */
export interface BenchTrayRun {
  /** The era the run is scored against (`sb-7.2`, `forge-1.0`); shown by its display name. */
  scorerVersion: string;
  /** The single-model entrant's model id as launched; null for a swarm entrant. */
  model: string | null;
  nodes: number;
  phase: BenchmarkPhase;
  /** A retry-scoring run re-grades a saved build: it has no build phase and no calls of its own. */
  rescore: boolean;
  startedAtMs: number;
  /** Provider calls the entrant made on its model, counted from its telemetry sink or the harness's
   *  final BENCH_BUDGET record; null until either exists. */
  callsUsed: number | null;
  /** The harness's call budget for this tier (isolated_tiers call_budget), known only for a budgeted entrant. */
  callBudget: number | null;
}

export type BenchTrayEnd = 'scored' | 'did_not_finish' | 'cancelled';

export interface BenchTrayLast {
  scorerVersion: string;
  model: string | null;
  nodes: number;
  /** A retry-scoring run: its failure is the scoring's, not the build's. */
  rescore: boolean;
  end: BenchTrayEnd;
  score: number | null;
  /** The billed amount as the Benchmark view states it ("$1.2345", "≥ $1.2345"); null when no bill. */
  billed: string | null;
  endedAtMs: number;
}

/** The view's own phase words (BenchmarkActivityPanel's stepper). */
const PHASE_LABEL: Record<BenchmarkPhase, string> = {
  boot: 'Prepare',
  build: 'Model build',
  score: 'Scoring',
  done: 'Finalize',
};

const PHASE_WORD: Record<BenchmarkPhase, string> = {
  boot: 'preparing',
  build: 'building',
  score: 'scoring',
  done: 'finalizing',
};

const finiteCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0;

/** A provider-qualified id shown by its own name: `openai/gpt-6.1-sol` → `gpt-6.1-sol`. */
export function benchModelShortName(modelId: string): string {
  const trimmed = modelId.trim();
  const slash = trimmed.lastIndexOf('/');
  return slash >= 0 && slash < trimmed.length - 1 ? trimmed.slice(slash + 1) : trimmed;
}

function entrantName(model: string | null, nodes: number): string {
  if (model) return benchModelShortName(model);
  return `Swarm ${nodes} node${nodes === 1 ? '' : 's'}`;
}

/** Elapsed at minute grain, so the menu changes once a minute rather than every tick. */
export function benchElapsed(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60000));
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h > 0 ? `${h}h ${String(m).padStart(2, '0')}m` : `${m}m`;
}

function callsProgress(run: BenchTrayRun): string | null {
  if (run.callsUsed == null) return null;
  return run.callBudget != null
    ? `${run.callsUsed}/${run.callBudget}`
    : `${run.callsUsed} call${run.callsUsed === 1 ? '' : 's'}`;
}

function endText(last: BenchTrayLast): string {
  if (last.end === 'scored' && last.score != null) return scoreText(last.score, last.scorerVersion);
  const what = last.end === 'cancelled' ? 'cancelled' : 'did not finish';
  return last.rescore ? `scoring ${what}` : what;
}

/** The menu-bar title while a run is live: the build shows its calls when they are counted. */
export function benchTrayTitle(run: BenchTrayRun): string {
  const progress = run.phase === 'build' ? callsProgress(run) : null;
  return `Benchmark · ${entrantName(run.model, run.nodes)} · ${progress ?? PHASE_WORD[run.phase]}`;
}

/** The title a just-ended run holds before the engine's status returns. */
export function benchTrayEndTitle(last: BenchTrayLast): string {
  return `Benchmark · ${entrantName(last.model, last.nodes)} · ${endText(last)}`;
}

/** The menu's benchmark lines while a run is live, top to bottom. */
export function benchTrayLines(run: BenchTrayRun, nowMs: number): string[] {
  const lines = [
    `${run.rescore ? 'Rescoring' : 'Benchmark running'}: ${eraDisplayName(run.scorerVersion)}`,
    run.model ? `Model: ${run.model}` : `Swarm: ${run.nodes} node${run.nodes === 1 ? '' : 's'}`,
    `Phase: ${PHASE_LABEL[run.phase]}`,
    `Elapsed: ${benchElapsed(nowMs - run.startedAtMs)}`,
  ];
  if (run.callsUsed != null)
    lines.push(
      run.callBudget != null
        ? `Calls: ${run.callsUsed} of ${run.callBudget}`
        : `Calls: ${run.callsUsed}`
    );
  return lines;
}

/** The one line a finished run leaves in the menu until the next run starts. */
export function lastBenchLine(last: BenchTrayLast): string {
  const parts = [
    entrantName(last.model, last.nodes),
    eraDisplayName(last.scorerVersion),
    endText(last),
  ];
  if (last.billed) parts.push(`${last.billed} billed`);
  return `Last benchmark: ${parts.join(' · ')}`;
}

/** The `benchmark-finished` broadcast main sends the view. */
export interface BenchFinishedPayload {
  row?: unknown;
  cancelled?: boolean;
  error?: string;
  details?: string;
}

/**
 * How a run ended, from the `benchmark-finished` payload main broadcasts: a row with a numeric score
 * is scored (its model and bill read from the row), `cancelled` is a cancel, anything else did not
 * finish. The row's own model id wins over the launch's, which it records verbatim.
 */
export function benchTrayEndOf(
  run: Pick<BenchTrayRun, 'scorerVersion' | 'model' | 'nodes' | 'rescore'>,
  payload: BenchFinishedPayload,
  endedAtMs: number
): BenchTrayLast {
  const base = {
    model: run.model,
    nodes: run.nodes,
    scorerVersion: run.scorerVersion,
    rescore: run.rescore,
    endedAtMs,
  };
  const row =
    payload.row && typeof payload.row === 'object'
      ? (payload.row as Record<string, unknown>)
      : null;
  if (row && typeof row.score === 'number' && Number.isFinite(row.score)) {
    const provider = typeof row.provider === 'string' ? row.provider : null;
    return {
      ...base,
      ...(typeof row.modelId === 'string' && row.modelId.trim() && run.model
        ? { model: row.modelId.trim() }
        : {}),
      ...(typeof row.scorerVersion === 'string' ? { scorerVersion: row.scorerVersion } : {}),
      ...(typeof row.nodes === 'number' ? { nodes: row.nodes } : {}),
      end: 'scored',
      score: row.score,
      billed: billedCostLine(row.billedCost, provider)?.amount ?? null,
    };
  }
  return {
    ...base,
    end: payload.cancelled === true ? 'cancelled' : 'did_not_finish',
    score: null,
    billed: null,
  };
}

/** run_build's `BENCH_BUDGET {...}` record (printed when the entrant exits): its calls and budget. */
export function parseBenchBudgetLine(
  line: string
): { callsUsed: number | null; maxCalls: number | null } | null {
  if (!line.startsWith('BENCH_BUDGET ')) return null;
  try {
    const value: unknown = JSON.parse(line.slice('BENCH_BUDGET '.length));
    if (!value || typeof value !== 'object') return null;
    const record = value as Record<string, unknown>;
    return {
      callsUsed: finiteCount(record.calls_used) ? record.calls_used : null,
      maxCalls: finiteCount(record.max_calls) ? record.max_calls : null,
    };
  } catch {
    return null;
  }
}

/**
 * Entrant calls in whole telemetry lines — bench_budget.entrant_calls's rule: a line counts when its
 * `model` is the entrant's (goose's session-title call on the fast model does not).
 */
export function telemetryEntrantCalls(wholeLines: string, model: string): number {
  let calls = 0;
  for (const line of wholeLines.split('\n')) {
    if (!line.trim()) continue;
    try {
      const entry: unknown = JSON.parse(line);
      if (entry && typeof entry === 'object' && (entry as { model?: unknown }).model === model)
        calls += 1;
    } catch {
      /* a malformed line is not a call */
    }
  }
  return calls;
}

/** The harness's call budget as the payload's Python prints it (readBenchCallBudget). */
export function parseCallBudget(stdout: string): number | null {
  const value = Number(stdout.trim().split('\n').pop());
  return Number.isInteger(value) && value > 0 ? value : null;
}

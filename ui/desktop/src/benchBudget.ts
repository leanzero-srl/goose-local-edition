/**
 * How a single-model benchmark run ended against its budget, as the harness recorded it
 * (`agent.budget`, evals/swarm-bench/bench/bench_budget.py): every single-model entrant gets the same
 * published call budget, after which goose ends the session and the harness scores what exists; the
 * operator's OpenRouter wallet guard (BENCH_MAX_USD) stops a run once the billed spend reaches the
 * limit and that run is scored too. The numbers shown come from the record — the policy lives in
 * bench_budget.py alone, never here.
 */
export interface BenchBudget {
  max_calls?: number;
  calls_used?: number | null;
  stopped_by?: 'call_budget' | 'model_finished' | 'wallet_guard' | 'engine_exit' | string;
  wallet?: {
    status?: 'tripped' | 'armed' | 'unavailable' | 'failed' | string;
    max_usd?: number;
    spent_usd_seen?: number;
    reason?: string;
    tripped?: { spent_usd?: number };
  };
}

/** The goose config key the Benchmark view stores the wallet limit under, and run_build reads. */
export const BENCH_MAX_USD_KEY = 'BENCH_MAX_USD';

/** The result-row field persisted from the verdict's `agent` block: verbatim, or nothing. */
export function budgetRowField(agent: unknown): { budget?: unknown } {
  return agent && typeof agent === 'object' && 'budget' in agent
    ? { budget: (agent as { budget: unknown }).budget }
    : {};
}

const finite = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

const dollars = (value: number) => `$${value.toFixed(2)}`;

export interface BudgetLine {
  sentence: string;
  tone: 'warn' | 'stopped' | null;
  detail: string | null;
}

/**
 * What the view says about the run's budget: why the harness stopped it, how many calls it used when
 * it finished on its own, and a limit that could not be enforced. Nothing for a run with no record.
 */
export function budgetLines(value: unknown): BudgetLine[] {
  if (!value || typeof value !== 'object') return [];
  const budget = value as BenchBudget;
  const lines: BudgetLine[] = [];
  const used = finite(budget.calls_used) ? budget.calls_used : null;
  const wallet = budget.wallet && typeof budget.wallet === 'object' ? budget.wallet : null;
  if (budget.stopped_by === 'call_budget' && finite(budget.max_calls)) {
    lines.push({
      sentence: `Stopped at the ${budget.max_calls}-call budget`,
      tone: 'stopped',
      detail: 'The harness scored what the model had built by then.',
    });
  } else if (budget.stopped_by === 'wallet_guard' && wallet && finite(wallet.max_usd)) {
    const spent = wallet.tripped?.spent_usd;
    lines.push({
      sentence: `Stopped by your ${dollars(wallet.max_usd)} limit`,
      tone: 'stopped',
      detail: [
        finite(spent) ? `OpenRouter had billed ${dollars(spent)} when it stopped` : null,
        used != null ? `${used} model call${used === 1 ? '' : 's'}` : null,
        'what it had built was scored',
      ]
        .filter(Boolean)
        .join(' · '),
    });
  } else if (budget.stopped_by === 'model_finished' && used != null && finite(budget.max_calls)) {
    lines.push({
      sentence: `Finished on its own after ${used} of ${budget.max_calls} model calls`,
      tone: null,
      detail: null,
    });
  }
  if (wallet && (wallet.status === 'unavailable' || wallet.status === 'failed')) {
    lines.push({
      sentence: finite(wallet.max_usd)
        ? `Your ${dollars(wallet.max_usd)} limit was not enforced on this run`
        : 'Your spend limit was not enforced on this run',
      tone: 'warn',
      detail: typeof wallet.reason === 'string' && wallet.reason ? wallet.reason : null,
    });
  }
  return lines;
}

/** A typed limit: a positive dollar amount, empty for no limit, or invalid (never saved). */
export function parseMaxUsd(text: string):
  | { kind: 'empty' }
  | { kind: 'ok'; usd: number }
  | {
      kind: 'invalid';
    } {
  const trimmed = text.trim().replace(/^\$/, '');
  if (trimmed === '') return { kind: 'empty' };
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return { kind: 'invalid' };
  const usd = Number(trimmed);
  return Number.isFinite(usd) && usd > 0 ? { kind: 'ok', usd } : { kind: 'invalid' };
}

/**
 * The BENCH_MAX_USD a run_build launch carries, from the parsed goose config.yaml: the stored value
 * verbatim when one is set. A malformed value is forwarded too, so run_build refuses the launch and
 * says why, before anything runs — it is never dropped into an unguarded run.
 */
export function maxUsdLaunchEnv(config: unknown): { BENCH_MAX_USD?: string } {
  if (!config || typeof config !== 'object') return {};
  const raw = (config as Record<string, unknown>)[BENCH_MAX_USD_KEY];
  if (raw === undefined || raw === null || (typeof raw === 'string' && raw.trim() === ''))
    return {};
  return { BENCH_MAX_USD: typeof raw === 'string' ? raw : JSON.stringify(raw) };
}

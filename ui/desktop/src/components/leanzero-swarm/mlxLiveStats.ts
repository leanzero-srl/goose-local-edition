import type { MlxLiveStatusResult } from '../../utils/mlxLiveStatus';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import type { MlxServing } from '../../utils/mlxServing';

/**
 * The state tile's live instrument, as pure functions over MEASURED inputs: Rapid-MLX's own
 * `/v1/status` body (read by main, IPC `mlx-live-status`), the sidecar's memory numbers and the
 * models list's on-disk sizes. Nothing here animates, estimates or ticks on a clock — every figure
 * the tile draws is one of these values or a difference of two of them.
 *
 * Field names verified against the running engine (Rapid-MLX v0.14.3-lz.1, 2026-09-23) and its
 * source (`rapid_mlx/routes/health.py` `/v1/status`, `scheduler.py get_running_requests_info`).
 */

const GIB = 1024 * 1024 * 1024;

/**
 * How often the engine is read while it runs — the Providers view's status poll and main's tray
 * monitor share it, so the tile and the menu bar tick together.
 */
export const MLX_STATUS_POLL_MS = 2000;

export interface MlxLiveRequest {
  /** `request_id` — the row's stable identity across polls. */
  id: string;
  /** `running` | `waiting` as the engine reports it. */
  status: string;
  /** `prefill` (no token out yet) | `generation` | `queued`. */
  phase: string;
  elapsedS: number | null;
  promptTokens: number | null;
  completionTokens: number;
  maxTokens: number | null;
  tokensPerSecond: number | null;
  /** Seconds from ARRIVAL to the first token (queue wait included); null until that token lands. */
  ttftS: number | null;
  /** Prompt tokens the prefix cache supplied, so the engine never computed them. */
  cachedTokens: number | null;
  /**
   * How far into the prompt the prefill is, and the prefill's own rate — reported by the
   * distributed engine's rank 0 (rank_live.py `prefilled_tokens`, `prompt_tokens_per_second`);
   * Rapid-MLX's single engine reports neither, so both stay null there.
   */
  prefilledTokens: number | null;
  promptTps: number | null;
}

export interface MlxLiveStats {
  /** The engine's own word: `generating` | `idle` | `not_loaded`. */
  engineStatus: string;
  /** The engine's own clock — orders samples and detects a restart. */
  uptimeS: number | null;
  /**
   * Σ over generating requests of tokens / seconds-since-first-token, recomputed per read while a
   * request generates — and STICKY at the last value once none does (scheduler.get_stats), so it
   * is live only while `activity` is `generating`.
   */
  generationTps: number | null;
  numRunning: number | null;
  numWaiting: number | null;
  activeMemoryGb: number | null;
  cacheHitRate: number | null;
  /** Prompt tokens the prefix cache supplied across the engine's life (`cache.tokens_saved`). */
  cacheTokensSaved: number | null;
  /** Lifetime counters since the engine process started (they reset with `uptimeS`). */
  totalRequests: number | null;
  totalPromptTokens: number | null;
  totalCompletionTokens: number | null;
  requests: MlxLiveRequest[];
}

export type MlxLiveRead = { ok: true; stats: MlxLiveStats } | { ok: false; detail: string };

function num(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

function obj(v: unknown): Record<string, unknown> | null {
  return v != null && typeof v === 'object' && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : null;
}

/** A `/v1/status` body → typed stats, or a named reason it is not one. Absent fields stay null. */
export function parseMlxLiveStatus(body: unknown): MlxLiveRead {
  const root = obj(body);
  if (!root || typeof root.status !== 'string') {
    return { ok: false, detail: 'the engine answered, but not with a Rapid-MLX status body' };
  }
  const metal = obj(root.metal);
  const cache = obj(root.cache);
  const rawRequests = Array.isArray(root.requests) ? root.requests : [];
  const requests: MlxLiveRequest[] = [];
  rawRequests.forEach((raw, i) => {
    const r = obj(raw);
    if (!r) return;
    requests.push({
      id: typeof r.request_id === 'string' && r.request_id ? r.request_id : `request-${i}`,
      status: typeof r.status === 'string' ? r.status : 'running',
      phase: typeof r.phase === 'string' ? r.phase : 'unknown',
      elapsedS: num(r.elapsed_s),
      promptTokens: num(r.prompt_tokens),
      completionTokens: num(r.completion_tokens) ?? 0,
      maxTokens: num(r.max_tokens),
      tokensPerSecond: num(r.tokens_per_second),
      ttftS: num(r.ttft_s),
      cachedTokens: num(r.cached_tokens),
      prefilledTokens: num(r.prefilled_tokens),
      promptTps: num(r.prompt_tokens_per_second),
    });
  });
  return {
    ok: true,
    stats: {
      engineStatus: root.status,
      uptimeS: num(root.uptime_s),
      generationTps: num(root.generation_tps),
      numRunning: num(root.num_running),
      numWaiting: num(root.num_waiting),
      activeMemoryGb: metal ? num(metal.active_memory_gb) : null,
      cacheHitRate: cache ? num(cache.hit_rate) : null,
      cacheTokensSaved: cache ? num(cache.tokens_saved) : null,
      totalRequests: num(root.total_requests_processed),
      totalPromptTokens: num(root.total_prompt_tokens),
      totalCompletionTokens: num(root.total_completion_tokens),
      requests,
    },
  };
}

/** What the engine is doing right now, from the requests' own phases. */
export type MlxActivity = 'generating' | 'prefill' | 'queued' | 'idle' | 'not_loaded';

export function mlxActivity(stats: MlxLiveStats): MlxActivity {
  if (stats.engineStatus === 'not_loaded') return 'not_loaded';
  const running = stats.requests.filter((r) => r.status !== 'waiting');
  if (running.some((r) => r.phase === 'generation')) return 'generating';
  if (running.some((r) => r.phase === 'prefill')) return 'prefill';
  if (stats.requests.length > 0 || (stats.numWaiting ?? 0) > 0) return 'queued';
  return 'idle';
}

/**
 * The decode rate this instant, from the generating requests' OWN rates — only those that have
 * written at least two tokens, because a rate needs an interval between tokens. The engine's
 * aggregate `generation_tps` divides by seconds-since-first-token, so a one-token request (a title,
 * a probe) reads as 1,048,576 tok/s — measured 2026-09-23 after three tiny requests totalling 4
 * tokens — and it stays sticky at that value once idle; it is never displayed.
 */
export function liveDecodeTps(stats: MlxLiveStats): number {
  if (mlxActivity(stats) !== 'generating') return 0;
  return stats.requests
    .filter((r) => r.phase === 'generation' && r.completionTokens >= 2)
    .reduce((sum, r) => sum + (r.tokensPerSecond ?? 0), 0);
}

/**
 * The prompt-reading rate NOW: the sum of the engine-reported prefill rates of the requests still
 * reading (`promptTps`). Only the distributed engine reports one; on the single engine this is 0.
 */
export function readingNowTps(stats: MlxLiveStats): number {
  return stats.requests
    .filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
    .reduce((sum, r) => sum + (r.promptTps ?? 0), 0);
}

/**
 * The prompt-reading (prefill) rate. While a request is still reading and the engine reports its
 * prefill rate (the distributed engine does), that live rate. Otherwise the rate achieved on the
 * request whose first token landed most recently: the engine's own prefill rate when it reports one,
 * else (prompt − cached) / ttft_s — the tokens it actually COMPUTED over the time it took to compute
 * them, the same accounting oMLX's usage history calls "prefill speed". Cached tokens are excluded
 * because a prefix-cache hit reads as thousands of tok/s it never did (the engine's own aggregate
 * `prompt_tps` counts them, and is sticky like `generation_tps`, so it is never shown). ttft_s runs
 * from ARRIVAL, so time queued behind another request is inside it: the figure is conservative,
 * never flattering. The single engine reports no per-request progress, so there this is 0 until
 * some request has written its first token.
 */
export function measuredPrefillTps(stats: MlxLiveStats): number {
  const now = readingNowTps(stats);
  if (now > 0) return now;
  let best: { firstTokenAge: number; tps: number } | null = null;
  for (const r of stats.requests) {
    if (r.phase !== 'generation' || r.ttftS == null || r.ttftS <= 0 || r.promptTokens == null) {
      continue;
    }
    const computed = r.promptTokens - (r.cachedTokens ?? 0);
    if (computed <= 0) continue;
    // Seconds since the first token: the smallest is the prefill that finished last.
    const firstTokenAge = (r.elapsedS ?? r.ttftS) - r.ttftS;
    if (best == null || firstTokenAge < best.firstTokenAge) {
      best = { firstTokenAge, tps: r.promptTps ?? computed / r.ttftS };
    }
  }
  return best?.tps ?? 0;
}

/** One run (one request) this reader saw, with the rates the engine measured for it. */
export interface RunRates {
  /** Its writing rate at the last read that caught it generating (the engine's own per-request rate). */
  decodeTps: number | null;
  /** Its prompt's uncached tokens over its time to first token — the same measure as the tile's. */
  prefillTps: number | null;
}

/**
 * Every run this reader saw on ONE engine life, by request id — what the idle tile and the tray
 * summarise as a median and a slowest–fastest range instead of one "last run" (the owner, 3.0.33:
 * "this only shows the last reported value instead of showing a median"). A run shorter than one
 * read is never seen; that is the reader's limit, stated by the count shown beside the median.
 */
export interface RateBook {
  uptimeS: number | null;
  runs: ReadonlyMap<string, RunRates>;
}

export const EMPTY_BOOK: RateBook = { uptimeS: null, runs: new Map() };

function runPrefillTps(r: MlxLiveRequest): number | null {
  // The distributed engine's rank 0 reports the prompt's own rate while it reads.
  if (r.phase === 'prefill')
    return r.status !== 'waiting' && (r.promptTps ?? 0) > 0 ? r.promptTps : null;
  if (r.phase !== 'generation' || r.ttftS == null || r.ttftS <= 0 || r.promptTokens == null) {
    return null;
  }
  const computed = r.promptTokens - (r.cachedTokens ?? 0);
  if (computed <= 0) return null;
  return r.promptTps ?? computed / r.ttftS;
}

/** Fold one read into the book; an engine whose uptime went backwards starts a new one. */
export function advanceRateBook(prev: RateBook, stats: MlxLiveStats): RateBook {
  const restarted = prev.uptimeS != null && stats.uptimeS != null && stats.uptimeS < prev.uptimeS;
  const runs = new Map(restarted ? [] : prev.runs);
  for (const r of stats.requests) {
    const decode =
      r.phase === 'generation' && r.completionTokens >= 2 && (r.tokensPerSecond ?? 0) > 0
        ? r.tokensPerSecond
        : null;
    const prefill = runPrefillTps(r);
    if (decode == null && prefill == null) continue;
    const had = runs.get(r.id);
    runs.set(r.id, {
      decodeTps: decode ?? had?.decodeTps ?? null,
      prefillTps: prefill ?? had?.prefillTps ?? null,
    });
  }
  return { uptimeS: stats.uptimeS ?? (restarted ? null : prev.uptimeS), runs };
}

export interface RateSpread {
  median: number;
  min: number;
  max: number;
  runs: number;
}

export function rateSpread(values: readonly number[]): RateSpread | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
  return { median, min: sorted[0], max: sorted[sorted.length - 1], runs: sorted.length };
}

/** The book's writing and reading spreads — each over the runs that measured it. */
export function bookSpreads(book: RateBook): {
  writing: RateSpread | null;
  reading: RateSpread | null;
} {
  const runs = [...book.runs.values()];
  return {
    writing: rateSpread(runs.flatMap((r) => (r.decodeTps != null ? [r.decodeTps] : []))),
    reading: rateSpread(runs.flatMap((r) => (r.prefillTps != null ? [r.prefillTps] : []))),
  };
}

export interface TpsSample {
  uptimeS: number;
  tps: number;
}

/** The sparkline's window: 60 reads at the 2-second cadence is the last two minutes. */
export const SPARK_WINDOW = 60;

/**
 * Append one read to the sparkline history. The engine's own uptime orders the samples: a repeat
 * of the same uptime replaces the last point (two reads of one engine tick), and an uptime that went
 * BACKWARDS is a restarted engine, so the old history is dropped rather than drawn into the new one.
 */
export function pushSample(
  history: readonly TpsSample[],
  sample: TpsSample,
  window = SPARK_WINDOW
): TpsSample[] {
  const last = history[history.length - 1];
  if (last && sample.uptimeS < last.uptimeS) return [sample];
  const base = last && sample.uptimeS === last.uptimeS ? history.slice(0, -1) : history;
  return [...base, sample].slice(-window);
}

/** The sparkline's polyline points in a `width`×`height` box; the top is the window's own peak. */
export function sparklinePoints(samples: readonly TpsSample[], width: number, height: number) {
  if (samples.length < 2) return null;
  const peak = Math.max(...samples.map((s) => s.tps));
  const step = width / (samples.length - 1);
  return samples
    .map((s, i) => {
      const y = peak > 0 ? height - (s.tps / peak) * height : height;
      return `${(i * step).toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
}

/**
 * Memory watch across a mount: the highest free memory seen since the mount began is the baseline,
 * and what the engine has claimed is that peak minus free now. `sawStart` is true only when this
 * view saw the engine BEFORE it flipped to mounting — a view opened mid-mount has no honest baseline
 * and draws no fraction.
 */
export interface MountWatch {
  modelId: string;
  peakFreeGb: number;
  sawStart: boolean;
}

export function advanceMountWatch(
  watch: MountWatch | null,
  modelId: string,
  freeGb: number,
  settledFreeGb: number | null
): MountWatch {
  if (!watch || watch.modelId !== modelId) {
    return {
      modelId,
      peakFreeGb: settledFreeGb != null ? Math.max(settledFreeGb, freeGb) : freeGb,
      sawStart: settledFreeGb != null,
    };
  }
  return { ...watch, peakFreeGb: Math.max(watch.peakFreeGb, freeGb) };
}

export interface MountFill {
  modelGb: number;
  claimedGb: number;
  /** 0..1, or null when the baseline was never seen (no fraction is drawn). */
  fraction: number | null;
}

export function mountFill(
  watch: MountWatch | null,
  freeGb: number,
  modelBytes: number | null
): MountFill | null {
  if (modelBytes == null || modelBytes <= 0) return null;
  const modelGb = modelBytes / GIB;
  if (!watch || !watch.sawStart) return { modelGb, claimedGb: 0, fraction: null };
  const claimedGb = Math.max(0, watch.peakFreeGb - freeGb);
  return { modelGb, claimedGb, fraction: Math.min(1, claimedGb / modelGb) };
}

/**
 * A single-engine mount in flight as the SIDECAR measured it (`status.load`): where the start is —
 * `makingRoom` (macOS reclaiming memory before the gate judges again), `starting` (the process
 * runs, no load yet), `loading`, `warming` (weights in, compiling kernels) — and the engine
 * process's resident bytes against the model's bytes on disk, which a finished load holds (measured
 * 0.985–0.994×). THE BINDING POINT for the backend's `EngineLoad`; `null` = an older backend
 * without it (the tile then uses the memory watch above).
 */
export interface SingleLoad {
  phase: string;
  residentBytes: number | null;
  weightsBytes: number;
}

export function singleLoad(status: object | null): SingleLoad | null {
  const load = (status as { load?: unknown } | null)?.load;
  if (load == null || typeof load !== 'object') return null;
  const l = load as Record<string, unknown>;
  if (typeof l.phase !== 'string' || typeof l.weightsBytes !== 'number') return null;
  return {
    phase: l.phase,
    residentBytes: typeof l.residentBytes === 'number' ? l.residentBytes : null,
    weightsBytes: l.weightsBytes,
  };
}

/**
 * What a mount would cost, READ from the sidecar's one fit rule (crates/goose-sidecar/src/fit.rs,
 * `mlxEngine/status` `mountFit`) — never recomputed here: the tile's old copy of the gate (8 GiB /
 * 10% / 4 GiB) said "fits" where the mount and the placement badge judged on other numbers.
 */
export type FitVerdict = 'fits' | 'tight' | 'no-fit';

export interface MountCost {
  /** What the mount needs: the weights plus KV for the smallest useful context. */
  modelGb: number;
  freeGb: number;
  /** What the rule keeps back from free memory: the margin, or more where the GPU ceiling binds. */
  reserveGb: number;
  /** The budget left after the mount; negative = short by that much. */
  spareGb: number;
  verdict: FitVerdict;
}

const VERDICT: Record<string, FitVerdict> = { allow: 'fits', warn: 'tight', block: 'no-fit' };

/** The sidecar's verdict in the tile's units; null for a verdict word it does not know. */
export function mountCostOf(fit: {
  verdict: string;
  needBytes: number;
  availableBytes: number;
  budgetBytes: number;
}): MountCost | null {
  const verdict = VERDICT[fit.verdict];
  if (!verdict) return null;
  return {
    modelGb: fit.needBytes / GIB,
    freeGb: fit.availableBytes / GIB,
    reserveGb: (fit.availableBytes - fit.budgetBytes) / GIB,
    spareGb: (fit.budgetBytes - fit.needBytes) / GIB,
    verdict,
  };
}

/** "32k" for a token count a person reads at a glance; small counts stay exact. */
export function compactTokens(n: number): string {
  if (n >= 10_000) return `${Math.round(n / 1000)}k`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

/** A rate a person reads at a glance: one decimal under 100 tok/s, whole numbers above. */
export function formatRate(tps: number, locale?: string): string {
  const digits = tps >= 100 ? 0 : 1;
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(tps);
}

/** "27m 54s" / "45s" from the engine's own elapsed seconds. */
export function formatElapsed(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** The renderer side of the IPC read: parsed stats, or the named reason there are none. */
export async function readMlxLiveStatus(baseUrl: string): Promise<MlxLiveRead> {
  const bridge = (
    window as unknown as {
      electron?: { mlxLiveStatus?: (b: string) => Promise<MlxLiveStatusResult> };
    }
  ).electron?.mlxLiveStatus;
  if (!bridge) return { ok: false, detail: 'this build has no live-status bridge' };
  let result: MlxLiveStatusResult;
  try {
    result = await bridge(baseUrl);
  } catch (err) {
    return { ok: false, detail: err instanceof Error ? err.message : String(err) };
  }
  if (!result.ok) return { ok: false, detail: `${result.error}: ${result.detail}` };
  return parseMlxLiveStatus(result.body);
}

/**
 * What MAIN last read of the engine (utils/mlxEngineMonitor.ts), which reads it the whole time it
 * answers — the page reads only while the Engine tab is open. WHO it serves, and every run main
 * caught, for the engine kind main read. null when this build has no bridge or the bridge fails:
 * the tile then shows no "Serving" block rather than an empty one that would read as "nobody", and
 * only the runs the page itself caught.
 */
export async function readMainEngine(): Promise<{
  serving: MlxServing | null;
  engine: MlxEngineSnapshot['engine'];
  rates: RateBook;
} | null> {
  const bridge = (
    window as unknown as { electron?: { mlxEngineActivity?: () => Promise<MlxEngineSnapshot> } }
  ).electron?.mlxEngineActivity;
  if (!bridge) return null;
  try {
    const snapshot = await bridge();
    return {
      serving: snapshot.mode === 'running' ? snapshot.serving : null,
      engine: snapshot.engine,
      rates: snapshot.rates,
    };
  } catch {
    return null;
  }
}

/** Two readers' books of ONE engine: the union of their runs (request ids are the engine's own). */
export function mergeRateBooks(a: RateBook, b: RateBook): RateBook {
  const uptimes = [a.uptimeS, b.uptimeS].filter((u): u is number => u != null);
  return {
    uptimeS: uptimes.length ? Math.max(...uptimes) : null,
    runs: new Map([...a.runs, ...b.runs]),
  };
}

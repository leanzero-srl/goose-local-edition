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
 * The prompt-reading (prefill) rate the engine achieved on the request whose first token landed
 * most recently: (prompt − cached) / ttft_s — the tokens it actually COMPUTED over the time it took
 * to compute them, the same accounting oMLX's usage history calls "prefill speed". Cached tokens are
 * excluded because a prefix-cache hit reads as thousands of tok/s it never did (the engine's own
 * aggregate `prompt_tps` counts them, and is sticky like `generation_tps`, so it is never shown).
 * ttft_s runs from ARRIVAL, so time queued behind another request is inside it: the figure is
 * conservative, never flattering. A request still in prefill has no rate yet — the engine reports
 * no per-request progress — so this is 0 until some request has written its first token.
 */
export function measuredPrefillTps(stats: MlxLiveStats): number {
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
      best = { firstTokenAge, tps: computed / r.ttftS };
    }
  }
  return best?.tps ?? 0;
}

/**
 * The last rates this reader MEASURED — what an idle tile or tray states as "last run". Kept apart
 * from the sparkline window (which forgets after two minutes) and dropped when the engine's own
 * uptime goes backwards (a restarted engine, possibly on another model, has measured nothing yet).
 */
export interface LastRates {
  uptimeS: number | null;
  decodeTps: number | null;
  prefillTps: number | null;
}

export const NO_RATES: LastRates = { uptimeS: null, decodeTps: null, prefillTps: null };

export function advanceLastRates(prev: LastRates, stats: MlxLiveStats): LastRates {
  const restarted = prev.uptimeS != null && stats.uptimeS != null && stats.uptimeS < prev.uptimeS;
  const base = restarted ? NO_RATES : prev;
  const decode = liveDecodeTps(stats);
  const prefill = measuredPrefillTps(stats);
  return {
    uptimeS: stats.uptimeS ?? base.uptimeS,
    decodeTps: decode > 0 ? decode : base.decodeTps,
    prefillTps: prefill > 0 ? prefill : base.prefillTps,
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
 * What a mount of `modelBytes` would cost against free memory, with the SAME verdict the sidecar's
 * mount gate gives (crates/goose-sidecar/src/memory.rs `MemoryGate::default`: a reserve of
 * max(8 GiB, 10% of total) kept free; block when the model plus reserve exceeds free memory, warn
 * when less than 4 GiB is left above the reserve). A second copy of that rule — the tile must not say
 * "fits" for a mount the gate would refuse; if the gate moves, this moves with it.
 */
export type FitVerdict = 'fits' | 'tight' | 'no-fit';

export interface MountCost {
  modelGb: number;
  freeGb: number;
  reserveGb: number;
  /** Free memory left above the reserve after the mount; negative = short by that much. */
  spareGb: number;
  verdict: FitVerdict;
}

const GATE_RESERVE_MIN_GB = 8;
const GATE_RESERVE_FRACTION = 0.1;
const GATE_WARN_BAND_GB = 4;

export function mountCost(modelBytes: number, freeGb: number, totalGb: number): MountCost {
  const modelGb = modelBytes / GIB;
  const reserveGb = Math.max(GATE_RESERVE_MIN_GB, totalGb * GATE_RESERVE_FRACTION);
  const spareGb = freeGb - modelGb - reserveGb;
  const verdict: FitVerdict =
    spareGb < 0 ? 'no-fit' : spareGb < GATE_WARN_BAND_GB ? 'tight' : 'fits';
  return { modelGb, freeGb, reserveGb, spareGb, verdict };
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
 * WHO the local engine is serving, as main last read it (utils/mlxEngineMonitor.ts) — null when this
 * build has no bridge, main has not read a running engine, or the bridge fails: the tile then shows
 * no "Serving" block rather than an empty one that would read as "nobody".
 */
export async function readMlxServing(): Promise<MlxServing | null> {
  const bridge = (
    window as unknown as { electron?: { mlxEngineActivity?: () => Promise<MlxEngineSnapshot> } }
  ).electron?.mlxEngineActivity;
  if (!bridge) return null;
  try {
    const snapshot = await bridge();
    return snapshot.mode === 'running' ? snapshot.serving : null;
  } catch {
    return null;
  }
}

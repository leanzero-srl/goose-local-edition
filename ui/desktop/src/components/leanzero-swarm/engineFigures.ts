import type { MeasuredFigure } from '../../utils/mlxMeasuredRuns';
import {
  answeredRequests,
  leavingRowsOf,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  requestActivity,
  type MlxLiveRequest,
  type MlxLiveStats,
} from './mlxLiveStats';

/**
 * The two figures the Engine tile leads with, as FACTS — the one derivation the tile, the sidebar's
 * engine glance and the floating mini window all format (each in its own words and size). Pure and
 * React-free: main imports it to build the glance it pushes to every window.
 *
 * Per activity: writing leads with the live writing rate; reading leads with the prompt's size and
 * how long it has been read; queued leads with how many wait — or, while rows whose answers already
 * ended hold the batch (Q-231 `leaving`), with how many of those and how long ago they were stopped
 * (Q-246); otherwise the median of goose's
 * measured writing runs. The second figure is always the reading rate — this prompt's while one is
 * read, else the median of the measured prompts. A figure nothing measured is null, never a dash.
 */
export type EngineFigure =
  | { kind: 'writing'; tps: number }
  | { kind: 'writingMedian'; median: number; runs: number }
  | { kind: 'prompt'; tokens: number | null; elapsedS: number }
  | { kind: 'queued'; count: number }
  /** Rows whose answers ended still hold the batch: how many, stopped how long ago (Q-246). */
  | { kind: 'leaving'; rows: number; sinceStopS: number | null }
  | { kind: 'reading'; tps: number }
  | { kind: 'readingMedian'; median: number; runs: number };

export interface MeasuredPair {
  writing: MeasuredFigure | null;
  reading: MeasuredFigure | null;
}

function largestOf(requests: readonly MlxLiveRequest[]): MlxLiveRequest | undefined {
  return requests.reduce<MlxLiveRequest | undefined>(
    (best, r) => (best == null || (r.promptTokens ?? 0) > (best.promptTokens ?? 0) ? r : best),
    undefined
  );
}

/**
 * The request with the largest prompt. A chat's turn carries the whole conversation; goose's own
 * calls beside it (a title, the fact check, a tool label — Q-185) carry a few hundred tokens. So the
 * largest prompt is the one a person waits on — the rule the composer's `turnRequestOf`, the glance's
 * lead and every headline below use. A `leaving` row (its answer already ended, Q-231) is waited on
 * by nobody, so it is never that request.
 */
export function largestPrompt(requests: readonly MlxLiveRequest[]): MlxLiveRequest | undefined {
  return largestOf(answeredRequests(requests));
}

/**
 * The request still reading its prompt that a headline names: the LARGEST prompt being read. Q-218:
 * the old rule (the one read longest) named a 174-token side call read for 12 s ("Reading prompt ·
 * 174 prompt tokens") while the chat's own 77k prompt was 1% in. A `leaving` row is never it: its
 * answer ended, and "Reading 5.5K" for a stopped fact check read as work beside the queued turn
 * (Q-246); the `leaving` figure says those rows.
 */
export function readingRequest(stats: MlxLiveStats): MlxLiveRequest | undefined {
  return largestPrompt(
    stats.requests.filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
  );
}

/**
 * The figures. Without `lead` they speak for the ENGINE (the Engine tile, the tray): its activity,
 * its summed writing rate, its largest prompt being read. With `lead` — the request a chat's turn is
 * (the glance, engineGlance.ts) — they speak for THAT request: its own phase, writing rate and
 * reading rate, so a side call running beside it is never read as the turn.
 */
export function engineFigures(
  stats: MlxLiveStats,
  measured: MeasuredPair,
  lead: MlxLiveRequest | null = null
): { hero: EngineFigure | null; second: EngineFigure | null } {
  const activity = lead ? requestActivity(lead) : mlxActivity(stats);
  const leadReading = lead?.phase === 'prefill' ? (lead.promptTps ?? 0) : 0;
  const prefillNow = leadReading > 0 ? leadReading : measuredPrefillTps(stats);
  const { writing, reading } = measured;
  const second: EngineFigure | null =
    prefillNow > 0
      ? { kind: 'reading', tps: prefillNow }
      : reading
        ? { kind: 'readingMedian', median: reading.median, runs: reading.runs }
        : null;
  if (activity === 'generating') {
    return {
      hero: { kind: 'writing', tps: lead ? leadWritingTps(lead) : liveDecodeTps(stats) },
      second,
    };
  }
  if (activity === 'prefill') {
    const r = lead ?? readingRequest(stats);
    return {
      hero: { kind: 'prompt', tokens: r?.promptTokens ?? null, elapsedS: r?.elapsedS ?? 0 },
      second,
    };
  }
  if (activity === 'queued') {
    const leaving = leavingRowsOf(stats.requests);
    return {
      hero: leaving
        ? { kind: 'leaving', ...leaving }
        : { kind: 'queued', count: answeredRequests(stats.requests).length },
      second,
    };
  }
  return {
    hero: writing ? { kind: 'writingMedian', median: writing.median, runs: writing.runs } : null,
    second,
  };
}

/** One request's own writing rate — a rate needs two tokens (liveDecodeTps's rule, for one). */
function leadWritingTps(lead: MlxLiveRequest): number {
  return lead.completionTokens >= 2 ? (lead.tokensPerSecond ?? 0) : 0;
}

/**
 * How far into the prompt the named read is (the lead's when it is reading, else the largest) — the distributed engine reports it; the single engine
 * reports no per-request progress, so there it is null and no bar is drawn (never a guessed share).
 */
export function promptProgress(
  stats: MlxLiveStats,
  lead: MlxLiveRequest | null = null
): { done: number; total: number } | null {
  const r = lead ? (lead.phase === 'prefill' ? lead : undefined) : readingRequest(stats);
  return r ? readProgressOf(r) : null;
}

function readProgressOf(r: MlxLiveRequest): { done: number; total: number } | null {
  if (r.prefilledTokens == null || !r.promptTokens) return null;
  return { done: Math.min(r.prefilledTokens, r.promptTokens), total: r.promptTokens };
}

/**
 * A chat's turn while its prompt is read, in the figures the card leads with for it (Q-301 — the
 * chat's own working row repeats them): the prompt's size, how long it has been read, how far in
 * (`promptProgress`'s rule), the rate it is read at NOW (the request's own prefill rate — the
 * card's "tok/s reading this prompt"), and the time left AT THAT RATE. Every figure is one the
 * engine measured; one it does not report is null, and with no live rate or no progress there is
 * no time left — never a guessed rate, never a guessed share.
 */
export interface PromptRead {
  tokens: number | null;
  elapsedS: number;
  progress: { done: number; total: number } | null;
  tps: number | null;
  leftS: number | null;
}

export function promptRead(lead: MlxLiveRequest): PromptRead | null {
  if (requestActivity(lead) !== 'prefill') return null;
  const progress = readProgressOf(lead);
  const tps = lead.promptTps != null && lead.promptTps > 0 ? lead.promptTps : null;
  return {
    tokens: lead.promptTokens,
    elapsedS: lead.elapsedS ?? 0,
    progress,
    tps,
    leftS: progress && tps ? (progress.total - progress.done) / tps : null,
  };
}

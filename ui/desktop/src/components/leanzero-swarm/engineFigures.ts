import type { MeasuredFigure } from '../../utils/mlxMeasuredRuns';
import {
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  type MlxLiveRequest,
  type MlxLiveStats,
} from './mlxLiveStats';

/**
 * The two figures the Engine tile leads with, as FACTS — the one derivation the tile, the sidebar's
 * engine glance and the floating mini window all format (each in its own words and size). Pure and
 * React-free: main imports it to build the glance it pushes to every window.
 *
 * Per activity: writing leads with the live writing rate; reading leads with the prompt's size and
 * how long it has been read; queued leads with how many wait; otherwise the median of goose's
 * measured writing runs. The second figure is always the reading rate — this prompt's while one is
 * read, else the median of the measured prompts. A figure nothing measured is null, never a dash.
 */
export type EngineFigure =
  | { kind: 'writing'; tps: number }
  | { kind: 'writingMedian'; median: number; runs: number }
  | { kind: 'prompt'; tokens: number | null; elapsedS: number }
  | { kind: 'queued'; count: number }
  | { kind: 'reading'; tps: number }
  | { kind: 'readingMedian'; median: number; runs: number };

export interface MeasuredPair {
  writing: MeasuredFigure | null;
  reading: MeasuredFigure | null;
}

/** The request still reading its prompt that has waited longest — what a headline names. */
export function readingRequest(stats: MlxLiveStats): MlxLiveRequest | undefined {
  return stats.requests
    .filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
    .sort((a, b) => (b.elapsedS ?? 0) - (a.elapsedS ?? 0))[0];
}

export function engineFigures(
  stats: MlxLiveStats,
  measured: MeasuredPair
): { hero: EngineFigure | null; second: EngineFigure | null } {
  const activity = mlxActivity(stats);
  const prefillNow = measuredPrefillTps(stats);
  const { writing, reading } = measured;
  const second: EngineFigure | null =
    prefillNow > 0
      ? { kind: 'reading', tps: prefillNow }
      : reading
        ? { kind: 'readingMedian', median: reading.median, runs: reading.runs }
        : null;
  if (activity === 'generating') {
    return { hero: { kind: 'writing', tps: liveDecodeTps(stats) }, second };
  }
  if (activity === 'prefill') {
    const r = readingRequest(stats);
    return {
      hero: { kind: 'prompt', tokens: r?.promptTokens ?? null, elapsedS: r?.elapsedS ?? 0 },
      second,
    };
  }
  if (activity === 'queued') {
    return { hero: { kind: 'queued', count: stats.requests.length }, second };
  }
  return {
    hero: writing ? { kind: 'writingMedian', median: writing.median, runs: writing.runs } : null,
    second,
  };
}

/**
 * How far into the prompt the longest read is — the distributed engine reports it; the single engine
 * reports no per-request progress, so there it is null and no bar is drawn (never a guessed share).
 */
export function promptProgress(stats: MlxLiveStats): { done: number; total: number } | null {
  const r = readingRequest(stats);
  if (!r || r.prefilledTokens == null || !r.promptTokens) return null;
  return { done: Math.min(r.prefilledTokens, r.promptTokens), total: r.promptTokens };
}

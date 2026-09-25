import type { Message } from '../../types/message';
import type { MlxLiveRequest } from '../leanzero-swarm/mlxLiveStats';
import { reconnectingMac, type ChatServedBy } from './chatServedBy';

/**
 * WHAT THIS CHAT'S TURN IS WAITING ON — the status line under the composer ("goose is working on
 * it…" otherwise). Every cue is a fact about THIS turn, never a guess:
 *  - `reconnecting`: the Mac that serves it stopped answering (served-by `reconnecting`);
 *  - `checking`: that Mac answers again but the turn has not streamed since — the relay can tell
 *    whether the Mac still holds the answer only now, so ONE cue holds until the turn streams again
 *    or ends in the dropped-turn notice (Q-52);
 *  - `silent`: the turn's own stream went quiet for far longer than its own measured cadence,
 *    before main's poll can notice anything (Q-55) — a display cue, never a terminator;
 *  - `reading`: the engine is reading this turn's prompt, from main's live stats (Q-13).
 */
export type TurnCue =
  | { kind: 'reconnecting'; mac: string }
  | { kind: 'checking'; mac: string }
  | { kind: 'silent'; mac: string }
  | ({ kind: 'reading'; mac: string } & ReadingProgress);

/**
 * How far the engine is into this turn's prompt, by what it actually reports:
 *  - `percent`: the engine reports the tokens it has read (the distributed engine's rank 0);
 *  - `eta`: it does not (Rapid-MLX's single engine), but it has a measured read rate — elapsed
 *    against the time that rate needs for the uncached tokens, and only while elapsed is inside it;
 *  - `elapsed`: neither, or the read already took longer than the rate says;
 *  - `plain`: not even the elapsed time.
 * A percentage is never computed from time.
 */
export type ReadingProgress =
  | { progress: 'percent'; promptTokens: number; percent: number }
  | { progress: 'eta'; promptTokens: number; elapsedS: number; expectedS: number; rate: number }
  | { progress: 'elapsed'; promptTokens: number; elapsedS: number }
  | { progress: 'plain'; promptTokens: number };

export function readingProgress(
  request: MlxLiveRequest | null,
  readTps: number | null
): ReadingProgress | null {
  if (!request || request.phase !== 'prefill' || request.status === 'waiting') return null;
  const promptTokens = request.promptTokens;
  if (promptTokens == null || promptTokens <= 0) return null;
  if (request.prefilledTokens != null) {
    return {
      progress: 'percent',
      promptTokens,
      percent: Math.min(100, Math.floor((request.prefilledTokens / promptTokens) * 100)),
    };
  }
  const elapsedS = request.elapsedS;
  if (elapsedS == null) return { progress: 'plain', promptTokens };
  const toRead = promptTokens - (request.cachedTokens ?? 0);
  if (readTps != null && readTps > 0 && toRead > 0) {
    const expectedS = toRead / readTps;
    if (elapsedS <= expectedS) {
      return { progress: 'eta', promptTokens, elapsedS, expectedS, rate: readTps };
    }
  }
  return { progress: 'elapsed', promptTokens, elapsedS };
}

/** What the turn's stream has shown, in characters (text and thinking), for its silence and growth. */
export function streamSize(message: Message | undefined): number {
  if (!message || message.role !== 'assistant') return 0;
  let size = 0;
  for (const c of message.content) {
    if (c.type === 'text') size += c.text.length;
    else if (c.type === 'thinking' && 'thinking' in c && c.thinking) size += c.thinking.length;
  }
  return size;
}

/** The stream is on words (not a tool call running on this Mac, whose silence is the tool's). */
export function streamOnWords(message: Message | undefined): boolean {
  const last = message?.role === 'assistant' ? message.content[message.content.length - 1] : null;
  return last?.type === 'text' || last?.type === 'thinking';
}

/**
 * A stream quiet for this many of ITS OWN median chunk gaps has skipped far more beats than
 * decode or relay jitter makes. // ratio: a multiple of the turn's measured cadence (a 20 tok/s
 * turn chunking every ~50 ms says so after ~1 s; the Link kill froze it 4.7 s before main's poll
 * noticed, Q-55).
 */
export const SILENCE_GAP_MULTIPLE = 20;

/** The median of fewer gaps than this is one chunk's accident, not a cadence. // ratio: 5 gaps */
export const SILENCE_MIN_GAPS = 5;

/**
 * The cadence is the RECENT one: a turn changes pace (thinking, then writing), and the median is
 * taken on every chunk, so the window is bounded. // ratio: the last 64 chunk gaps
 */
export const SILENCE_WINDOW_GAPS = 64;

export function medianOf(values: readonly number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/** How long the stream may be quiet before `silent`, from its measured gaps; null = not measured. */
export function silenceAfterMs(gapsMs: readonly number[]): number | null {
  if (gapsMs.length < SILENCE_MIN_GAPS) return null;
  const median = medianOf(gapsMs);
  return median == null ? null : median * SILENCE_GAP_MULTIPLE;
}

export interface TurnCueInputs {
  served: ChatServedBy | null;
  /** This chat's turn is thinking or streaming. */
  inFlight: boolean;
  /** Contact was lost during this turn and it has not streamed since: that Mac's name. */
  lostTo: string | null;
  /** The turn's own stream went quiet past its measured cadence. */
  silent: boolean;
}

/** The one cue, by precedence: what blocks the turn now first, then what it is doing. */
export function pickTurnCue({ served, inFlight, lostTo, silent }: TurnCueInputs): TurnCue | null {
  if (!inFlight || !served) return null;
  const reconnecting = reconnectingMac(served);
  if (reconnecting) return { kind: 'reconnecting', mac: reconnecting };
  const mac = served.where[0] ?? null;
  if (lostTo) return { kind: 'checking', mac: lostTo };
  if (silent && mac && served.engine === 'remote') return { kind: 'silent', mac };
  const reading = readingProgress(served.turnRequest, served.readTps);
  if (reading && mac) return { kind: 'reading', mac, ...reading };
  return null;
}

import { compactTokens, formatElapsed } from '../components/leanzero-swarm/mlxLiveStats';
import type { MlxEngineSnapshot } from './mlxEngineMonitor';
import type { MlxClient } from './mlxServing';
import type { MlxTrayAction } from './mlxTray';

/**
 * The work a stop or a switch would CUT, from main's one read of the engine that serves this Mac's
 * chat (mlxEngineMonitor): its own request list and goose's in-flight list naming whose work it is.
 * Every door that stops or replaces an engine reads this before it acts (Q-148: Run it's "Run on
 * Work's Mac Studio · Best" and the tray's "Stop the distributed engine" (now "Stop the split") both stopped a split in the
 * middle of a 39-minute answer, with no word about it).
 *
 * Pure, so main (the tray) and the renderer (every dialog) name the same work in the same numbers.
 */

/** Which engine a read is of, and which engine a door stops. */
export type MlxEngineKind = MlxEngineSnapshot['engine'];

export interface InFlightWork {
  engine: MlxEngineKind;
  /** Requests the engine holds now, running and waiting. */
  requests: number;
  /** Whose work it is, as goose listed it — empty when goose's list could not name it. */
  clients: MlxClient[];
  /** The request longest in flight: its engine-measured seconds, and the tokens it has written. */
  elapsedS: number | null;
  tokens: number;
  /** It has not written a token yet: reading its prompt, or waiting for a slot. */
  reading: boolean;
  promptTokens: number | null;
  /**
   * Prompt + written tokens of the largest request — the context the live conversation already
   * holds, which an engine that replaces this one must hold too.
   */
  contextTokens: number | null;
}

/** The work main's read shows in flight, or null when the engine holds none (or was not read). */
export function inFlightWork(snapshot: MlxEngineSnapshot | null | undefined): InFlightWork | null {
  if (!snapshot || snapshot.mode !== 'running' || !snapshot.stats) return null;
  const requests = snapshot.stats.requests;
  if (requests.length === 0) return null;
  const lead = [...requests].sort((a, b) => (b.elapsedS ?? 0) - (a.elapsedS ?? 0))[0];
  const contexts = requests
    .filter((r) => r.promptTokens != null)
    .map((r) => (r.promptTokens ?? 0) + r.completionTokens);
  return {
    engine: snapshot.engine,
    requests: requests.length,
    clients: snapshot.serving?.clients ?? [],
    elapsedS: lead.elapsedS,
    tokens: lead.completionTokens,
    reading: lead.phase !== 'generation',
    promptTokens: lead.promptTokens,
    contextTokens: contexts.length > 0 ? Math.max(...contexts) : null,
  };
}

/** The work a door that stops `engines` cuts: main's read, only when it is of one of them. */
export function workCutBy(
  snapshot: MlxEngineSnapshot | null | undefined,
  engines: readonly MlxEngineKind[]
): InFlightWork | null {
  const work = inFlightWork(snapshot);
  return work && engines.includes(work.engine) ? work : null;
}

/**
 * The engine each tray action stops (utils/mlxTray.ts `MlxTrayAction`): main raises the window
 * for the renderer's question when the action would cut work, and the renderer asks it. Mount and
 * Open Providers stop nothing.
 */
export const TRAY_ACTION_ENGINES: Readonly<
  Partial<Record<MlxTrayAction, readonly MlxEngineKind[]>>
> = {
  unmount: ['single'],
  'stop-distributed': ['distributed'],
  'stop-remote': ['remote'],
  'run-here': ['remote'],
  'stop-waiting': ['remote'],
};

/** A client by the name a person knows it by. */
export function clientName(client: MlxClient): string {
  switch (client.kind) {
    case 'chat':
      return client.sessionName || client.sessionId;
    case 'session':
      return client.sessionName || client.sessionId || client.sessionType || 'a goose session';
    case 'external':
      return `/v1 · ${client.model}`;
  }
}

/** The work's figures in the tray's English: "39m 15s, 24.2k tokens written". */
export function workFigures(work: InFlightWork): string {
  const elapsed = work.elapsedS != null ? formatElapsed(work.elapsedS) : null;
  const tokens = work.reading
    ? work.promptTokens != null
      ? `reading a ${compactTokens(work.promptTokens)}-token prompt`
      : 'reading its prompt'
    : `${compactTokens(work.tokens)} tokens written`;
  return [elapsed, tokens].filter(Boolean).join(', ');
}

/**
 * The tray's line above a stop that would cut work: what it cuts, in figures — the serving line
 * above it already names whose it is. "Stopping cuts the answer in flight (39m 15s, 24.2k tokens
 * written)".
 */
export function trayCutLine(work: InFlightWork): string {
  const what =
    work.requests > 1 ? `${work.requests} requests in flight, the longest` : 'the answer in flight';
  return `Stopping cuts ${what} (${workFigures(work)})`;
}

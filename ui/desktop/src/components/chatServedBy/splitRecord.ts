import type { MlxDistributedEvent, MlxDistributedStatus } from '../../acp/mlx-distributed';
import { splitMacNames, splitStopAt, splitStopIn, type SplitStop } from './splitStop';

/**
 * THE SPLIT'S RECORD, SAVED WITH THE FAILED TURN (Q-121, Q-122). The split's events live in
 * goosed's memory only: after a relaunch ("Supervisor events 0") the notice a failed turn showed
 * live — "The split across your Macs stopped — the Macs stopped making progress" — had nothing to
 * be derived from, and history said "No model is mounted" five times instead. So the agent loop
 * (crates/goose/src/agents/split_record.rs) writes the supervisor's own events into the failure
 * text on one line, between the error and the closing sentence:
 *
 *   Network error: Stream decode error: … no [DONE] after 9771 data frames — the answer is incomplete
 *
 *   Split supervisor record: {"v":1,"state":"failed","turnStartedMs":…,"events":[…]}
 *
 *   Please resend your message to try again.
 *
 * It is written whenever this goosed's split had served. This file takes the line out of the text
 * and reads it back through the SAME rule the live events go through (`splitStopIn`) — the record
 * is facts, the meaning is derived here.
 */
export const SPLIT_RECORD_MARKER = 'Split supervisor record: ';

export interface SplitRecord {
  /** The split's state when the turn failed ("failed", "stopped", "recovering", …). */
  state: string;
  /** When the failed provider call began (ms since the epoch). */
  turnStartedMs: number;
  modelId: string | null;
  macs: string[];
  /** The supervisor's events from the split's last `ready` on. */
  events: MlxDistributedEvent[];
}

export type TakenSplitRecord =
  | { kind: 'none'; text: string }
  | { kind: 'record'; text: string; record: SplitRecord }
  /** The line is there but does not read as a record: it stays in the text, seen, never guessed. */
  | { kind: 'unreadable'; text: string; error: string };

const strings = (v: unknown): string[] | null =>
  Array.isArray(v) && v.every((x) => typeof x === 'string') ? v : null;

function eventOf(v: unknown): MlxDistributedEvent | null {
  if (typeof v !== 'object' || v == null) return null;
  const e = v as Record<string, unknown>;
  if (typeof e.atMs !== 'number' || typeof e.kind !== 'string' || typeof e.message !== 'string') {
    return null;
  }
  const node = typeof e.node === 'string' ? e.node : null;
  return { atMs: e.atMs, kind: e.kind, node, message: e.message };
}

function readRecord(json: string): SplitRecord | string {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
  if (typeof parsed !== 'object' || parsed == null) return 'not an object';
  const r = parsed as Record<string, unknown>;
  if (typeof r.state !== 'string') return 'no state';
  if (typeof r.turnStartedMs !== 'number') return 'no turnStartedMs';
  if (!Array.isArray(r.events)) return 'no events';
  const events = r.events.map(eventOf);
  if (events.some((e) => e == null)) return 'an event without atMs, kind or message';
  const nodes = strings(r.nodes);
  if (!nodes) return 'no nodes';
  const configured = r.configNodes == null ? [] : strings(r.configNodes);
  if (!configured) return 'configNodes is not a list of names';
  return {
    state: r.state,
    turnStartedMs: r.turnStartedMs,
    modelId: typeof r.modelId === 'string' ? r.modelId : null,
    macs: splitMacNames(nodes, configured),
    events: events as MlxDistributedEvent[],
  };
}

/** The failure text without the record line, and the record it carried. */
export function takeSplitRecord(text: string): TakenSplitRecord {
  const at = text.lastIndexOf(SPLIT_RECORD_MARKER);
  if (at < 0) return { kind: 'none', text };
  const lineStart = at === 0 || text[at - 1] === '\n';
  if (!lineStart) return { kind: 'none', text };
  const newline = text.indexOf('\n', at);
  const end = newline < 0 ? text.length : newline;
  const read = readRecord(text.slice(at + SPLIT_RECORD_MARKER.length, end));
  if (typeof read === 'string') return { kind: 'unreadable', text, error: read };
  // The record sits between "\n\n" pairs: take one pair with it, so the error and the closer keep
  // exactly the blank line the text had without a record.
  const before = text.slice(0, at);
  const after = text.slice(end);
  const joined =
    before.endsWith('\n\n') && after.startsWith('\n\n') ? before + after.slice(2) : before + after;
  return { kind: 'record', text: joined, record: read };
}

/**
 * The split stop a record carries for a turn that met the split already stopped (a refusal, the
 * router's "no node can serve this turn"): bounded by the message's own time exactly as the live
 * notice bounds it.
 */
export function refusalStopFromRecord(
  record: SplitRecord,
  atMs: number,
  deathByMs: number | null
): SplitStop | null {
  const found = splitStopIn(record.events, record.macs, atMs, deathByMs);
  return found ? { ...found.stop, modelId: record.modelId } : null;
}

/**
 * The split stop that CUT a turn: the split was serving when the provider call began (`ready` at
 * or before it) and the event that stopped it came after it — a stop from before the call began
 * did not cut this answer, whatever served it.
 */
export function cutStopFromRecord(
  record: SplitRecord,
  live: MlxDistributedStatus | null
): SplitStop | null {
  const found = splitStopIn(record.events, record.macs, record.turnStartedMs, null);
  if (found) {
    return found.death.atMs < record.turnStartedMs
      ? null
      : { ...found.stop, modelId: record.modelId };
  }
  // A rank that died on its own cut the stream before the supervisor's poll wrote it (E2E #2): the
  // record says the turn ran on the split; the stop is in the events this window reads after it.
  const later = splitStopAt(live, record.turnStartedMs, null);
  return later && later.atMs >= record.turnStartedMs ? later : null;
}

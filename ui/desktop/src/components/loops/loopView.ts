/**
 * What the rail shows for a loop (DESIGN-SESSION-LOOPS §4.7, §8.3, §8.4), as pure functions over the
 * record: the pill's label and fill, the status chip, a tick's outcome chip, the quiet-tick rule,
 * the header's totals and each tick's message range. Words are message descriptors plus values, so
 * the components only format; every fact comes from the record or the transcript, and an absent
 * fact is left out or named, never filled in.
 */
import type { MessageDescriptor } from 'react-intl';
import type { Message } from '../../types/message';
import type { Tone } from '../lz';
import { loopWords as w } from './loopWords';
import {
  clockTime,
  durationWords,
  parseCadenceSeconds,
  parseTime,
  tickRanges,
  type LoopCadence,
  type LoopRecord,
  type LoopStatus,
  type LoopStatusReason,
  type LoopTickOrigin,
  type LoopTickRecord,
} from './model';

export interface Words {
  message: MessageDescriptor;
  values?: Record<string, string | number>;
}

const words = (message: MessageDescriptor, values?: Record<string, string | number>): Words =>
  values ? { message, values } : { message };

/** The viewer's own UTC offset, the one `clockTime` takes. */
export function viewerOffsetMinutes(atMs: number): number {
  return -new Date(atMs).getTimezoneOffset();
}

/** `HH:MM` in the viewer's zone, or null for a time the record cannot give. */
export function hm(rfc3339: string | null | undefined): string | null {
  if (!rfc3339) return null;
  const t = parseTime(rfc3339);
  if (!t.ok) return null;
  const c = clockTime(t.value, viewerOffsetMinutes(t.value));
  return c.ok ? c.value : null;
}

/** The loop's status as read now, and why. */
export interface LoopState {
  status: LoopStatus;
  reason?: LoopStatusReason | null;
}

export interface PillView {
  tone: Tone;
  label: Words;
}

export function lastTick(record: LoopRecord): LoopTickRecord | undefined {
  const ticks = record.ticks ?? [];
  return ticks[ticks.length - 1];
}

function elapsedSince(rfc3339: string | undefined, nowMs: number): string | null {
  if (!rfc3339) return null;
  const t = parseTime(rfc3339);
  return t.ok ? durationWords((nowMs - t.value) / 1000) : null;
}

/** The collapsed rail's loop pill (§4.7 "Rail pill"). */
export function pillView(record: LoopRecord, state: LoopState, nowMs: number): PillView {
  const last = lastTick(record);
  const reason = state.reason;
  switch (state.status) {
    case 'running': {
      const elapsed = elapsedSince(last?.startedAt, nowMs);
      return {
        tone: 'ok',
        label:
          last && elapsed ? words(w.pillRunning, { n: last.n, elapsed }) : words(w.statusRunning),
      };
    }
    case 'checking': {
      const elapsed = elapsedSince(last?.check?.startedAt, nowMs);
      return {
        tone: 'accent',
        label: elapsed ? words(w.pillChecking, { elapsed }) : words(w.statusChecking),
      };
    }
    case 'waiting': {
      const at = record.nextTick ? parseTime(record.nextTick.at) : null;
      if (at?.ok && at.value <= nowMs) return { tone: 'accent', label: words(w.pillStartsNow) };
      const time = hm(record.nextTick?.at);
      return {
        tone: 'accent',
        label: time ? words(w.pillNext, { time }) : words(w.statusWaiting),
      };
    }
    case 'waiting_turn':
      switch (reason?.kind) {
        case 'reviewers':
          return { tone: 'secondary', label: words(w.pillAfterReviewers, { n: reason.n }) };
        case 'way_held':
          return { tone: 'secondary', label: words(w.pillAfterWayHeld, { node: reason.node }) };
        case 'refused':
          if (reason.refused.kind === 'load_failed' || reason.refused.kind === 'submit_failed') {
            return { tone: 'warn', label: words(w.pillCouldNotStart) };
          }
          return { tone: 'secondary', label: words(w.pillAfterYourTurn) };
        default:
          return { tone: 'secondary', label: words(w.pillAfterYourTurn) };
      }
    case 'waiting_you':
      return { tone: 'warn', label: words(w.pillWaitingYou) };
    case 'needs_you':
      return { tone: 'warn', label: words(w.pillNeedsYou) };
    case 'paused':
      return { tone: 'stopped', label: words(w.pillPaused) };
    case 'ended':
      return { tone: 'stopped', label: words(w.pillEnded) };
    case 'elsewhere':
      return { tone: 'secondary', label: words(w.pillElsewhere) };
  }
}

/** The panel's status chip (§8.4 "Status chip"): the status word, solid. */
export function statusChip(status: LoopStatus): { tone: Tone; label: MessageDescriptor } {
  switch (status) {
    case 'running':
      return { tone: 'ok', label: w.statusRunning };
    case 'checking':
      return { tone: 'accent', label: w.statusChecking };
    case 'waiting':
      return { tone: 'accent', label: w.statusWaiting };
    case 'waiting_turn':
      return { tone: 'secondary', label: w.statusWaitingTurn };
    case 'waiting_you':
      return { tone: 'warn', label: w.statusWaitingYou };
    case 'needs_you':
      return { tone: 'warn', label: w.statusNeedsYou };
    case 'paused':
      return { tone: 'stopped', label: w.statusPaused };
    case 'ended':
      return { tone: 'stopped', label: w.statusEnded };
    case 'elsewhere':
      return { tone: 'secondary', label: w.statusElsewhere };
  }
}

/** "every 10 min", "goose decides when", "back to back"; an unreadable cadence as typed. */
export function cadenceWords(cadence: LoopCadence): Words {
  switch (cadence.kind) {
    case 'self_paced':
      return words(w.cadenceSelfPaced);
    case 'back_to_back':
      return words(w.cadenceBackToBack);
    case 'every': {
      const text = cadence.every.trim();
      const match = /^(\d+)([smh])$/.exec(text);
      if (!match || parseCadenceSeconds(text) === null) {
        return words(w.cadenceAsTyped, { every: text });
      }
      const n = Number(match[1]);
      const unit = match[2];
      return words(
        unit === 's' ? w.cadenceSeconds : unit === 'm' ? w.cadenceMinutes : w.cadenceHours,
        { n }
      );
    }
  }
}

/**
 * What started a tick, for its marker (§8.5): the cadence's own label when the cadence started it,
 * else the event that did — the tick after a yield starts "after your turn", never "back to back"
 * (Q-279).
 */
export function tickCauseWords(origin: LoopTickOrigin, cadence: LoopCadence): Words {
  switch (origin) {
    case 'after_your_turn':
      return words(w.originAfterYourTurn);
    case 'after_your_answer':
      return words(w.originAfterYourAnswer);
    case 'on_wake':
      return words(w.originOnWake);
    case 'resume':
      return words(w.originResume);
    case 'now':
      return words(w.originNow);
    case 'first':
    case 'cadence':
    case 'self_paced':
    case 'back_to_back':
      return cadenceWords(cadence);
  }
}

/** A tick is in flight while it has not ended; the ledger lists ended ticks only. */
export function endedTicks(record: LoopRecord): LoopTickRecord[] {
  return (record.ticks ?? []).filter((tick) => !!tick.endedAt);
}

/** The loop's total wall time: the sum of its ticks' own durations (the running one up to now). */
export function totalWallSeconds(record: LoopRecord, nowMs: number): number | null {
  let total = 0;
  let any = false;
  for (const tick of record.ticks ?? []) {
    const start = parseTime(tick.startedAt);
    if (!start.ok) continue;
    const end = tick.endedAt ? parseTime(tick.endedAt) : { ok: true as const, value: nowMs };
    if (!end.ok) continue;
    total += Math.max(0, (end.value - start.value) / 1000);
    any = true;
  }
  return any ? total : null;
}

/** Total tokens across the ticks that measured theirs; null when none did (never a 0). */
export function totalTokens(record: LoopRecord): number | null {
  const measured = (record.ticks ?? []).filter((tick) => tick.tokens);
  if (measured.length === 0) return null;
  return measured.reduce((sum, tick) => sum + (tick.tokens?.total ?? 0), 0);
}

export function tickDurationSeconds(tick: LoopTickRecord, nowMs: number): number | null {
  const start = parseTime(tick.startedAt);
  if (!start.ok) return null;
  const end = tick.endedAt ? parseTime(tick.endedAt) : { ok: true as const, value: nowMs };
  return end.ok ? Math.max(0, (end.value - start.value) / 1000) : null;
}

export type ChipKind =
  | 'progress'
  | 'done'
  | 'goalMet'
  | 'blocked'
  | 'asked'
  | 'failed'
  | 'noReport'
  | 'stalled'
  | 'yielded'
  | 'stoppedByYou';

const CHIP: Record<ChipKind, { tone: Tone; label: MessageDescriptor }> = {
  progress: { tone: 'accent', label: w.chipProgress },
  done: { tone: 'ok', label: w.chipDone },
  goalMet: { tone: 'ok', label: w.chipGoalMet },
  blocked: { tone: 'err', label: w.chipBlocked },
  asked: { tone: 'warn', label: w.chipAsked },
  failed: { tone: 'err', label: w.chipFailed },
  noReport: { tone: 'stopped', label: w.chipNoReport },
  stalled: { tone: 'stopped', label: w.chipStalled },
  yielded: { tone: 'secondary', label: w.chipYielded },
  stoppedByYou: { tone: 'stopped', label: w.chipStoppedByYou },
};

export function chipOf(kind: ChipKind): { tone: Tone; label: MessageDescriptor } {
  return CHIP[kind];
}

/**
 * The tick's outcome chip. "Goal met" and "Stalled" are the loop's own verdict on THIS tick (the
 * status reason names the tick), so a later tick never relabels an earlier one.
 */
export function tickChipKind(tick: LoopTickRecord, record: LoopRecord): ChipKind | null {
  const outcome = tick.outcome;
  if (!outcome) return null;
  const reason = record.statusReason;
  switch (outcome.kind) {
    case 'progress':
    case 'done':
      if (reason?.kind === 'goal_met' && reason.n === tick.n) return 'goalMet';
      if (reason?.kind === 'stalled' && reason.n === tick.n) return 'stalled';
      return outcome.kind;
    case 'blocked':
      return 'blocked';
    case 'asked':
      return 'asked';
    case 'failed':
      return 'failed';
    case 'no_report':
      return 'noReport';
    case 'yielded':
      return 'yielded';
    case 'stopped_by_you':
      return 'stoppedByYou';
  }
}

function sameCheckExit(prev: LoopTickRecord | undefined, cur: LoopTickRecord): boolean {
  const a = prev?.check;
  const b = cur.check;
  if (!a && !b) return true;
  if (!a || !b) return false;
  return a.ran === b.ran && (a.exit ?? null) === (b.exit ?? null);
}

/**
 * A quiet tick (§8.4): it reported progress, made no write or edit outside the state file
 * (`wrote` already excludes it), and its check's exit did not change. It collapses to one line.
 */
export function isQuietTick(
  tick: LoopTickRecord,
  prev: LoopTickRecord | undefined,
  record: LoopRecord
): boolean {
  return (
    tickChipKind(tick, record) === 'progress' &&
    (tick.wrote ?? []).length === 0 &&
    !!tick.report &&
    sameCheckExit(prev, tick)
  );
}

export function firstLine(text: string): string {
  return (text.trim().split('\n')[0] ?? '').trim();
}

export interface TickSlice {
  tick: LoopTickRecord;
  /** The tick's messages, from its marker to the next marker; null when an edit removed it. */
  messages: Message[] | null;
}

/** Every tick's own messages — the ONE derivation the rows' files come from (`sessionChanges`). */
export function tickSlices(messages: readonly Message[], record: LoopRecord): TickSlice[] {
  const ticks = record.ticks ?? [];
  const ranges = tickRanges(
    messages.map((message) => message.id),
    ticks
  );
  return ticks.map((tick, i) => {
    const range = ranges[i]?.range ?? null;
    return { tick, messages: range ? messages.slice(range[0], range[1]) : null };
  });
}

function textOf(message: Message): string {
  return message.content
    .map((content) => (content.type === 'text' ? content.text : ''))
    .join('')
    .trim();
}

/** The last line of the last thing goose said in the tick (a no-report tick quotes it). */
export function lastAssistantLine(slice: readonly Message[]): string | null {
  for (let i = slice.length - 1; i >= 0; i--) {
    const message = slice[i];
    if (message.role !== 'assistant') continue;
    const text = textOf(message);
    if (!text) continue;
    const lines = text.split('\n').filter((line) => line.trim());
    return lines[lines.length - 1]?.trim() ?? null;
  }
  return null;
}

/** The person's answer to a tick that asked: the first message they sent after the marker. */
export function answerIn(slice: readonly Message[]): string | null {
  for (const message of slice.slice(1)) {
    if (message.role !== 'user' || message.metadata.steer || message.metadata.loopTick) continue;
    const text = textOf(message);
    if (text) return text;
  }
  return null;
}

/** The DOM id of a tick's marker in the transcript; the rail scrolls to it. */
export function tickMarkerDomId(messageId: string): string {
  return `loop-tick-${messageId}`;
}

export function revealTickMarker(messageId: string): boolean {
  const marker = document.getElementById(tickMarkerDomId(messageId));
  marker?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  return !!marker;
}

/** Whether a status still has a clock the person watches (elapsed, next tick in …). */
export function statusTicks(status: LoopStatus): boolean {
  return status === 'running' || status === 'checking' || status === 'waiting';
}

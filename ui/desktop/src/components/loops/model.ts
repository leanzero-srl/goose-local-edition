/**
 * Session loops — the desktop's view of the loop model (design DESIGN-SESSION-LOOPS.md §4, §9 L0).
 * The TYPES are the generated mirror of goosed's `custom_requests/loops.rs` (one shape for the
 * stored record, the wire and this file); the RULES here mirror `crates/goose/src/session_loops/
 * rules.rs`, and both suites run `session_loops/loops.fixture.json`, whose expectations an
 * independent Ruby encoding of the design computed. The runner-only rules (the check's output tail,
 * the tick prompt, `wrote`) live in goosed alone: the desktop never builds a prompt or reads a
 * check's output.
 *
 * No rule fills a missing input with a default: a self-paced tick that named no delay is
 * `waiting_you`, a check that could not run pauses with its error, an unreadable time is an error
 * with its words (every fallible rule returns a `Checked` value, never throws a guess).
 */
import type {
  LoopCadence,
  LoopCheckRun,
  LoopControlAction,
  LoopEdit,
  LoopNextReason,
  LoopNextTick,
  LoopOwner,
  LoopRecord,
  LoopRefusal,
  LoopRefusalCode,
  LoopReport,
  LoopStatus,
  LoopStatusReason,
  LoopSummaryDto,
  LoopTemplateDto,
  LoopTemplateId,
  LoopTickOrigin,
  LoopTickOutcome,
  LoopTickRecord,
  LoopVerdict,
} from '@aaif/goose-sdk';

export type {
  LoopCadence,
  LoopCheckRun,
  LoopControlAction,
  LoopEdit,
  LoopNextReason,
  LoopNextTick,
  LoopOwner,
  LoopRecord,
  LoopRefusal,
  LoopRefusalCode,
  LoopReport,
  LoopStatus,
  LoopStatusReason,
  LoopSummaryDto,
  LoopTemplateDto,
  LoopTemplateId,
  LoopTickOrigin,
  LoopTickOutcome,
  LoopTickRecord,
  LoopVerdict,
};

export type Checked<T> = { ok: true; value: T } | { ok: false; error: string };

const ok = <T>(value: T): Checked<T> => ({ ok: true, value });
const fail = <T>(error: string): Checked<T> => ({ ok: false, error });

export const TICK_ID_PREFIX = 'looptick_';
export const LOOP_ID_PREFIX = 'lp_';

// ------------------------------------------------------------------------------------------------
// Time
// ------------------------------------------------------------------------------------------------

const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

/** Milliseconds since the epoch of an RFC 3339 time. */
export function parseTime(text: string): Checked<number> {
  const ms = RFC3339.test(text) ? Date.parse(text) : NaN;
  return Number.isFinite(ms) ? ok(ms) : fail(`"${text}" is not an RFC 3339 time`);
}

/** The record's canonical form: RFC 3339, UTC, whole seconds. */
export function fmtTime(ms: number): string {
  return new Date(Math.floor(ms / 1000) * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');
}

// ------------------------------------------------------------------------------------------------
// Cadence
// ------------------------------------------------------------------------------------------------

/** The bounds of the Rust side's integer and duration types (i64, chrono's TimeDelta in seconds). */
const I64_MAX = 9223372036854775807n;
const MAX_DELTA_SECS = 9223372036854775n;
const UNIT_SECS: Record<string, bigint> = { s: 1n, m: 60n, h: 3600n };

function cadenceParts(text: string): { n: bigint; unit: string } | null {
  const chars = Array.from(text.trim());
  if (chars.length === 0) return null;
  const unit = chars[chars.length - 1];
  const num = chars.slice(0, -1).join('').trim();
  if (!/^[+-]?\d+$/.test(num)) return null;
  const n = BigInt(num);
  if (n <= 0n || n > I64_MAX) return null;
  const mult = UNIT_SECS[unit];
  if (mult === undefined || n * mult > MAX_DELTA_SECS) return null;
  return { n, unit };
}

/** Seconds of `<n>s|m|h` (n > 0) — the one cadence grammar goosed's `loop_clock` reads. */
export function parseCadenceSeconds(text: string): number | null {
  const parts = cadenceParts(text);
  return parts ? Number(parts.n * UNIT_SECS[parts.unit]) : null;
}

/** "every 10 min", "goose decides when", "back to back"; an unreadable cadence as typed. */
export function cadenceLabel(cadence: LoopCadence): string {
  switch (cadence.kind) {
    case 'self_paced':
      return 'goose decides when';
    case 'back_to_back':
      return 'back to back';
    case 'every': {
      const parts = cadenceParts(cadence.every);
      if (!parts) return `every ${cadence.every.trim()}`;
      const unit = parts.unit === 's' ? 's' : parts.unit === 'm' ? 'min' : 'h';
      return `every ${parts.n} ${unit}`;
    }
  }
}

/** "45s", "6m 12s", "41m", "2h", "1h 5m". */
export function durationWords(seconds: number): string {
  const s = Math.max(0, Math.trunc(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  if (m > 0) return sec > 0 ? `${m}m ${sec}s` : `${m}m`;
  return `${sec}s`;
}

/** `HH:MM` at a UTC offset in minutes (the viewer's own: `-new Date().getTimezoneOffset()`). */
export function clockTime(ms: number, utcOffsetMinutes: number): Checked<string> {
  if (!Number.isInteger(utcOffsetMinutes) || Math.abs(utcOffsetMinutes * 60) >= 86400) {
    return fail(`${utcOffsetMinutes} minutes is not a UTC offset`);
  }
  const local = new Date(ms + utcOffsetMinutes * 60_000);
  const pad = (v: number) => String(v).padStart(2, '0');
  return ok(`${pad(local.getUTCHours())}:${pad(local.getUTCMinutes())}`);
}

/**
 * How a sentence writes a clock time: `clockTime` is the rule's own `HH:MM` (goosed's words and the
 * fixture's); a surface a person reads beside the chat passes the chat's clock (loopView
 * `chatClockTime`, Q-316) so the rail never says "06:45" beside a transcript's "6:48 AM".
 */
export type LoopClock = (ms: number, utcOffsetMinutes: number) => Checked<string>;

function secondsBetween(fromMs: number, toMs: number): number {
  return Math.trunc((toMs - fromMs) / 1000);
}

// ------------------------------------------------------------------------------------------------
// The next tick (§4.3)
// ------------------------------------------------------------------------------------------------

export type NextTickDecision =
  | { kind: 'at'; next: LoopNextTick }
  | { kind: 'after_reviewers'; n: number }
  | { kind: 'waiting_you'; reason: LoopStatusReason };

function due(
  atMs: number,
  reason: LoopNextReason,
  nowMs: number,
  lastN: number,
  reviewersPending: boolean
): NextTickDecision {
  if (atMs <= nowMs && reviewersPending) return { kind: 'after_reviewers', n: lastN };
  return { kind: 'at', next: { at: fmtTime(atMs), reason } };
}

function reachable(ms: number): boolean {
  return Number.isFinite(new Date(ms).getTime());
}

/** When the tick after the record's last one starts. The last tick must have ended. */
export function nextTick(
  record: LoopRecord,
  nowMs: number,
  reviewersPending: boolean
): Checked<NextTickDecision> {
  const ticks = record.ticks ?? [];
  const last = ticks[ticks.length - 1];
  if (!last) return ok({ kind: 'at', next: { at: fmtTime(nowMs), reason: { kind: 'first' } } });
  if (!last.endedAt) return fail(`tick ${last.n} has not ended`);
  const ended = parseTime(last.endedAt);
  if (!ended.ok) return ended;
  const cadence = record.cadence;
  if (cadence.kind === 'every') {
    const secs = parseCadenceSeconds(cadence.every);
    if (secs === null)
      return fail(`the loop's cadence "${cadence.every}" is not <n>s, <n>m or <n>h`);
    const started = parseTime(last.startedAt);
    if (!started.ok) return started;
    const candidate = started.value + secs * 1000;
    if (!reachable(candidate)) {
      return fail(`the cadence "${cadence.every}" reaches past the last date goose can hold`);
    }
    return ok(
      candidate > nowMs
        ? due(candidate, { kind: 'cadence' }, nowMs, last.n, reviewersPending)
        : due(nowMs, { kind: 'overdue' }, nowMs, last.n, reviewersPending)
    );
  }
  if (cadence.kind === 'self_paced') {
    const given = last.report?.nextIn?.trim();
    if (!given) return ok({ kind: 'waiting_you', reason: { kind: 'no_delay', n: last.n } });
    const secs = parseCadenceSeconds(given);
    const at = secs === null ? NaN : ended.value + secs * 1000;
    if (secs === null || !reachable(at)) {
      return ok({ kind: 'waiting_you', reason: { kind: 'bad_delay', n: last.n, given } });
    }
    const reason: LoopNextReason = { kind: 'self_paced', interval: given };
    if (last.report?.nextReason != null) reason.reason = last.report.nextReason;
    return ok(due(Math.max(at, nowMs), reason, nowMs, last.n, reviewersPending));
  }
  return ok(due(nowMs, { kind: 'back_to_back' }, nowMs, last.n, reviewersPending));
}

// ------------------------------------------------------------------------------------------------
// After a tick (§4.6)
// ------------------------------------------------------------------------------------------------

export type CancelCause =
  | { kind: 'yield'; toSession: string; toChat: string; way?: string | null }
  | { kind: 'loop_stopped' };

export type TickEnd =
  | { kind: 'completed' }
  | { kind: 'cancelled'; cause?: CancelCause | null }
  | { kind: 'errored'; errorClass: string; error: string };

export interface AskedItem {
  itemId: string;
  question: string;
}

export interface TickFacts {
  end: TickEnd;
  check?: LoopCheckRun | null;
  asked?: AskedItem | null;
  reviewersPending?: boolean;
  now: string;
}

export interface Decision {
  outcome: LoopTickOutcome;
  status: LoopStatus;
  reason?: LoopStatusReason;
  nextTick?: LoopNextTick;
}

function outcomeOf(tick: LoopTickRecord, facts: TickFacts): LoopTickOutcome {
  const end = facts.end;
  if (end.kind === 'cancelled' && end.cause?.kind === 'yield') {
    const outcome: LoopTickOutcome = {
      kind: 'yielded',
      toSession: end.cause.toSession,
      toChat: end.cause.toChat,
    };
    if (end.cause.way != null) outcome.way = end.cause.way;
    return outcome;
  }
  if (end.kind === 'cancelled') return { kind: 'stopped_by_you' };
  if (facts.asked)
    return { kind: 'asked', itemId: facts.asked.itemId, question: facts.asked.question };
  if (end.kind === 'errored')
    return { kind: 'failed', errorClass: end.errorClass, error: end.error };
  if (!tick.report) return { kind: 'no_report' };
  return { kind: tick.report.verdict };
}

function normalizedStep(step: string): string {
  return step.split(/\s+/).filter(Boolean).join(' ').toLowerCase();
}

/**
 * Stalled: `cur` reported progress, made no write or edit outside the state file (`wrote`
 * excludes it), and named the same next step as `prev`.
 */
export function stalled(prev: LoopTickRecord, cur: LoopTickRecord): boolean {
  if (!prev.report || !cur.report) return false;
  return (
    cur.report.verdict === 'progress' &&
    (cur.wrote ?? []).length === 0 &&
    normalizedStep(prev.report.nextStep) === normalizedStep(cur.report.nextStep)
  );
}

/** Whether the check must run after a tick that ended this way. */
export function checkRunsAfter(
  end: TickEnd,
  report: LoopReport | null | undefined,
  asked: boolean
) {
  return (
    end.kind === 'completed' &&
    !asked &&
    !!report &&
    (report.verdict === 'progress' || report.verdict === 'done')
  );
}

/** The outcome of the tick that just ended and what the loop does next, in the design's order. */
export function decideAfterTick(record: LoopRecord, facts: TickFacts): Checked<Decision> {
  const now = parseTime(facts.now);
  if (!now.ok) return now;
  const ticks = record.ticks ?? [];
  const tick = ticks[ticks.length - 1];
  if (!tick) return fail('the loop has no tick to decide on');
  const n = tick.n;
  const prev = ticks.length >= 2 ? ticks[ticks.length - 2] : undefined;
  const outcome = outcomeOf(tick, facts);
  const decided = (status: LoopStatus, reason: LoopStatusReason): Checked<Decision> =>
    ok({ outcome, status, reason });
  const end = facts.end;

  if (end.kind === 'cancelled' && end.cause?.kind === 'loop_stopped') {
    return decided('ended', { kind: 'stopped_by_you', n });
  }
  const checked = outcome.kind === 'progress' || outcome.kind === 'done';
  let run: LoopCheckRun | undefined;
  if (record.check != null && checked) {
    if (!facts.check)
      return fail(`the check \`${record.check}\` has no run recorded after tick ${n}`);
    run = facts.check;
    if (run.ran && run.exit === 0) {
      return decided('ended', { kind: 'goal_met', n, check: record.check });
    }
  }
  if (record.check == null && outcome.kind === 'done') {
    return decided('ended', { kind: 'reported_done', n });
  }
  const k = record.stopAfterTicks;
  if (k != null && n >= k) return decided('ended', { kind: 'reached_count', k });

  switch (outcome.kind) {
    case 'stopped_by_you':
      return decided('paused', { kind: 'you_stopped_tick', n });
    case 'yielded':
      return decided('waiting_turn', {
        kind: 'user_turn',
        sessionId: outcome.toSession,
        chat: outcome.toChat,
      });
    case 'asked':
      return decided('needs_you', {
        kind: 'asked',
        n,
        itemId: outcome.itemId,
        question: outcome.question,
      });
    case 'blocked': {
      const said = tick.report?.blockedOn?.trim();
      const blockedOn = said ? said : `tick ${n} did not say what it is blocked on`;
      return decided('paused', { kind: 'blocked', n, blockedOn });
    }
  }
  if (run && !run.ran) {
    const error = run.error ?? 'the check did not start and named no error';
    return decided('paused', { kind: 'check_could_not_run', n, error });
  }
  if (
    outcome.kind === 'failed' &&
    prev?.outcome?.kind === 'failed' &&
    prev.outcome.errorClass === outcome.errorClass
  ) {
    return decided('paused', { kind: 'same_failure_twice', prev: prev.n, n, error: outcome.error });
  }
  if (outcome.kind === 'no_report' && prev?.outcome?.kind === 'no_report') {
    return decided('paused', { kind: 'no_report_twice', prev: prev.n, n });
  }
  if (outcome.kind === 'progress' && prev && stalled(prev, tick)) {
    return decided('paused', { kind: 'stalled', prev: prev.n, n });
  }

  const next = nextTick(record, now.value, facts.reviewersPending ?? false);
  if (!next.ok) return next;
  switch (next.value.kind) {
    case 'at':
      return ok({ outcome, status: 'waiting', nextTick: next.value.next });
    case 'after_reviewers':
      return ok({
        outcome,
        status: 'waiting_turn',
        reason: { kind: 'reviewers', n: next.value.n },
      });
    case 'waiting_you':
      return ok({ outcome, status: 'waiting_you', reason: next.value.reason });
  }
}

// ------------------------------------------------------------------------------------------------
// Who runs the clock (§5.1)
// ------------------------------------------------------------------------------------------------

export type OwnerProof =
  | { kind: 'this_process' }
  | { kind: 'live' }
  | { kind: 'gone'; why: string }
  | { kind: 'unproven'; why: string };

export interface EffectiveStatus {
  status: LoopStatus;
  reason?: LoopStatusReason;
}

/** The status as read now: proven gone → paused (closed), another live goose → elsewhere. */
export function effectiveStatus(record: LoopRecord, proof: OwnerProof | null): EffectiveStatus {
  const written: EffectiveStatus = { status: record.status };
  if (record.statusReason) written.reason = record.statusReason;
  if (record.status === 'paused' || record.status === 'ended') return written;
  if (proof === null || proof.kind === 'gone') {
    return { status: 'paused', reason: { kind: 'closed' } };
  }
  if (proof.kind === 'live') return { status: 'elsewhere' };
  return written;
}

/** How many ticks came due while no goose ran the clock. */
export function ticksDue(record: LoopRecord, nowMs: number): Checked<number> {
  if (!record.nextTick) return ok(0);
  const at = parseTime(record.nextTick.at);
  if (!at.ok) return at;
  if (at.value > nowMs) return ok(0);
  if (record.cadence.kind !== 'every') return ok(1);
  const step = parseCadenceSeconds(record.cadence.every);
  if (step === null) {
    return fail(`the loop's cadence "${record.cadence.every}" is not <n>s, <n>m or <n>h`);
  }
  return ok(Math.floor(secondsBetween(at.value, nowMs) / step) + 1);
}

// ------------------------------------------------------------------------------------------------
// The status sentence (§8.4)
// ------------------------------------------------------------------------------------------------

/** A sentence: its i18n key, its facts, and the English its default message renders. */
export interface Sentence {
  key: string;
  facts: Record<string, string>;
  text: string;
}

const sentence = (key: string, facts: Record<string, string>, text: string): Sentence => ({
  key,
  facts,
  text,
});

/** The NOW line of the rail for a status as read now, with every fact it names. */
export function statusSentence(
  record: LoopRecord,
  status: LoopStatus,
  reason: LoopStatusReason | null | undefined,
  nowMs: number,
  utcOffsetMinutes: number,
  clock: LoopClock
): Checked<Sentence> {
  const ticks = record.ticks ?? [];
  const last = ticks[ticks.length - 1];
  const next = String((last?.n ?? 0) + 1);
  const wrong = () =>
    fail<Sentence>(`a ${status} loop cannot carry the reason ${JSON.stringify(reason)}`);
  const hm = (text: string): Checked<string> => {
    const t = parseTime(text);
    return t.ok ? clock(t.value, utcOffsetMinutes) : t;
  };
  const since = (text: string): Checked<string> => {
    const t = parseTime(text);
    return t.ok ? ok(durationWords(secondsBetween(t.value, nowMs))) : t;
  };

  switch (status) {
    case 'running': {
      if (!last) return fail('a running loop has no tick');
      const time = hm(last.startedAt);
      const elapsed = since(last.startedAt);
      if (!time.ok) return time;
      if (!elapsed.ok) return elapsed;
      const n = String(last.n);
      if (last.served) {
        const node = last.served.node;
        return ok(
          sentence(
            'loops.now.runningOn',
            { n, time: time.value, elapsed: elapsed.value, node },
            `Tick ${n} · started ${time.value} · ${elapsed.value} · on ${node}`
          )
        );
      }
      return ok(
        sentence(
          'loops.now.running',
          { n, time: time.value, elapsed: elapsed.value },
          `Tick ${n} · started ${time.value} · ${elapsed.value}`
        )
      );
    }
    case 'checking': {
      const run = last?.check;
      if (!run) return fail('a checking loop has no check run');
      const elapsed = since(run.startedAt);
      if (!elapsed.ok) return elapsed;
      return ok(
        sentence(
          'loops.now.checking',
          { check: run.command, elapsed: elapsed.value },
          `Checking \`${run.command}\` · ${elapsed.value}`
        )
      );
    }
    case 'waiting': {
      const nt = record.nextTick;
      if (!nt) return fail('a waiting loop has no next tick');
      const at = parseTime(nt.at);
      if (!at.ok) return at;
      const time = clock(at.value, utcOffsetMinutes);
      if (!time.ok) return time;
      const r = nt.reason;
      if (r.kind === 'self_paced' && r.reason != null) {
        return ok(
          sentence(
            'loops.now.selfPaced',
            { time: time.value, interval: r.interval, reason: r.reason },
            `Next tick ${time.value} — goose chose ${r.interval}: "${r.reason}"`
          )
        );
      }
      if (r.kind === 'self_paced') {
        return ok(
          sentence(
            'loops.now.selfPacedNoReason',
            { time: time.value, interval: r.interval },
            `Next tick ${time.value} — goose chose ${r.interval} and gave no reason`
          )
        );
      }
      if (at.value <= nowMs) return ok(sentence('loops.now.startsNow', {}, 'Next tick starts now'));
      const rel = durationWords(secondsBetween(nowMs, at.value));
      return ok(
        sentence(
          'loops.now.nextAt',
          { time: time.value, rel },
          `Next tick ${time.value} · in ${rel}`
        )
      );
    }
    case 'waiting_turn':
      switch (reason?.kind) {
        case 'user_turn':
          return ok(
            sentence(
              'loops.now.dueAfterYourTurn',
              { next, chat: reason.chat },
              `Tick ${next} is due — it starts when your turn in "${reason.chat}" ends`
            )
          );
        case 'refused': {
          const refused = reason.refused;
          switch (refused.kind) {
            case 'turn_running':
            case 'queued_message':
              return ok(
                sentence(
                  'loops.now.dueAfterYourMessage',
                  { next },
                  `Tick ${next} is due — it starts after your message here`
                )
              );
            case 'pending_cancel':
              return ok(
                sentence(
                  'loops.now.dueAfterStop',
                  { next },
                  `Tick ${next} is due — it starts when the answer you stopped here has settled`
                )
              );
            case 'load_failed':
              return ok(
                sentence(
                  'loops.now.dueLoadFailed',
                  { next, error: refused.error },
                  `Tick ${next} is due — this chat could not be opened in the window: ${refused.error}`
                )
              );
            case 'submit_failed':
              return ok(
                sentence(
                  'loops.now.dueSubmitFailed',
                  { next, error: refused.error },
                  `Tick ${next} is due — goose refused its message: ${refused.error}`
                )
              );
          }
          return wrong();
        }
        case 'reviewers':
          return ok(
            sentence(
              'loops.now.dueAfterReviewers',
              { next, n: String(reason.n) },
              `Tick ${next} is due — it starts when goose's check of tick ${reason.n} ends`
            )
          );
        case 'way_held':
          return ok(
            sentence(
              'loops.now.dueWayHeld',
              { next, node: reason.node, chat: reason.chat, target: reason.target },
              `Tick ${next} is due — ${reason.node} is answering you in "${reason.chat}"; the tick loads ${reason.target} after`
            )
          );
        default:
          return wrong();
      }
    case 'waiting_you':
      switch (reason?.kind) {
        case 'no_delay':
          return ok(
            sentence(
              'loops.now.noDelay',
              { n: String(reason.n) },
              `Tick ${reason.n} didn't say when to come back.`
            )
          );
        case 'bad_delay':
          return ok(
            sentence(
              'loops.now.badDelay',
              { n: String(reason.n), given: reason.given },
              `Tick ${reason.n} named a delay goose can't read: "${reason.given}".`
            )
          );
        default:
          return wrong();
      }
    case 'needs_you':
      switch (reason?.kind) {
        case 'asked':
          return ok(
            sentence(
              'loops.now.asked',
              { n: String(reason.n), question: reason.question },
              `Tick ${reason.n} asked you: "${reason.question}"`
            )
          );
        case 'answer_running':
          return ok(
            sentence(
              'loops.now.answerRunning',
              { n: String(reason.n) },
              `Your answer to tick ${reason.n} is running — the next tick starts after it`
            )
          );
        default:
          return wrong();
      }
    case 'paused':
      switch (reason?.kind) {
        case 'by_you':
          return reason.afterTick === 0
            ? ok(
                sentence(
                  'loops.paused.byYouBeforeFirst',
                  {},
                  'Paused by you before the first tick.'
                )
              )
            : ok(
                sentence(
                  'loops.paused.byYou',
                  { n: String(reason.afterTick) },
                  `Paused by you after tick ${reason.afterTick}.`
                )
              );
        case 'you_stopped_tick':
          return ok(
            sentence(
              'loops.paused.youStoppedTick',
              { n: String(reason.n) },
              `You stopped tick ${reason.n}.`
            )
          );
        case 'blocked':
          return ok(
            sentence(
              'loops.paused.blocked',
              { blockedOn: reason.blockedOn },
              `Blocked — ${reason.blockedOn}`
            )
          );
        case 'check_could_not_run':
          return ok(
            sentence(
              'loops.paused.checkCouldNotRun',
              { error: reason.error },
              `The check could not run: ${reason.error}`
            )
          );
        case 'same_failure_twice':
          return ok(
            sentence(
              'loops.paused.sameFailureTwice',
              { prev: String(reason.prev), n: String(reason.n), error: reason.error },
              `Ticks ${reason.prev} and ${reason.n} failed the same way: ${reason.error}`
            )
          );
        case 'no_report_twice':
          return ok(
            sentence(
              'loops.paused.noReportTwice',
              { prev: String(reason.prev), n: String(reason.n) },
              `Ticks ${reason.prev} and ${reason.n} ended without a loop report`
            )
          );
        case 'stalled':
          return ok(
            sentence(
              'loops.paused.stalled',
              { prev: String(reason.prev), n: String(reason.n) },
              `Stalled — tick ${reason.n} named the same next step as tick ${reason.prev} and made no write or edit outside the state file`
            )
          );
        case 'closed': {
          const due = ticksDue(record, nowMs);
          if (!due.ok) return due;
          const were = due.value === 1 ? '1 tick was due' : `${due.value} ticks were due`;
          if (reason.closedAt != null) {
            const time = hm(reason.closedAt);
            if (!time.ok) return time;
            return ok(
              sentence(
                'loops.paused.closedAt',
                { time: time.value, due: String(due.value) },
                `goose was closed at ${time.value}; ${were}.`
              )
            );
          }
          return ok(
            sentence(
              'loops.paused.closed',
              { due: String(due.value) },
              `goose was closed; ${were}.`
            )
          );
        }
        case 'finishing_elsewhere':
          return ok(
            sentence(
              'loops.paused.finishingElsewhere',
              { n: String(reason.n) },
              `Paused — tick ${reason.n} is finishing in the other window`
            )
          );
        default:
          return wrong();
      }
    case 'ended':
      switch (reason?.kind) {
        case 'goal_met':
          return ok(
            sentence(
              'loops.ended.goalMet',
              { n: String(reason.n), check: reason.check },
              `Goal met — \`${reason.check}\` passed after tick ${reason.n}`
            )
          );
        case 'reported_done':
          return ok(
            sentence(
              'loops.ended.reportedDone',
              { n: String(reason.n) },
              `goose reported the goal done after tick ${reason.n} — no check was set`
            )
          );
        case 'reached_count':
          return ok(
            sentence(
              'loops.ended.reachedCount',
              { k: String(reason.k) },
              `Reached ${reason.k} ticks, as you set`
            )
          );
        case 'stopped_by_you':
          return ok(
            sentence(
              'loops.ended.stoppedByYou',
              { n: String(reason.n) },
              `Stopped by you after tick ${reason.n}`
            )
          );
        default:
          return wrong();
      }
    case 'elsewhere':
      return ok(sentence('loops.now.elsewhere', {}, 'This loop runs in another goose window.'));
  }
}

// ------------------------------------------------------------------------------------------------
// Tick ids and ranges
// ------------------------------------------------------------------------------------------------

export interface TickId {
  loopId: string;
  n: number;
  uuid: string;
}

export function tickId(loopId: string, n: number, uuid: string): string {
  return `${TICK_ID_PREFIX}${loopId}_${n}_${uuid}`;
}

const LOOP_ID = /^lp_[0-9a-f]{8}$/;

/** The parts of a tick id; `null` for any other id (a typed message is never read as a tick). */
export function parseTickId(id: string): TickId | null {
  if (!id.startsWith(TICK_ID_PREFIX)) return null;
  const rest = id.slice(TICK_ID_PREFIX.length);
  const last = rest.lastIndexOf('_');
  if (last < 0) return null;
  const uuid = rest.slice(last + 1);
  const head = rest.slice(0, last);
  const middle = head.lastIndexOf('_');
  if (middle < 0) return null;
  const n = head.slice(middle + 1);
  const loopId = head.slice(0, middle);
  if (!/^[0-9a-f-]+$/.test(uuid) || !/^[1-9][0-9]*$/.test(n) || !LOOP_ID.test(loopId)) return null;
  return { loopId, n: Number(n), uuid };
}

export interface TickRange {
  n: number;
  /** `[start, end)` over the message list; `null` when an edit removed the tick's marker. */
  range: [number, number] | null;
}

/** Each tick's messages, from its marker to the next marker or the end. */
export function tickRanges(
  messageIds: readonly (string | null | undefined)[],
  ticks: readonly LoopTickRecord[]
): TickRange[] {
  const starts = ticks.map((tick) => {
    const at = messageIds.indexOf(tick.firstMessageId);
    return at < 0 ? null : at;
  });
  return ticks.map((tick, i) => {
    const start = starts[i];
    if (start === null) return { n: tick.n, range: null };
    const later = starts.filter((s): s is number => s !== null && s > start);
    return { n: tick.n, range: [start, later.length ? Math.min(...later) : messageIds.length] };
  });
}

// ------------------------------------------------------------------------------------------------
// Steps and their slots (§7.4)
// ------------------------------------------------------------------------------------------------

export const SLOTS = [
  'state_file',
  'check',
  'goal_first_line',
  'last_next_step',
  'working_dir',
] as const;

export type LastNextStep =
  | { kind: 'first' }
  | { kind: 'named_none'; prev: number }
  | { kind: 'named'; text: string };

export interface StepFacts {
  stateFile: string;
  check?: string | null;
  goalFirstLine: string;
  lastNextStep: LastNextStep;
  workingDir: string;
}

export interface RenderedSteps {
  text: string;
  /** Slot names goose does not know, left as typed ("{foo} is not a fact goose knows"). */
  unknown: string[];
}

const SLOT = /\{([a-z][a-z0-9_]*)\}/g;

export const NO_CHECK_SENTENCE =
  'no check command is set; run the command that shows the change works and quote it';

/** The slot names the steps use, each once, in first-use order. */
export function stepSlots(steps: string): string[] {
  const names: string[] = [];
  for (const match of steps.matchAll(SLOT)) {
    if (!names.includes(match[1])) names.push(match[1]);
  }
  return names;
}

/** The steps with every known slot filled from the loop's facts; an absent fact says so. */
export function renderSteps(steps: string, facts: StepFacts): RenderedSteps {
  const unknown: string[] = [];
  const text = steps.replace(SLOT, (_whole, name: string) => {
    switch (name) {
      case 'state_file':
        return `\`${facts.stateFile}\``;
      case 'working_dir':
        return `\`${facts.workingDir}\``;
      case 'goal_first_line':
        return `"${facts.goalFirstLine}"`;
      case 'check':
        return facts.check != null ? `\`${facts.check}\`` : NO_CHECK_SENTENCE;
      case 'last_next_step': {
        const last = facts.lastNextStep;
        if (last.kind === 'first') return 'this is the first tick';
        if (last.kind === 'named_none') return `tick ${last.prev} named no next step`;
        return `the last tick named "${last.text}"`;
      }
      default:
        if (!unknown.includes(name)) unknown.push(name);
        return `{${name}}`;
    }
  });
  return { text, unknown };
}

export function goalFirstLine(goal: string): string {
  return (goal.trim().split('\n')[0] ?? '').trim();
}

/** `.goose/loops/<slug>/NOW.md`, the slug from the goal's first four words (editable). */
export function defaultStateFile(goal: string): string {
  const words = goalFirstLine(goal)
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .slice(0, 4);
  return `.goose/loops/${words.length ? words.join('-') : 'loop'}/NOW.md`;
}

// ------------------------------------------------------------------------------------------------
// Starting and editing (§8.2)
// ------------------------------------------------------------------------------------------------

export interface ChatFacts {
  workingDir: string;
  /** The chat builds with the swarm: every tick would start a full build. */
  swarmBuild?: boolean;
}

function relativeStateFile(stateFile: string, workingDir: string): string | 'outside' | 'empty' {
  let path = stateFile.trim();
  if (path.startsWith('/')) {
    const dir = workingDir.replace(/\/+$/, '');
    if (!dir) return 'outside';
    if (!(path === dir || path.startsWith(`${dir}/`))) return 'outside';
    path = path.slice(dir.length);
  }
  const parts: string[] = [];
  for (const segment of path.split('/')) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') {
      if (parts.length === 0) return 'outside';
      parts.pop();
    } else {
      parts.push(segment);
    }
  }
  return parts.length ? parts.join('/') : 'empty';
}

const refusal = (code: LoopRefusalCode, reason: string): { refusal: LoopRefusal } => ({
  refusal: { code, reason },
});

/** The loop as it will be stored, or the one named refusal (the Start dialog's states). */
export function validateLoop(
  edit: LoopEdit,
  chat: ChatFacts
): { ok: LoopEdit } | { refusal: LoopRefusal } {
  if (chat.swarmBuild) {
    return refusal(
      'swarm_build',
      'Loops run chat turns. This chat builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds.'
    );
  }
  if (!edit.goal.trim()) return refusal('empty_goal', 'Say what the loop should do.');
  const check = edit.check?.trim() ? edit.check.trim() : null;
  if (edit.template === 'until_check' && check === null) {
    return refusal('check_required', 'This template needs a command to check.');
  }
  if (edit.cadence.kind === 'every' && parseCadenceSeconds(edit.cadence.every) === null) {
    return refusal('bad_cadence', 'Use a number and s, m or h — 90m, 2h');
  }
  const stateFile = relativeStateFile(edit.stateFile, chat.workingDir);
  if (stateFile === 'outside') {
    return refusal('state_file_outside', `Keep the state file inside ${chat.workingDir}.`);
  }
  if (stateFile === 'empty') {
    return refusal(
      'empty_state_file',
      'Name the state file — goose reads it first and rewrites it last.'
    );
  }
  const unknownSlot = stepSlots(edit.steps ?? '').find(
    (slot) => !(SLOTS as readonly string[]).includes(slot)
  );
  if (unknownSlot !== undefined) {
    return refusal('unknown_slot', `{${unknownSlot}} is not a fact goose knows`);
  }
  if (edit.stopAfterTicks === 0) {
    return refusal(
      'bad_stop_after',
      'Stop after needs at least one tick — leave it empty to run until the goal is met or you stop it.'
    );
  }
  const valid: LoopEdit = {
    goal: edit.goal,
    template: edit.template,
    steps: edit.steps ?? '',
    cadence:
      edit.cadence.kind === 'every'
        ? { kind: 'every', every: edit.cadence.every.trim() }
        : edit.cadence,
    stateFile,
  };
  if (check !== null) valid.check = check;
  if (edit.stopAfterTicks != null) valid.stopAfterTicks = edit.stopAfterTicks;
  return { ok: valid };
}

// ------------------------------------------------------------------------------------------------
// `/loop` (§7.2)
// ------------------------------------------------------------------------------------------------

export type LoopCommandRefusal =
  | 'missing_goal'
  | 'missing_cadence'
  | 'bad_cadence'
  | 'control_takes_no_words';

export type LoopCommand =
  | { kind: 'status' }
  | { kind: 'now' }
  | { kind: 'pause' }
  | { kind: 'resume' }
  | { kind: 'stop' }
  | { kind: 'start'; goal: string; cadence: LoopCadence }
  | { kind: 'refused'; code: LoopCommandRefusal; reason: string };

/** The forms the composer sends straight to `loops/get` / `loops/control` while a turn runs. */
export function isControlCommand(command: LoopCommand): boolean {
  return ['status', 'now', 'pause', 'resume', 'stop'].includes(command.kind);
}

/** The `loops/control` action of a control form (`status` reads, it does not control). */
export function controlAction(command: LoopCommand): LoopControlAction | null {
  switch (command.kind) {
    case 'now':
      return 'tickNow';
    case 'pause':
    case 'resume':
    case 'stop':
      return command.kind;
    default:
      return null;
  }
}

function splitWord(text: string): [string, string] {
  const t = text.trimStart();
  const at = t.search(/\s/);
  return at < 0 ? [t, ''] : [t.slice(0, at), t.slice(at).trim()];
}

const CONTROL_WORDS = ['now', 'pause', 'resume', 'stop'] as const;

/** `null` = the line is not a `/loop` command. The subcommand words are fixed. */
export function parseLoopCommand(line: string): LoopCommand | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith('/loop')) return null;
  let rest = trimmed.slice('/loop'.length);
  if (rest !== '' && !/^\s/.test(rest)) return null;
  rest = rest.trim();
  if (!rest) return { kind: 'status' };
  const [word, after] = splitWord(rest);
  const lower = word.toLowerCase();
  const control = CONTROL_WORDS.find((w) => w === lower);
  if (control) {
    if (!after) return { kind: control };
    return {
      kind: 'refused',
      code: 'control_takes_no_words',
      reason: `/loop ${lower} takes nothing after it — to loop on a goal that starts with "${word}", use the Loop button.`,
    };
  }
  if (lower === 'every') {
    const [every, goal] = splitWord(after);
    if (!every) {
      return {
        kind: 'refused',
        code: 'missing_cadence',
        reason: 'Say how often and what: /loop every 10m <goal>.',
      };
    }
    if (parseCadenceSeconds(every) === null) {
      return {
        kind: 'refused',
        code: 'bad_cadence',
        reason: 'Use a number and s, m or h — 90m, 2h',
      };
    }
    if (!goal) {
      return {
        kind: 'refused',
        code: 'missing_goal',
        reason: `Say what the loop should do: /loop every ${every} <goal>.`,
      };
    }
    return { kind: 'start', goal, cadence: { kind: 'every', every } };
  }
  return { kind: 'start', goal: rest, cadence: { kind: 'self_paced' } };
}

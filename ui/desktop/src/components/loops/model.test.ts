import { describe, expect, it } from 'vitest';
import raw from '../../../../../crates/goose/src/session_loops/loops.fixture.json';
import {
  cadenceLabel,
  controlAction,
  decideAfterTick,
  defaultStateFile,
  durationWords,
  effectiveStatus,
  fmtTime,
  isControlCommand,
  nextTick,
  parseCadenceSeconds,
  parseLoopCommand,
  parseTickId,
  parseTime,
  renderSteps,
  stalled,
  statusSentence,
  stepSlots,
  tickId,
  tickRanges,
  validateLoop,
  type ChatFacts,
  type Checked,
  type LoopCadence,
  type LoopEdit,
  type LoopRecord,
  type LoopStatus,
  type LoopStatusReason,
  type LoopTickRecord,
  type OwnerProof,
  type StepFacts,
  type TickFacts,
} from './model';

/**
 * The fixture goosed is pinned to (`session_loops/tests.rs` runs the very same file). Its
 * expectations come from an independent Ruby encoding of the design, not from either suite.
 */
interface Named {
  name: string;
}
interface Fixture {
  cadences: { text: string; seconds: number | null; label: string }[];
  cadenceLabels: { cadence: LoopCadence; label: string }[];
  durationWords: { seconds: number; words: string }[];
  records: (Named & { record: LoopRecord })[];
  nextTick: (Named & {
    record: LoopRecord;
    now: string;
    reviewersPending: boolean;
    expect: unknown;
  })[];
  nextTickErrors: (Named & { record: LoopRecord; now: string; reviewersPending: boolean })[];
  decide: (Named & { record: LoopRecord; facts: TickFacts; expect: unknown })[];
  decideErrors: (Named & { record: LoopRecord; facts: TickFacts })[];
  stalled: (Named & { prev: LoopTickRecord; cur: LoopTickRecord; expect: boolean })[];
  effective: (Named & { record: LoopRecord; proof: OwnerProof | null; expect: unknown })[];
  sentences: (Named & {
    record: LoopRecord;
    status: LoopStatus;
    reason?: LoopStatusReason;
    now: string;
    utcOffsetMinutes: number;
    expect: unknown;
  })[];
  sentenceErrors: (Named & {
    record: LoopRecord;
    status: LoopStatus;
    reason?: LoopStatusReason;
    now: string;
    utcOffsetMinutes: number;
  })[];
  tickIds: { id: string; expect: unknown }[];
  tickIdMint: { loopId: string; n: number; uuid: string; id: string }[];
  tickRanges: (Named & {
    messageIds: (string | null)[];
    ticks: LoopTickRecord[];
    expect: unknown;
  })[];
  renderSteps: (Named & { steps: string; facts: StepFacts; expect: unknown })[];
  stepSlots: { steps: string; slots: string[] }[];
  defaultStateFile: { goal: string; path: string }[];
  validate: (Named & { edit: LoopEdit; chat: ChatFacts; expect: unknown })[];
  loopCommands: { line: string; expect: unknown }[];
}

const fixture = raw as unknown as Fixture;

function value<T>(checked: Checked<T>, name: string): T {
  if (!checked.ok) throw new Error(`${name}: ${checked.error}`);
  return checked.value;
}

function ms(text: string): number {
  return value(parseTime(text), text);
}

describe('the loops model — the fixture goosed is pinned to', () => {
  it('reads every fixture record as the typed shape and writes it back unchanged', () => {
    expect(fixture.records.length).toBeGreaterThanOrEqual(5);
    for (const { name, record } of fixture.records) {
      expect(JSON.parse(JSON.stringify(record)), name).toEqual(record);
      expect(record.id.startsWith('lp_'), name).toBe(true);
    }
  });

  it('reads the cadence grammar and labels exactly as loop_clock does', () => {
    expect(fixture.cadences.length).toBeGreaterThanOrEqual(15);
    for (const { text, seconds, label } of fixture.cadences) {
      expect(parseCadenceSeconds(text), JSON.stringify(text)).toBe(seconds);
      expect(cadenceLabel({ kind: 'every', every: text }), JSON.stringify(text)).toBe(label);
    }
    for (const { cadence, label } of fixture.cadenceLabels)
      expect(cadenceLabel(cadence)).toBe(label);
    for (const { seconds, words } of fixture.durationWords)
      expect(durationWords(seconds)).toBe(words);
  });

  it('schedules the next tick as the runner does', () => {
    for (const c of fixture.nextTick) {
      expect(value(nextTick(c.record, ms(c.now), c.reviewersPending), c.name), c.name).toEqual(
        c.expect
      );
    }
    for (const c of fixture.nextTickErrors) {
      expect(nextTick(c.record, ms(c.now), c.reviewersPending).ok, c.name).toBe(false);
    }
  });

  it('decides every §4.6 row as the runner does', () => {
    expect(fixture.decide.length).toBeGreaterThanOrEqual(30);
    for (const c of fixture.decide) {
      expect(value(decideAfterTick(c.record, c.facts), c.name), c.name).toEqual(c.expect);
    }
    for (const c of fixture.decideErrors) {
      expect(decideAfterTick(c.record, c.facts).ok, c.name).toBe(false);
    }
  });

  it('calls a tick stalled only on a repeat with nothing written', () => {
    for (const c of fixture.stalled) expect(stalled(c.prev, c.cur), c.name).toBe(c.expect);
  });

  it('derives the status as read now, never from a guess about the owner', () => {
    for (const c of fixture.effective) {
      expect(effectiveStatus(c.record, c.proof), c.name).toEqual(c.expect);
    }
  });

  it('words every status sentence as goosed does, key, facts and text', () => {
    expect(fixture.sentences.length).toBeGreaterThanOrEqual(35);
    for (const c of fixture.sentences) {
      const got = statusSentence(c.record, c.status, c.reason, ms(c.now), c.utcOffsetMinutes);
      expect(value(got, c.name), c.name).toEqual(c.expect);
    }
    for (const c of fixture.sentenceErrors) {
      expect(
        statusSentence(c.record, c.status, c.reason, ms(c.now), c.utcOffsetMinutes).ok,
        c.name
      ).toBe(false);
    }
  });

  it('reads and mints tick ids exactly as goosed does', () => {
    for (const { id, expect: want } of fixture.tickIds) expect(parseTickId(id), id).toEqual(want);
    for (const c of fixture.tickIdMint) {
      expect(tickId(c.loopId, c.n, c.uuid)).toBe(c.id);
      expect(parseTickId(c.id)?.n).toBe(c.n);
    }
  });

  it("cuts each tick's messages from its marker to the next, naming a removed marker", () => {
    for (const c of fixture.tickRanges) {
      expect(tickRanges(c.messageIds, c.ticks), c.name).toEqual(c.expect);
    }
  });

  it("fills the steps' slots from the loop's facts and names every absence", () => {
    for (const c of fixture.renderSteps)
      expect(renderSteps(c.steps, c.facts), c.name).toEqual(c.expect);
    for (const c of fixture.stepSlots) expect(stepSlots(c.steps)).toEqual(c.slots);
    for (const c of fixture.defaultStateFile) expect(defaultStateFile(c.goal), c.goal).toBe(c.path);
  });

  it('validates a start or an edit with the one named refusal', () => {
    expect(fixture.validate.length).toBeGreaterThanOrEqual(15);
    for (const c of fixture.validate)
      expect(validateLoop(c.edit, c.chat), c.name).toEqual(c.expect);
  });

  it('parses /loop exactly as the server does, and never reads "stop" as a goal', () => {
    for (const { line, expect: want } of fixture.loopCommands) {
      expect(parseLoopCommand(line), JSON.stringify(line)).toEqual(want);
    }
  });
});

describe('the loops model — beyond the fixture', () => {
  it("writes times in the record's canonical form and refuses a time that is not RFC 3339", () => {
    expect(fmtTime(Date.UTC(2026, 8, 27, 22, 1, 0, 999))).toBe('2026-09-27T22:01:00Z');
    expect(parseTime('2026-09-27T22:01:00Z').ok).toBe(true);
    expect(parseTime('2026-09-27 22:01').ok).toBe(false);
    expect(parseTime('yesterday').ok).toBe(false);
  });

  it("maps the composer's control forms to loops/control actions", () => {
    const at = (line: string) => parseLoopCommand(line)!;
    expect(isControlCommand(at('/loop stop'))).toBe(true);
    expect(isControlCommand(at('/loop'))).toBe(true);
    expect(isControlCommand(at('/loop fix the tests'))).toBe(false);
    expect(controlAction(at('/loop now'))).toBe('tickNow');
    expect(controlAction(at('/loop pause'))).toBe('pause');
    expect(controlAction(at('/loop'))).toBeNull();
  });

  it('refuses an out-of-range UTC offset instead of guessing one', () => {
    const record = fixture.sentences[0].record;
    expect(statusSentence(record, 'running', null, ms('2026-09-27T22:45:00Z'), 24 * 60).ok).toBe(
      false
    );
  });
});

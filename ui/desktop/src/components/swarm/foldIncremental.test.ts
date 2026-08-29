import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { foldEvents, foldEventsIncremental, foldStats, resetFoldCache } from './useSwarmRun';
import {
  eventsGeneration,
  readEvents,
  resetSwarmReadCache,
} from '../../utils/swarmIncrementalRead';

/**
 * `foldEvents` rebuilt every lane from the FULL event array on every 500ms poll — ~380 lines of folding
 * over a log that only ever grows, twice a second, to learn about one appended line. The panel now folds
 * only what was appended, carrying the event-derived state between ticks.
 *
 * The ONE property that makes that safe is proved here: for the same array, the incremental path and the
 * from-scratch path produce identical output, at EVERY split point rather than a convenient one. The rest
 * of the file guards the ways the optimisation could lie — reusing a carry across a different log, missing
 * a digest that moved while the events did not, and serving a carry left half-applied by a throw.
 *
 * The cache key is (runId, generation) FROM MAIN plus the previous length — never anything derived from
 * the array's content. A content fingerprint was tried and it collided: two 40-event arrays differing only
 * at index 7 fingerprinted the same, and the fold served a carry that had never seen the differing event.
 */

type Ev = Record<string, unknown>;

/** main re-reads and re-PARSES nothing, but IPC structured-clones — the renderer never sees the same
 *  object twice, only the same bytes. Every test folds through this so it exercises the real tick. */
const reparse = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

// A realistic stream: fleet resolution, the research/plan phases, three build tasks (one judged into a
// split, one retried), judge looks with revised ETAs, the verify repair wave, and a clean finish.
const EVENTS: Ev[] = [
  {
    event: 'run_started',
    ts: '2026-08-29T09:00:00Z',
    prompt: '# Build `vendorsync`\n\nA thing.',
    planner_model: 'gabee-qwen3.6-27b',
    pool: [
      { id: 'mac-gabee-qwen3.6-27b-fable-fusi', model_id: 'gabee-qwen3.6-27b-mtp' },
      { id: 'mac-mihai-qwen3.6-27b-', model_id: 'mihai-qwen3.6-27b' },
    ],
  },
  {
    event: 'pool_resolved',
    ts: '2026-08-29T09:00:01Z',
    devices: [
      { id: 'mac-gabee-qwen3.6-27b-fable-fusi', model_id: 'gabee-qwen3.6-27b-mtp' },
      { id: 'mac-mihai-qwen3.6-27b-', model_id: 'mihai-qwen3.6-27b' },
      { id: 'mac-workhorse-qwen3.6-27b', model_id: 'workhorse-qwen3.6-27b' },
    ],
  },
  { event: 'judge_look', ts: '2026-08-29T09:00:20Z', task_id: 'open-coverage-1', eta_mins: 5 },
  { event: 'judge_look', ts: '2026-08-29T09:00:40Z', task_id: 'open-coverage-1', eta_mins: 2 },
  { event: 'scouts_planned', ts: '2026-08-29T09:00:02Z', lenses: ['api', 'storage'] },
  { event: 'research_completed', ts: '2026-08-29T09:01:00Z', findings: 4 },
  { event: 'pillars', ts: '2026-08-29T09:01:10Z' },
  {
    event: 'plan_loaded',
    ts: '2026-08-29T09:02:00Z',
    plan_confidence: 78,
    task_count: 3,
    tasks: [
      {
        id: 'lexer',
        description:
          '\n\n**Subtask: [lexer] Tokenize the template source**\n\n**Owned files:** src/lexer.rs',
        files: ['src/lexer.rs'],
      },
      {
        id: 'store',
        description:
          '\n\n**Subtask: [store] Persist objects to disk**\n\n**Owned files:** src/store.rs',
        files: ['src/store.rs'],
      },
      {
        id: 'integrate-verify',
        description: '\n\n**Subtask: [integrate-verify] Wire it up and prove it runs**',
        files: [],
      },
    ],
  },
  {
    event: 'task_dispatched',
    ts: '2026-08-29T09:02:05Z',
    task_id: 'lexer',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model: 'gabee-qwen3.6-27b-mtp',
  },
  // An event name the fold does not know, carrying a task_id, must fall straight through.
  { event: 'judge_observed', ts: '2026-08-29T09:02:31Z', task_id: 'lexer' },
  {
    event: 'judge_verdict',
    ts: '2026-08-29T09:03:01Z',
    task_id: 'lexer',
    verdict: 'over_reading',
    action: 'nudge',
  },
  {
    event: 'task_dispatched',
    ts: '2026-08-29T09:03:10Z',
    task_id: 'store',
    device: 'mac-mihai-qwen3.6-27b-',
    model: 'mihai-qwen3.6-27b',
  },
  {
    event: 'task_retry',
    ts: '2026-08-29T09:04:00Z',
    task_id: 'store',
    from_device: 'mac-workhorse-qwen3.6-27b',
    error: 'the worker returned without touching src/store.rs',
  },
  {
    event: 'task_completed',
    ts: '2026-08-29T09:05:00Z',
    task_id: 'lexer',
    status: 'done',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model: 'gabee-qwen3.6-27b-mtp',
    elapsed_ms: 175000,
    attempts: 1,
    tool_calls: [{ name: 'developer__write' }, { name: 'developer__shell' }],
  },
  // A task the judge decomposes: the parent lane must DISAPPEAR, and it must disappear the same way
  // whether the split arrived in this fold or a previous one.
  {
    event: 'task_dispatched',
    ts: '2026-08-29T09:05:05Z',
    task_id: 'renderer',
    device: 'mac-workhorse-qwen3.6-27b',
    model: 'workhorse-qwen3.6-27b',
  },
  {
    event: 'judge_verdict',
    ts: '2026-08-29T09:05:40Z',
    task_id: 'renderer',
    verdict: 'too_large',
    action: 'split',
  },
  {
    event: 'task_completed',
    ts: '2026-08-29T09:07:00Z',
    task_id: 'store',
    status: 'failed',
    device: 'mac-mihai-qwen3.6-27b-',
    elapsed_ms: 231000,
    attempts: 2,
    tool_calls: [{ name: 'developer__shell' }],
  },
  {
    event: 'task_dispatched',
    ts: '2026-08-29T09:07:30Z',
    task_id: 'integrate-verify',
    device: 'mac-workhorse-qwen3.6-27b',
    model: 'workhorse-qwen3.6-27b',
  },
  {
    event: 'complete_fix_dispatched',
    ts: '2026-08-29T09:09:00Z',
    task_id: 'complete-fix::twin0',
    model: 'gabee-qwen3.6-27b-mtp',
    round: 1,
    twin: 0,
  },
  {
    event: 'complete_fix_dispatched',
    ts: '2026-08-29T09:09:01Z',
    task_id: 'complete-fix::twin1',
    model: 'mihai-qwen3.6-27b',
    round: 1,
    twin: 1,
  },
  // No task_id — pre-fix streams reconstruct it from `twin`, and that reconstruction has to survive being
  // folded in a later chunk than the dispatch it closes.
  { event: 'complete_fix_completed', ts: '2026-08-29T09:18:00Z', twin: 0 },
  { event: 'smoke', ts: '2026-08-29T09:19:00Z', result: { ran: true } },
  // Closes every twin still running, including twin1, whose own completion was lost.
  { event: 'spec_repair_wave', ts: '2026-08-29T09:19:30Z', round: 2 },
  {
    event: 'task_completed',
    ts: '2026-08-29T09:20:00Z',
    task_id: 'integrate-verify',
    status: 'done',
    device: 'mac-workhorse-qwen3.6-27b',
    elapsed_ms: 750000,
    attempts: 1,
    tool_calls: [{ name: 'developer__shell' }],
  },
  {
    event: 'run_finished',
    ts: '2026-08-29T09:20:30Z',
    report: { done: ['lexer', 'integrate-verify'], failed: ['store'] },
  },
];

const ACTIVITY: Record<string, unknown> = {
  lexer: {
    tool_calls: 12,
    errors: 1,
    recent: ['Wrote src/lexer.rs', 'Ran the tests'],
    last_text: 'Tokenizer covers the delimiter cases now.',
    reasoning: 'I need to handle nested delimiters before the escape rules.',
    full_reasoning: 'I need to handle nested delimiters before the escape rules. Then the escapes.',
    calls: [
      { name: 'developer__write', summary: 'src/lexer.rs', ok: true, result: 'ok' },
      { name: 'developer__shell', summary: 'cargo test', ok: false, result: '3 failed' },
    ],
    thinking_chars: 4200,
    last_thinking: 'the escape rules are the tricky part',
    full_thinking: 'the escape rules are the tricky part, and the nesting',
    judging: false,
    queued_chunks: 0,
  },
  store: { tool_calls: 5, errors: 2, recent: ['Ran a shell command'], thinking_chars: 900 },
  'integrate-verify': {
    tool_calls: 41,
    recent: ['Built the project'],
    last_text: 'Entry point runs.',
    phase: 'processing',
    judging: true,
    queued_chunks: 7,
  },
  'complete-fix::twin0': { tool_calls: 3, last_text: 'Patched the missing store write.' },
  'complete-fix::twin1': { tool_calls: 2, thinking_chars: 1500 },
  'plandraft-0': {
    model: 'gabee-qwen3.6-27b-mtp',
    full_reasoning: 'Split it into a lexer, a store and a sink.',
    thinking_chars: 3000,
  },
  'plandraft-1': {
    model: 'mihai-qwen3.6-27b',
    phase: 'done',
    last_text: 'Three modules is enough.',
  },
  'scout-api': { model: 'workhorse-qwen3.6-27b', last_text: 'The API surface is small.' },
  'scout-storage': {
    model: 'gabee-qwen3.6-27b-mtp',
    calls: [{ name: 'developer__shell', summary: 'rg store', ok: true }],
  },
  'contract-store': { model: 'mihai-qwen3.6-27b', thinking_chars: 700 },
  'detail-lexer': { model: 'gabee-qwen3.6-27b-mtp', phase: 'processing' },
  'slice-storage': {
    model: 'workhorse-qwen3.6-27b',
    last_text: 'The store owns the on-disk format.',
  },
  open: { model: 'gabee-qwen3.6-27b-mtp', last_text: 'Four slices.' },
  'open-coverage-1': { model: 'mihai-qwen3.6-27b', thinking_chars: 2100 },
  synthesis: { model: 'workhorse-qwen3.6-27b', last_text: 'Merged the drafts.' },
  // No narration, no calls, no thinking, no phase — must stay filtered out on either path.
  'scout-empty': { model: 'workhorse-qwen3.6-27b', tool_calls: 0 },
};

const src = (runId: string, generation: number) => ({ runId, generation });

beforeEach(() => resetFoldCache());

describe('the incremental fold — a prefix plus the remainder equals the whole', () => {
  it('agrees with a from-scratch fold at EVERY split point', () => {
    const whole = foldEvents(EVENTS, ACTIVITY);
    for (let split = 0; split <= EVENTS.length; split++) {
      resetFoldCache();
      foldEventsIncremental(reparse(EVENTS.slice(0, split)), ACTIVITY, src('run-a', 7));
      const after = foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
      expect({ split, ...after }).toStrictEqual({ split, ...whole });
    }
  });

  it('agrees when the stream arrives in many small appends, as the 500ms tick delivers it', () => {
    let last = foldEventsIncremental([], ACTIVITY, src('run-a', 7));
    for (let n = 1; n <= EVENTS.length; n++) {
      const prefix = reparse(EVENTS.slice(0, n));
      last = foldEventsIncremental(prefix, ACTIVITY, src('run-a', 7));
      expect(last).toStrictEqual(foldEvents(prefix, ACTIVITY));
    }
    expect(last).toStrictEqual(foldEvents(EVENTS, ACTIVITY));
    // One event per tick, so the fold did exactly one event's work per tick — never the whole log again.
    expect(foldStats().eventsFolded).toBe(EVENTS.length);
    expect(foldStats().fullFolds).toBe(1);
  });

  it('folds only the appended events, not the log, when the log jumps forward', () => {
    foldEventsIncremental(reparse(EVENTS.slice(0, 10)), ACTIVITY, src('run-a', 7));
    const before = foldStats().eventsFolded;
    foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    expect(foldStats().eventsFolded - before).toBe(EVENTS.length - 10);
    expect(foldStats().incrementalFolds).toBe(1);
  });

  it('keeps the split-away parent lane away, whichever chunk the split landed in', () => {
    for (const split of [15, 16, 17, 18]) {
      resetFoldCache();
      foldEventsIncremental(reparse(EVENTS.slice(0, split)), ACTIVITY, src('run-a', 7));
      const after = foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
      expect(after.lanes.map((l) => l.taskId)).not.toContain('renderer');
    }
  });

  it('carries the lanes, statuses, devices and repair twins the panel renders', () => {
    foldEventsIncremental(reparse(EVENTS.slice(0, 9)), ACTIVITY, src('run-a', 7));
    const f = foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    expect(f.lanes.map((l) => [l.taskId, l.status, l.device])).toStrictEqual([
      ['lexer', 'done', 'gabee'],
      ['integrate-verify', 'done', 'workhorse'],
      ['store', 'error', 'mihai'],
    ]);
    expect(f.lanes.find((l) => l.taskId === 'store')?.error).toContain('src/store.rs');
    expect(f.lanes.find((l) => l.taskId === 'lexer')?.description).toBe(
      'Tokenize the template source'
    );
    expect(f.totals).toStrictEqual({ tasks: 3, running: 0, done: 2, failed: 1 });
    // Both twins closed: twin0 by its own event, twin1 by the wave that ended over it.
    expect(f.fixLanes.map((l) => [l.taskId, l.status])).toStrictEqual([
      ['complete-fix::twin0', 'done'],
      ['complete-fix::twin1', 'done'],
    ]);
    // The judge's LAST ETA for a planning lane wins, and survives being carried across ticks.
    expect(f.planningLanes.find((l) => l.taskId === 'open-coverage-1')?.judgeEtaMins).toBe(2);
  });
});

describe('the incremental fold — a carry is never reused across a different log', () => {
  it('refolds from scratch when the GENERATION changed (the log was replaced under the same run)', () => {
    const first = reparse(EVENTS.slice(0, 12));
    foldEventsIncremental(first, ACTIVITY, src('run-a', 7));
    const restarted = reparse(EVENTS.slice(0, 14));
    const folded = foldEventsIncremental(restarted, ACTIVITY, src('run-a', 8));
    expect(folded).toStrictEqual(foldEvents(restarted, ACTIVITY));
    expect(foldStats().fullFolds).toBe(2);
    expect(foldStats().incrementalFolds).toBe(0);
  });

  it('refolds from scratch when the RUN ID changed', () => {
    foldEventsIncremental(reparse(EVENTS.slice(0, 12)), ACTIVITY, src('run-a', 7));
    const other = reparse(EVENTS.slice(0, 14));
    const folded = foldEventsIncremental(other, ACTIVITY, src('run-b', 7));
    expect(folded).toStrictEqual(foldEvents(other, ACTIVITY));
    expect(foldStats().fullFolds).toBe(2);
    expect(foldStats().incrementalFolds).toBe(0);
  });

  it('refolds from scratch when the array got SHORTER under the same runId and generation', () => {
    foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    const shorter = reparse(EVENTS.slice(0, 8));
    const folded = foldEventsIncremental(shorter, ACTIVITY, src('run-a', 7));
    expect(folded).toStrictEqual(foldEvents(shorter, ACTIVITY));
    expect(foldStats().fullFolds).toBe(2);
    expect(foldStats().incrementalFolds).toBe(0);
  });

  it('refolds from scratch when main could not name the log at all', () => {
    foldEventsIncremental(reparse(EVENTS.slice(0, 12)), ACTIVITY, null);
    const all = reparse(EVENTS);
    expect(foldEventsIncremental(all, ACTIVITY, null)).toStrictEqual(foldEvents(all, ACTIVITY));
    expect(foldStats().incrementalFolds).toBe(0);
  });

  /**
   * The content fingerprint this key replaced: first/quarter/middle/last event plus the length. Two runs
   * of the same length that differ ONLY in the middle collide under it, and the second one's differing
   * event — here a whole dispatched task — went invisible. The generation is main's own answer, so the
   * two arrays cannot be confused however similar their content is.
   */
  it('does not confuse two same-length logs that differ only in the middle', () => {
    const a: Ev[] = Array.from({ length: 40 }, (_, i) => ({ event: 'noise', i }));
    a[0] = EVENTS[0];
    a[1] = EVENTS[1];
    const b = a.map((e) => ({ ...e }));
    b[7] = {
      event: 'task_dispatched',
      task_id: 'only-in-b',
      device: 'mac-workhorse-qwen3.6-27b',
      model: 'workhorse-qwen3.6-27b',
    };
    expect(a.length).toBe(b.length);

    foldEventsIncremental(reparse(a), {}, src('run-a', 11));
    const folded = foldEventsIncremental(reparse(b), {}, src('run-b', 12));
    expect(folded.lanes.map((l) => l.taskId)).toStrictEqual(['only-in-b']);
    expect(folded).toStrictEqual(foldEvents(b, {}));
  });
});

describe('the incremental fold — only the EVENT half is cached, never the digests', () => {
  it('re-joins the digests on a cache hit, so a lane that moved while the log did not still moves', () => {
    const events = reparse(EVENTS);
    const before = foldEventsIncremental(events, ACTIVITY, src('run-a', 7));
    expect(before.lanes.find((l) => l.taskId === 'integrate-verify')?.queuedChunks).toBe(7);
    expect(before.lanes.find((l) => l.taskId === 'integrate-verify')?.judging).toBe(true);

    const moved = {
      ...ACTIVITY,
      'integrate-verify': {
        ...(ACTIVITY['integrate-verify'] as Record<string, unknown>),
        judging: false,
        queued_chunks: 0,
        last_text: 'The end-to-end run is green.',
        tool_calls: 44,
      },
    };
    const eventsFolded = foldStats().eventsFolded;
    const after = foldEventsIncremental(reparse(EVENTS), moved, src('run-a', 7));

    // Not one event was re-folded, and the lane still changed.
    expect(foldStats().eventsFolded).toBe(eventsFolded);
    expect(foldStats().incrementalFolds).toBe(1);
    const lane = after.lanes.find((l) => l.taskId === 'integrate-verify');
    expect(lane?.judging).toBe(false);
    expect(lane?.queuedChunks).toBe(0);
    expect(lane?.lastText).toBe('The end-to-end run is green.');
    expect(lane?.toolCalls).toBe(44);
    expect(after).toStrictEqual(foldEvents(EVENTS, moved));
  });

  it('picks up a digest that did not exist on the previous tick', () => {
    foldEventsIncremental(reparse(EVENTS), {}, src('run-a', 7));
    const after = foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    expect(after.scoutLanes.map((l) => l.taskId)).toStrictEqual(['scout-api', 'scout-storage']);
    expect(after.sliceLanes.map((l) => l.taskId)).toStrictEqual(['slice-storage']);
    expect(after).toStrictEqual(foldEvents(EVENTS, ACTIVITY));
  });

  it('does not mutate the carried lanes when it joins a digest', () => {
    const events = reparse(EVENTS);
    foldEventsIncremental(events, ACTIVITY, src('run-a', 7));
    const bare = foldEventsIncremental(reparse(EVENTS), {}, src('run-a', 7));
    // The digest fields from the first join must not have leaked into the carry.
    expect(bare.lanes.find((l) => l.taskId === 'lexer')?.lastThinking).toBeUndefined();
    expect(bare).toStrictEqual(foldEvents(EVENTS, {}));
  });
});

describe('the incremental fold — the FIRST pool wins, as `find()` always did', () => {
  const RESTATED: Ev[] = [
    ...EVENTS.slice(0, 2),
    // A second fleet announcement, naming the SAME ids with different models. `find()` never saw it and
    // neither may the accumulator — otherwise a re-announced pool silently renames every node.
    {
      event: 'run_started',
      pool: [{ id: 'mac-gabee-qwen3.6-27b-fable-fusi', model_id: 'imposter-model' }],
    },
    {
      event: 'pool_resolved',
      devices: [{ id: 'mac-workhorse-qwen3.6-27b', model_id: 'imposter-model' }],
    },
    ...EVENTS.slice(2),
  ];

  it('ignores a later run_started / pool_resolved, from scratch and incrementally alike', () => {
    const whole = foldEvents(RESTATED, ACTIVITY);
    expect(whole.lanes.find((l) => l.taskId === 'integrate-verify')?.device).toBe('workhorse');
    for (let split = 0; split <= RESTATED.length; split++) {
      resetFoldCache();
      foldEventsIncremental(reparse(RESTATED.slice(0, split)), ACTIVITY, src('run-a', 7));
      expect(foldEventsIncremental(reparse(RESTATED), ACTIVITY, src('run-a', 7))).toStrictEqual(
        whole
      );
    }
  });

  it('takes the FIRST pool_resolved even when it arrives in a later tick than the first run_started', () => {
    // Split between the two announcements: the accumulator has seen run_started but no pool_resolved yet.
    foldEventsIncremental(reparse(RESTATED.slice(0, 1)), ACTIVITY, src('run-a', 7));
    const after = foldEventsIncremental(reparse(RESTATED), ACTIVITY, src('run-a', 7));
    expect(after.lanes.find((l) => l.taskId === 'lexer')?.device).toBe('gabee');
    expect(after.lanes.find((l) => l.taskId === 'store')?.device).toBe('mihai');
    expect(after.lanes.find((l) => l.taskId === 'integrate-verify')?.device).toBe('workhorse');
  });
});

describe('the incremental fold — a throw mid-fold leaves no cache to poison the next tick', () => {
  const exploding = (): Ev => {
    const e: Ev = { task_id: 'boom' };
    Object.defineProperty(e, 'event', {
      enumerable: true,
      get() {
        throw new Error('a torn event');
      },
    });
    return e;
  };

  it('drops the half-applied carry, and the next tick is identical to a from-scratch fold', () => {
    const good = reparse(EVENTS.slice(0, 10));
    foldEventsIncremental(good, ACTIVITY, src('run-a', 7));

    // The tick that dies part way through: the events before the bad one HAVE been absorbed.
    const torn = [...reparse(EVENTS.slice(0, 14)), exploding()];
    expect(() => foldEventsIncremental(torn, ACTIVITY, src('run-a', 7))).toThrow('a torn event');

    // Same run, same generation, longer array — the array a poisoned carry would mis-fold.
    const next = reparse(EVENTS);
    expect(foldEventsIncremental(next, ACTIVITY, src('run-a', 7))).toStrictEqual(
      foldEvents(EVENTS, ACTIVITY)
    );
    // It could only be right by refolding from scratch, and it says so.
    expect(foldStats().fullFolds).toBe(2);
  });

  it('recovers the same way when the very first fold of a run throws', () => {
    const torn = [...reparse(EVENTS.slice(0, 6)), exploding()];
    expect(() => foldEventsIncremental(torn, ACTIVITY, src('run-c', 3))).toThrow('a torn event');
    expect(foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-c', 3))).toStrictEqual(
      foldEvents(EVENTS, ACTIVITY)
    );
  });
});

describe('foldEvents itself is unchanged — pure, uncached, same answer every time', () => {
  it('does not read or write the incremental cache', () => {
    foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    const stats = foldStats();
    const a = foldEvents(EVENTS, ACTIVITY);
    const b = foldEvents(EVENTS, ACTIVITY);
    expect(a).toStrictEqual(b);
    expect(foldStats()).toStrictEqual(stats);
    // And the cached run is still the cached run afterwards.
    const eventsFolded = foldStats().eventsFolded;
    foldEventsIncremental(reparse(EVENTS), ACTIVITY, src('run-a', 7));
    expect(foldStats().eventsFolded).toBe(eventsFolded);
  });

  it('folds an empty log, with and without digests', () => {
    for (const act of [{}, ACTIVITY]) {
      resetFoldCache();
      expect(foldEventsIncremental([], act, src('run-a', 7))).toStrictEqual(foldEvents([], act));
    }
  });
});

/**
 * The two halves, joined: the reader in main that ISSUES the generation and the fold in the renderer that
 * KEYS on it. Each half is right on its own above; this is the only place that proves they agree about
 * what a generation means, against a real file on disk being appended to and then replaced.
 */
describe('main issues the key, the renderer folds on it', () => {
  let dir: string;
  beforeEach(async () => {
    resetSwarmReadCache();
    dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'swarm-fold-'));
  });
  afterEach(async () => {
    await fsp.rm(dir, { recursive: true, force: true });
  });

  const tick = async (p: string, runId: string) => {
    const events = await readEvents(p);
    // IPC structured-clones, so the renderer never gets main's array — only its bytes.
    return foldEventsIncremental(reparse(events), ACTIVITY, {
      runId,
      generation: eventsGeneration(p),
    });
  };

  it('folds an appended log incrementally and lands on the from-scratch answer', async () => {
    const p = path.join(dir, 'run-a.jsonl');
    await fsp.writeFile(p, '');
    for (let n = 1; n <= EVENTS.length; n++) {
      await fsp.appendFile(p, JSON.stringify(EVENTS[n - 1]) + '\n');
      expect(await tick(p, 'run-a')).toStrictEqual(foldEvents(EVENTS.slice(0, n), ACTIVITY));
    }
    expect(foldStats().fullFolds).toBe(1);
    expect(foldStats().eventsFolded).toBe(EVENTS.length);
  });

  it('refolds from scratch when the log is REPLACED under the same run id', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, EVENTS.map((e) => JSON.stringify(e)).join('\n') + '\n');
    expect(await tick(p, 'bench')).toStrictEqual(foldEvents(EVENTS, ACTIVITY));

    // The bench harness reuses `.swarm/run.jsonl` run over run: same path, same runId, different run.
    const second = EVENTS.slice(0, 12);
    await fsp.rm(p);
    await fsp.writeFile(p, second.map((e) => JSON.stringify(e)).join('\n') + '\n');
    expect(await tick(p, 'bench')).toStrictEqual(foldEvents(second, ACTIVITY));
    expect(foldStats().fullFolds).toBe(2);
    expect(foldStats().incrementalFolds).toBe(0);
  });

  it('does not re-fold a single event when the log did not move', async () => {
    const p = path.join(dir, 'quiet.jsonl');
    await fsp.writeFile(p, EVENTS.map((e) => JSON.stringify(e)).join('\n') + '\n');
    await tick(p, 'run-a');
    const folded = foldStats().eventsFolded;
    const again = await tick(p, 'run-a');
    expect(foldStats().eventsFolded).toBe(folded);
    expect(again).toStrictEqual(foldEvents(EVENTS, ACTIVITY));
  });
});

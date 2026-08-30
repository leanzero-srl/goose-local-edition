import { beforeEach, describe, expect, it } from 'vitest';
import {
  buildActivity,
  buildPhaseTodo,
  foldEvents,
  foldRunPhase,
  resetFoldCache,
} from './useSwarmRun';

/**
 * RESEARCH FAN v2 — the live engine's research: the opener's OWN questions, one read-only structured
 * call each, fanned across the fleet between ASK and SYNTHESIS. Every payload shape below is verbatim
 * from the emit sites in swarm.rs (research_fan / fold_research_outcome, read 2026-08-30). The fan
 * emits NO `phase` event (its banner is stderr-only), so phase visibility is derived from these
 * events — that derivation is pinned here too.
 */

const ts = '2026-08-30T10:00:00Z';
const START = { event: 'run_started', ts, pool: [{ id: 'mac-mihai-x', model_id: 'mihai-qwen' }] };

const DISPATCHED = {
  event: 'research_dispatched',
  slice: 'store-layer',
  q_index: 2,
  question: 'Which storage format does the request mandate for the payments ledger?',
  model: 'mihai-qwen',
  activity_key: 'research-store-layer-q2',
};
const ANSWERED = {
  event: 'research_answered',
  slice: 'store-layer',
  q_index: 2,
  chars: 1843,
  raised: 1,
  secs: 74,
  model: 'mihai-qwen',
};
const UNANSWERED = {
  event: 'research_unanswered',
  slice: 'boot',
  q_index: 0,
  reason: 'judge_ended',
  detail: 'judge_out_of_moves after 4 looks',
  secs: 210,
  model: 'mihai-qwen',
};

describe('the fan reaches the feed — one line per event, from its own fields', () => {
  it('a dispatch names the slice, the question index and the node in BOTH feeds', () => {
    const { activity, verbose } = buildActivity([START, DISPATCHED]);
    const row = activity.find((r) => r.text === 'Researching store-layer q2');
    expect(row).toBeDefined();
    expect(row?.sub).toContain('mihai');
    // Verbose additionally carries the question itself — the thing being researched.
    const v = verbose.find((r) => r.text === 'Researching store-layer q2');
    expect(v?.sub).toContain('Which storage format');
  });

  it('an answer is a verbose done-line with chars, raised follow-ups and the node', () => {
    const { verbose } = buildActivity([START, DISPATCHED, ANSWERED]);
    const row = verbose.find((r) => r.text.startsWith('Research answered — store-layer q2'));
    expect(row?.text).toContain('1,843 chars');
    expect(row?.text).toContain('1 follow-up raised');
    expect(row?.tone).toBe('good');
    expect(row?.sub).toContain('74s');
  });

  it('an unanswered question is LOUD in both feeds, reason verbatim, detail as sub', () => {
    const { activity, verbose } = buildActivity([START, UNANSWERED]);
    for (const feed of [activity, verbose]) {
      const row = feed.find((r) => r.text === 'Research unanswered — boot q0 (judge ended)');
      expect(row?.tone).toBe('warn');
      expect(row?.sub).toContain('judge_out_of_moves');
    }
  });

  it('zero questions is a measured absence line, never silence', () => {
    const { activity } = buildActivity([START, { event: 'research_no_questions', slices: 3 }]);
    expect(activity.map((r) => r.text)).toContain(
      'No research questions — the opener raised none across 3 slices'
    );
  });

  it('a fan-wide panic is a bad line carrying the error', () => {
    const { activity } = buildActivity([
      START,
      { event: 'research_fan_panicked', error: 'task panicked at join' },
    ]);
    const row = activity.find(
      (r) => r.text === 'The research fan crashed — planning proceeds with zero answers'
    );
    expect(row?.tone).toBe('bad');
    expect(row?.sub).toBe('task panicked at join');
  });
});

describe('phase visibility — derived from the fan events, since no phase event fires', () => {
  it('research_dispatched enters Research; the engine phase:synthesis moves it forward', () => {
    const askFirst = [START, { event: 'phase', phase: 'ask' }];
    expect(foldRunPhase([...askFirst, DISPATCHED]).phase).toBe('research');
    expect(foldRunPhase([...askFirst, DISPATCHED]).observed.research).toBe(true);
    expect(
      foldRunPhase([...askFirst, DISPATCHED, ANSWERED, { event: 'phase', phase: 'synthesis' }])
        .phase
    ).toBe('synthesize');
  });

  it('answered/unanswered also count as research evidence (a resumed log mid-fan)', () => {
    expect(foldRunPhase([START, ANSWERED]).phase).toBe('research');
    expect(foldRunPhase([START, UNANSWERED]).phase).toBe('research');
  });

  it('research_no_questions does NOT enter Research — nothing ran, the chip reads skipped', () => {
    const folded = foldRunPhase([
      START,
      { event: 'phase', phase: 'ask' },
      { event: 'research_no_questions', slices: 2 },
    ]);
    expect(folded.phase).toBe('ask');
    expect(folded.observed.research).toBeUndefined();
  });
});

describe('the research lanes reach the planning board through the digest poll', () => {
  beforeEach(() => resetFoldCache());

  it('a research-<slice>-q<n> digest becomes a labeled planning lane', () => {
    const folded = foldEvents(
      [START] as never,
      {
        'research-store-layer-q2': {
          model: 'mihai-qwen',
          full_transcript: 'The request mandates tab-separated storage.',
          thinking_chars: 120,
        },
      } as never,
      'research-lane-test'
    );
    const lane = folded.planningLanes.find((l) => l.taskId === 'research-store-layer-q2');
    expect(lane, 'the research lane must reach planningLanes').toBeTruthy();
    // Identity before the ' · ' cut (laneSiblingTitle), caption after — the synthesis-label idiom.
    expect(lane?.description).toBe('Research store-layer q2 · one opener question, answered read-only');
    expect(lane?.status).toBe('running');
  });

  it('the lane closes when its own digest stamps done', () => {
    const folded = foldEvents(
      [START] as never,
      {
        'research-boot-q0': {
          model: 'mihai-qwen',
          full_transcript: 'answered',
          last_text: 'answered',
          phase: 'done',
        },
      } as never,
      'research-lane-done-test'
    );
    expect(folded.planningLanes.find((l) => l.taskId === 'research-boot-q0')?.status).toBe('done');
  });
});

describe('the Research checklist rows count what the fan measured', () => {
  const research = (events: Array<Record<string, unknown>>) =>
    buildPhaseTodo(events, {}, { clarifyPending: false }).find((p) => p.key === 'research')!;

  it('mid-fan: a running row counting settled questions', () => {
    const p = research([START, DISPATCHED, { ...DISPATCHED, slice: 'boot', q_index: 0 }, ANSWERED]);
    expect(p.items.map((i) => i.label)).toContain('Researching — 1 of 2 questions settled');
    expect(p.state).toBe('running');
  });

  it('settled: answered-of-dispatched, with the misses on their own row, reasons tallied', () => {
    const p = research([
      START,
      DISPATCHED,
      { ...DISPATCHED, slice: 'boot', q_index: 0 },
      ANSWERED,
      UNANSWERED,
    ]);
    const labels = p.items.map((i) => i.label);
    expect(labels).toContain('Research — 1 of 2 questions answered');
    expect(labels).toContain('1 question unanswered — kept as raw questions in the briefs');
    expect(p.items.find((i) => i.label.includes('unanswered'))?.detail).toBe('judge ended ×1');
  });

  it('zero questions: a skipped row, so the chip reads skipped rather than vanishing', () => {
    const p = research([START, { event: 'research_no_questions', slices: 2 }]);
    expect(p.items.map((i) => i.label)).toContain('No research questions — the opener raised none');
    expect(p.state).toBe('skipped');
  });

  it('a fan panic is an unverified row naming what was dispatched before the crash', () => {
    const p = research([
      START,
      DISPATCHED,
      { event: 'research_fan_panicked', error: 'join panicked' },
    ]);
    const row = p.items.find((i) => i.label.startsWith('Research fan crashed'));
    expect(row?.state).toBe('unverified');
    expect(row?.detail).toBe('1 question dispatched before the crash');
  });

  it('an archived pre-v2 run without fan events builds no v2 rows (legacy branches untouched)', () => {
    const p = research([
      START,
      { event: 'research_completed', slices: 2, brief_chars: [4200, 3100], secs: 260 },
    ]);
    expect(p.items.some((i) => i.id.startsWith('r2-'))).toBe(false);
    expect(p.items.some((i) => i.id === 'r-specs')).toBe(true);
  });
});

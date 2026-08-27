import { describe, expect, it } from 'vitest';
import { buildPhaseTodo, foldEvents, foldRunPhase } from './useSwarmRun';

/**
 * The rewritten engine's planning flow, folded from its ACTUAL event stream:
 *   OPEN -> ASK -> RESEARCH -> SYNTHESIS -> REVIEW -> BUILD -> INTEGRATE -> REPAIR
 * Every shape below is the one swarm.rs emits.
 */
const OPEN = { event: 'phase', phase: 'open' };
const SLICES = {
  event: 'slices_opened',
  count: 3,
  weights: [3, 2, 2],
  slices: ['core', 'cli', 'store'],
  secs: 41,
};
const ASK = { event: 'phase', phase: 'ask' };
const RESEARCH = { event: 'phase', phase: 'research' };
const RESEARCH_DONE = {
  event: 'research_completed',
  slices: 3,
  brief_chars: [4200, 3100, 2800],
  secs: 260,
};
const SYNTHESIS = { event: 'phase', phase: 'synthesis' };
const REVIEW = { event: 'phase', phase: 'review' };

describe('foldRunPhase — the ribbon reads the engine, never a label', () => {
  it('maps each phase event onto its own step, with ask folded into Open', () => {
    expect(foldRunPhase([OPEN]).phase).toBe('open');
    expect(foldRunPhase([OPEN, SLICES, ASK]).phase).toBe('open');
    expect(foldRunPhase([OPEN, ASK, RESEARCH]).phase).toBe('research');
    expect(foldRunPhase([OPEN, RESEARCH, SYNTHESIS]).phase).toBe('synthesize');
    expect(foldRunPhase([OPEN, RESEARCH, SYNTHESIS, REVIEW]).phase).toBe('review');
  });

  it('takes Build, Integrate, Repair and Done from the task lifecycle', () => {
    const planned = [OPEN, RESEARCH, SYNTHESIS, REVIEW, { event: 'plan_loaded', tasks: [] }];
    expect(foldRunPhase(planned).phase).toBe('build');
    expect(foldRunPhase([...planned, { event: 'task_dispatched', task_id: 'core' }]).phase).toBe(
      'build'
    );
    expect(
      foldRunPhase([...planned, { event: 'task_dispatched', task_id: 'integrate-verify' }]).phase
    ).toBe('integrate');
    expect(foldRunPhase([...planned, { event: 'complete_verify', findings: 2 }]).phase).toBe(
      'repair'
    );
    // A verify that found nothing is still integration, not repair.
    expect(foldRunPhase([...planned, { event: 'complete_verify', findings: 0 }]).phase).toBe(
      'integrate'
    );
    expect(
      foldRunPhase([...planned, { event: 'defects_rated', round: 1, critical: 0, minor: 2 }]).phase
    ).toBe('repair');
    expect(foldRunPhase([...planned, { event: 'run_finished', report: {} }]).phase).toBe('done');
  });

  // THE BUG THIS PINS: the old ribbon regex-matched the human label and DEFAULTED to Build, so a run with
  // no recognised phase — a Paused one, or one that had only just started — rendered "Build active" over
  // an idle fleet. There is no default any more.
  it('reports no phase at all before the engine has emitted one', () => {
    expect(foldRunPhase([]).phase).toBeNull();
    expect(foldRunPhase([{ event: 'run_started', pool: [] }]).phase).toBeNull();
  });

  it('records which phases ran, so a stage that never happened is never back-filled', () => {
    const { observed } = foldRunPhase([
      OPEN,
      RESEARCH,
      SYNTHESIS,
      REVIEW,
      { event: 'plan_loaded', tasks: [] },
      { event: 'run_finished', report: {} },
    ]);
    expect(observed.open).toBe(true);
    expect(observed.review).toBe(true);
    expect(observed.build).toBe(true);
    // No sink was dispatched and nothing was repaired — neither may show a completed check.
    expect(observed.integrate).toBeUndefined();
    expect(observed.repair).toBeUndefined();
  });
});

describe('buildPhaseTodo — the new phases are populated from the new events', () => {
  const todo = (events: Array<Record<string, unknown>>, clarifyPending = false) =>
    buildPhaseTodo(events, {}, { clarifyPending });
  const labels = (events: Array<Record<string, unknown>>, key: string, pending = false) =>
    todo(events, pending)
      .find((p) => p.key === key)!
      .items.map((i) => `${i.label}${i.detail ? ` | ${i.detail}` : ''}`);

  it('lists the slice cut, its weights, and flags a lopsided one', () => {
    const rows = labels([OPEN, SLICES], 'open');
    expect(rows.some((r) => r.includes('Request cut into 3 slices'))).toBe(true);
    expect(rows.some((r) => r.includes('core (w3), cli (w2), store (w2)'))).toBe(true);
    const uneven = labels([OPEN, { ...SLICES, weights: [6, 2, 2] }], 'open');
    expect(uneven.some((r) => r.includes('uneven'))).toBe(true);
  });

  // A QUESTION IS ALWAYS ANSWERED. Which of you answered it is the fact this row exists to carry.
  it('says who is answering the open decisions, and who did', () => {
    const armedImmediate = [
      OPEN,
      SLICES,
      ASK,
      { event: 'clarify_proxy_armed', mode: 'immediate', wait_secs: 0, questions: 2 },
    ];
    expect(labels(armedImmediate, 'open', true).join('\n')).toContain(
      'unattended run — goose is answering these'
    );

    const armedWaiting = [
      OPEN,
      SLICES,
      ASK,
      { event: 'clarify_proxy_armed', mode: 'after_wait', wait_secs: 300, questions: 2 },
    ];
    expect(labels(armedWaiting, 'open', true).join('\n')).toContain('goose answers in 5 min');

    const answered = [
      ...armedWaiting,
      {
        event: 'clarify_proxy_answered',
        questions: ['a', 'b'],
        answers: ['x', 'y'],
        source: 'proxy',
      },
    ];
    expect(labels(answered, 'open').join('\n')).toContain('answered by goose — you did not reply');
  });

  // An OLD run has no phase event and no slices. Its ask lived under the new-engine branch, which made it
  // dead code for exactly the runs it exists to serve.
  it('still surfaces a legacy low_confidence_ask on a run that predates the rewrite', () => {
    const rows = labels(
      [
        { event: 'run_started', pool: [] },
        { event: 'low_confidence_ask', questions: [{ question: 'a' }, { question: 'b' }] },
      ],
      'open'
    );
    expect(rows.join('\n')).toContain('Asked you 2 questions');
  });

  it('reports the slice specs research produced, and names a slice that returned none', () => {
    const rows = labels([OPEN, SLICES, RESEARCH, RESEARCH_DONE], 'research');
    expect(rows.some((r) => r.includes('Slice specs written — 3 of 3'))).toBe(true);
    expect(rows.some((r) => r.includes('10,100 chars of spec'))).toBe(true);
    const withEmpty = labels(
      [OPEN, SLICES, RESEARCH, { ...RESEARCH_DONE, brief_chars: [4200, 0, 2800] }],
      'research'
    );
    expect(withEmpty.some((r) => r.includes('1 slice came back with no spec'))).toBe(true);
  });

  it('treats the synthesis fallback as a degraded plan, never a failure', () => {
    const phase = todo([
      OPEN,
      SLICES,
      RESEARCH,
      RESEARCH_DONE,
      SYNTHESIS,
      { event: 'synthesis_fallback', error: 'no json', tasks: 4 },
      { event: 'plan_loaded', tasks: [{ id: 'core', deps: [] }] },
    ]).find((p) => p.key === 'synthesis')!;
    const fallback = phase.items.find((i) => i.id === 's-fallback')!;
    expect(fallback.state).toBe('unverified');
    expect(fallback.state).not.toBe('failed');
    expect(phase.items.some((i) => i.label.includes('Plan wired'))).toBe(true);
  });

  // The engine settles the review loop on whether a round asked for a CHANGE, not on how much prose it
  // produced. The checklist has to say the same thing or it contradicts the engine's own stop rule.
  it('measures a review round by what its patch touched, not by its wordcount', () => {
    const rows = labels(
      [
        REVIEW,
        {
          event: 'review_findings',
          round: 1,
          new: 2,
          repeated: 0,
          findings: ['a', 'b'],
          patch_touches: 2,
        },
        { event: 'plan_patched', round: 1, replace: 1, add: 1, remove: 0 },
        {
          event: 'review_findings',
          round: 2,
          new: 3,
          repeated: 1,
          findings: ['plan is sound'],
          patch_touches: 0,
        },
      ],
      'review'
    );
    expect(rows[0]).toContain('Round 1 — patched 2 tasks');
    expect(rows[1]).toContain('Round 2 — settled, no change requested');
  });

  it('carries a rejected patch without ending the run', () => {
    const phase = todo([
      REVIEW,
      {
        event: 'review_findings',
        round: 1,
        new: 1,
        repeated: 0,
        findings: ['x'],
        patch_touches: 1,
      },
      { event: 'plan_patch_rejected', round: 1, diagnostic: 'unknown task id' },
    ]).find((p) => p.key === 'review')!;
    expect(phase.items[0].state).toBe('unverified');
    expect(phase.items[0].detail).toContain('patch rejected');
  });

  // A green run with minor defects is still GREEN. Rating them must never paint the phase red.
  it('ships green with known active bugs and red only on a remaining critical', () => {
    const green = todo([
      {
        event: 'defects_rated',
        round: 1,
        critical: 0,
        minor: 2,
        engine_forced: 0,
        minors: ['a', 'b'],
      },
    ]).find((p) => p.key === 'repair')!;
    expect(green.items[0].state).toBe('done');
    expect(green.items[0].label).toContain('2 known active bugs');

    const red = todo([
      { event: 'defects_rated', round: 1, critical: 1, minor: 1, engine_forced: 1, minors: ['b'] },
    ]).find((p) => p.key === 'repair')!;
    expect(red.items[0].state).toBe('failed');
    expect(red.items[0].detail).toContain('forced critical by the engine');
  });

  it('keeps the sink out of Build and in Integrate', () => {
    const phases = todo([
      {
        event: 'plan_loaded',
        tasks: [
          { id: 'core', deps: [] },
          { id: 'integrate-verify', deps: ['core'] },
        ],
      },
      { event: 'task_dispatched', task_id: 'integrate-verify', device: 'gabee-x' },
    ]);
    const build = phases.find((p) => p.key === 'build')!;
    const integrate = phases.find((p) => p.key === 'integrate')!;
    expect(build.items.map((i) => i.id)).not.toContain('b-integrate-verify');
    expect(integrate.items.map((i) => i.id)).toContain('b-integrate-verify');
  });
});

describe('foldEvents — the slice fan and the planning calls have lanes', () => {
  // THE BUG THIS PINS: the digest prefix table knew scout-/contract-/detail- and nothing else, so the whole
  // fleet writing slice specs rendered as an empty lane list and every node read "idle — no task".
  it('turns slice-* and the single-node planning digests into lanes', () => {
    const activity = {
      'slice-core': { model: 'gabee-qwen', last_text: 'core needs a token type', tool_calls: 2 },
      'slice-cli': { model: 'mihai-qwen', thinking_chars: 900 },
      open: { model: 'gabee-qwen', last_text: 'three balanced slices', phase: 'done' },
      synthesis: { model: 'workhorse-qwen', last_text: 'wiring the dag' },
      review: { model: 'mihai-qwen', last_text: 'the request asks for an export command' },
      'proxy-answer': { model: 'gabee-qwen', last_text: 'take sqlite' },
      rate: { model: 'workhorse-qwen', last_text: 'this one is minor' },
    };
    const folded = foldEvents([OPEN, SLICES, RESEARCH], activity);
    expect(folded.sliceLanes.map((l) => l.taskId).sort()).toEqual(['slice-cli', 'slice-core']);
    expect(folded.sliceLanes[0].description).toBe('Slice · cli');
    expect(folded.planningLanes.map((l) => l.taskId)).toEqual([
      'open',
      'proxy-answer',
      'synthesis',
      'review',
      'rate',
    ]);
    // A per-call phase='done' closes that lane the instant its call ends, so the node stops reading busy.
    expect(folded.planningLanes.find((l) => l.taskId === 'open')!.status).toBe('done');
    expect(folded.planningLanes.find((l) => l.taskId === 'synthesis')!.status).toBe('running');
  });
});

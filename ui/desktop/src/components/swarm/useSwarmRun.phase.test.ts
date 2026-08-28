import { describe, expect, it } from 'vitest';
import { buildPhaseTodo, foldEvents, foldRunPhase, isPlanningDigestKey } from './useSwarmRun';

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

  // MEASURED on swarm-3node-r0: `phase synthesis` 07:04:52 -> `phase review` 07:08:27, strictly sequential,
  // and `plan_loaded` lands only after REVIEW has patched the DAG. Gating the Synthesize row on plan_loaded
  // therefore rendered it as still-running for the whole of Review on EVERY run, which reads as two phases
  // executing at once. The row must close when its own phase ends.
  it('closes the synthesis row when review opens, not when the plan loads', () => {
    const phase = todo([OPEN, SLICES, RESEARCH, RESEARCH_DONE, SYNTHESIS, REVIEW]).find(
      (p) => p.key === 'synthesis'
    )!;
    expect(phase.items.some((i) => i.state === 'running')).toBe(false);
    expect(phase.items.some((i) => i.id === 's-wired' && i.state === 'done')).toBe(true);
  });

  it('still shows synthesis running while it is genuinely the live phase', () => {
    const phase = todo([OPEN, SLICES, RESEARCH, RESEARCH_DONE, SYNTHESIS]).find(
      (p) => p.key === 'synthesis'
    )!;
    expect(phase.items.some((i) => i.id === 's-run' && i.state === 'running')).toBe(true);
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

/**
 * THE ENGINE MAY TAKE ITS GREEN BACK, AND THE PANEL MUST LET IT.
 *
 * `complete_result_revised` shipped with NO consumer in this file, in the scorer, or anywhere else — grep
 * across crates/, ui/desktop/src and evals/ returned only the emit site. So the CLI printed
 * "NOT VERIFIED - dead code shipped" while this panel, reading the retracted `complete_result` alone,
 * promoted every built task to 'done' and headed the run "Finished - app verified". The demote is ON by
 * default (SwarmConfig::default().unwired_demotes_verified) and `review: true` is in the shipped user
 * config, so this was the DEFAULT rendering of a run the engine had already called unverified.
 *
 * These pin all three derived verdicts, because one consumer that forgets the revision re-opens the whole
 * hole: the v-e2e row, the Done headline, and the unverified -> done promotion of the build rows.
 */
describe('buildPhaseTodo — a revised complete_result retracts the green everywhere', () => {
  const BUILT = [
    { event: 'phase', phase: 'open' },
    { event: 'plan_loaded', tasks: [{ id: 'core', files: ['app/core.py'] }] },
    { event: 'task_dispatched', task_id: 'core', device: 'n1' },
    { event: 'task_completed', task_id: 'core', status: 'ok', device: 'n1' },
    { event: 'complete_verify', round: 0, ran: true, findings: 0 },
    { event: 'complete_result', passed: true, verified: true, remaining_findings: 0 },
  ];
  const REVISED = {
    event: 'complete_result_revised',
    verified: false,
    reason: 'unwired-module-unfixed',
    evidence: ['kanban/db.py'],
  };
  const FINISH = { event: 'run_finished', report: {} };
  const row = (events: Array<Record<string, unknown>>, id: string) =>
    buildPhaseTodo(events, {}, { clarifyPending: false })
      .flatMap((p) => p.items)
      .find((i) => i.id === id);

  it('without the revision the run is green — the baseline this fix must not break', () => {
    const events = [...BUILT, FINISH];
    expect(row(events, 'v-e2e')!.state).toBe('done');
    expect(row(events, 'd-outcome')!.label).toBe('Finished — app verified');
    expect(row(events, 'b-core')!.state).toBe('done');
  });

  it('with it, v-e2e drops to unverified and names the dead module', () => {
    const r = row([...BUILT, REVISED, FINISH], 'v-e2e')!;
    expect(r.state).toBe('unverified');
    expect(r.detail).toContain('kanban/db.py');
    expect(r.detail).toContain('imported by nothing');
  });

  it('with it, the run never reads "app verified"', () => {
    expect(row([...BUILT, REVISED, FINISH], 'd-outcome')!.label).toBe('Finished — unverified');
  });

  it('with it, built tasks are no longer promoted to done off a retracted verdict', () => {
    expect(row([...BUILT, REVISED, FINISH], 'b-core')!.state).toBe('unverified');
  });
});

describe('planning lanes include the calls that FAN', () => {
  /** Observed live: the panel read "PLANNING CALLS · 2 NODES" (open, open-resplit) while three
   *  open-coverage-* lanes were running — the heaviest work in OPEN. They showed in FLEET, which reads
   *  node state, so the two halves of the same screen disagreed. A fixed key list cannot describe a phase
   *  whose lane count is a property of the fleet. */
  it('recognises coverage and review fan lanes, not just the fixed keys', () => {
    expect(isPlanningDigestKey('open')).toBe(true);
    expect(isPlanningDigestKey('open-coverage-1')).toBe(true);
    expect(isPlanningDigestKey('open-coverage-3')).toBe(true);
    expect(isPlanningDigestKey('review-2')).toBe(true);
  });

  it('does not swallow lanes that belong to other phases', () => {
    expect(isPlanningDigestKey('slice-store-layer')).toBe(false);
    expect(isPlanningDigestKey('apptest-bad-input')).toBe(false);
    expect(isPlanningDigestKey('verify::api')).toBe(false);
  });
});

describe("the judge's own ETA reaches the panel", () => {
  /** The judge is asked for an `ETA=<n>m` on every look and answers — measured live, open-coverage-2
   *  estimated 5, 5, 3, 3, 2 as it converged. Nothing consumed it, so the only "time left" on screen was
   *  the panel's own extrapolation (elapsed / items_done x remaining). A model judgement was being
   *  computed over rather than shown. */
  it('keeps the LATEST estimate per task, because the judge revises as it reads', () => {
    const folded = foldEvents(
      [
        OPEN,
        { event: 'judge_look', task_id: 'open-coverage-2', eta_mins: 5, verdict: 'ok' },
        { event: 'judge_look', task_id: 'open-coverage-2', eta_mins: 2, verdict: 'ok' },
        { event: 'judge_look', task_id: 'open', eta_mins: 7, verdict: 'ok' },
      ],
      {
        'open-coverage-2': { model: 'gabee-qwen', last_text: 'mapping components' },
        open: { model: 'workhorse-qwen', last_text: 'cutting slices' },
      }
    );
    const byId = Object.fromEntries(folded.planningLanes.map((l) => [l.taskId, l.judgeEtaMins]));
    expect(byId['open-coverage-2']).toBe(2);
    expect(byId['open']).toBe(7);
  });

  it('ignores a look that carries no estimate rather than inventing one', () => {
    const folded = foldEvents(
      [OPEN, { event: 'judge_look', task_id: 'open', verdict: 'ok', eta_mins: null }],
      { open: { model: 'workhorse-qwen', last_text: 'cutting slices' } }
    );
    expect(folded.planningLanes.find((l) => l.taskId === 'open')?.judgeEtaMins).toBeUndefined();
  });
});

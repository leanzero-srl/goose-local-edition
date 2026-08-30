import { describe, expect, it } from 'vitest';
import { buildActivity, buildPhaseTodo, foldEvents } from './useSwarmRun';

/**
 * The event-coverage batch of the frontend truth review (findings 1-5, 7-10): every absence event the
 * engine emits under the fallback gate, the user-input negatives, the transport-failure twins, the
 * judge's re-stream, the REPAIR-tail decisions and the mid-run Q&A now render — pinned here against the
 * engine's real payload shapes (field names read from the emit sites in swarm.rs, 2026-08-30).
 */

const ts = '2026-08-30T10:00:00Z';
const START = { event: 'run_started', ts, pool: [{ id: 'mac-mihai-x', model_id: 'mihai-qwen' }] };

describe('finding 1 — the fallback-gate absence events render instead of vanishing', () => {
  it('renders each absence event as a compact line built from its own facts', () => {
    const { activity } = buildActivity([
      START,
      { event: 'ledger_empty_at_sink', task_id: 'integrate-verify', spec_surface_empty: true },
      { event: 'spec_surface_empty_at_sink', task_id: 'integrate-verify' },
      { event: 'transcript_write_failed', activity_key: 'ledgerd-core', error: 'ENOSPC' },
      { event: 'ledger_row_unreadable', task_id: 'web-ui', rows: ['row-3.json'] },
      { event: 'dependency_context_empty', task_id: 'web-ui' },
      { event: 'thin_brief', task_id: 'web-ui', chars: 41, missing: ['owned file', 'objective fact'] },
      { event: 'pillars_distill_failed', reason: 'distillation call failed: connect timeout' },
    ]);
    const texts = activity.map((r) => r.text);
    expect(texts).toContain(
      'No ledger rows at the sink — integrate-verify dispatched without build history'
    );
    expect(texts).toContain(
      'The spec advertises no surface — integrate-verify verifies against an empty surface'
    );
    expect(texts).toContain('Transcript stopped persisting for ledgerd-core');
    expect(texts.some((t) => t.includes('ledger row') && t.includes('web-ui'))).toBe(true);
    expect(texts).toContain(
      'No dependency context for web-ui — it builds without its upstream sources'
    );
    expect(texts.some((t) => t.startsWith('Thin brief for web-ui (41 chars)'))).toBe(true);
    expect(texts).toContain(
      'Pillar distillation failed — the run proceeds without distilled pillars'
    );
    const ledger = activity.find((r) => r.text.startsWith('No ledger rows'));
    expect(ledger?.tone).toBe('bad');
    expect(ledger?.sub).toBe('the spec surface is empty too');
    const thin = activity.find((r) => r.text.startsWith('Thin brief'));
    expect(thin?.sub).toBe('missing: owned file, objective fact');
  });

  it('pillars_write_failed is its own loud line AND the positive pillars line keeps rendering with the caveat', () => {
    const { activity, verbose } = buildActivity([
      START,
      { event: 'pillars_write_failed', error: 'EACCES: /run/pillars.json' },
      { event: 'pillars', count: 4 },
    ]);
    const failed = activity.find((r) => r.text === 'pillars.json write failed — pillar checks will not run');
    expect(failed?.tone).toBe('warn');
    expect(failed?.sub).toContain('EACCES');
    // The positive line is NOT suppressed — injection into workers still happens — but it carries the
    // honest caveat instead of a clean green claim over a failed write.
    const pillars = verbose.find((r) => r.text === 'Defining quality pillars');
    expect(pillars).toBeDefined();
    expect(pillars?.sub).toBe('pillars.json write failed — pillar checks will not run');
  });

  it('without a write failure the pillars line carries no caveat', () => {
    const { verbose } = buildActivity([START, { event: 'pillars', count: 4 }]);
    expect(verbose.find((r) => r.text === 'Defining quality pillars')?.sub).toBeUndefined();
  });
});

describe('finding 2 — a note skipped as stale reaches both feeds, like its delivered twin', () => {
  it('names the skipped files and carries the engine detail', () => {
    const { activity, verbose } = buildActivity([
      START,
      {
        event: 'user_notes_skipped_stale',
        task_id: 'web-ui',
        files: ['1756-note.txt'],
        since_ms: 1756500000000,
        detail: 'the filename prefix must be epoch MILLISECONDS',
      },
    ]);
    for (const feed of [activity, verbose]) {
      const row = feed.find((r) => r.text === 'Your note skipped as stale — never delivered');
      expect(row?.tone).toBe('warn');
      expect(row?.sub).toContain('1756-note.txt');
      expect(row?.sub).toContain('epoch MILLISECONDS');
    }
  });
});

describe('finding 3 — replanned.reason: only the exact declined constant stays hidden', () => {
  const empty = (reason?: string) => ({
    event: 'replanned',
    round: 1,
    added: [],
    stopped: true,
    ...(reason !== undefined ? { reason } : {}),
  });

  it('hides the routine declined round', () => {
    const { activity } = buildActivity([START, empty('planner declined (empty plan)')]);
    expect(activity.some((r) => r.text.startsWith('Re-plan round'))).toBe(false);
  });

  it('a planner CALL failure is a warn line in both feeds — a network fault is not a decision', () => {
    const { activity, verbose } = buildActivity([
      START,
      empty('planner call failed: connect timeout'),
    ]);
    for (const feed of [activity, verbose]) {
      const row = feed.find((r) => r.text.includes('planner call failed: connect timeout'));
      expect(row?.tone).toBe('warn');
    }
  });

  it('an unknown future arm fails loud rather than being absorbed', () => {
    const { activity } = buildActivity([START, empty('some brand new arm')]);
    expect(activity.some((r) => r.text.includes('some brand new arm'))).toBe(true);
  });

  it('a pre-GEN-6a log with no reason field keeps its empty rounds hidden', () => {
    const { activity } = buildActivity([START, empty(undefined)]);
    expect(activity.some((r) => r.text.startsWith('Re-plan round'))).toBe(false);
  });

  it('the success arm surfaces the splice-hygiene repairs riding reason', () => {
    const { verbose } = buildActivity([
      START,
      { event: 'replanned', round: 1, added: ['docs-pass'], stopped: false, reason: 'renamed web.py -> web/app.py' },
    ]);
    const row = verbose.find((r) => r.text === 'Re-planned — added 1 task');
    expect(row?.sub).toContain('docs-pass');
    expect(row?.sub).toContain('renamed web.py -> web/app.py');
  });
});

describe('finding 5 — transport-failure twins are distinguishable from clean passes', () => {
  it('review_failed lands in the feed AND in reviewRounds as a failed round', () => {
    const { activity, reviewRounds } = buildActivity([
      START,
      { event: 'review_failed', round: 1, error: 'connection reset by peer' },
    ]);
    expect(
      activity.find((r) => r.text.includes('Review round 1 FAILED'))?.tone
    ).toBe('warn');
    expect(reviewRounds).toHaveLength(1);
    expect(reviewRounds[0].failed).toBe('connection reset by peer');
    expect(reviewRounds[0].findings).toEqual([]);
  });

  it('review_failed also lands in buildPhaseTodo as a neutral (never green) review row', () => {
    const phases = buildPhaseTodo(
      [START, { event: 'phase', phase: 'review' }, { event: 'review_failed', round: 1, error: 'boom' }],
      {},
      { clarifyPending: false }
    );
    const review = phases.find((p) => p.key === 'review');
    const row = review?.items.find((i) => i.id.startsWith('rv-failed-'));
    expect(row?.state).toBe('unverified');
    expect(row?.detail).toBe('boom');
  });

  it('pre_review_failed renders bad, distinct from a clean pre_review', () => {
    const { verbose } = buildActivity([
      START,
      { event: 'pre_review_failed', task_id: 'web-ui', device: 'mac-mihai-x', error: 'dead node', secs: 4.2 },
    ]);
    const row = verbose.find((r) => r.text.includes('Pre-review web-ui FAILED'));
    expect(row?.tone).toBe('bad');
    expect(row?.sub).toBe('dead node');
  });

  it('open_fallback says the run degrades to one slice, with both errors', () => {
    const { activity, verbose } = buildActivity([
      START,
      { event: 'open_fallback', first_error: 'no json', second_error: 'still no json', detail: 'one slice' },
    ]);
    expect(activity.some((r) => r.text.includes('opener failed twice'))).toBe(true);
    const v = verbose.find((r) => r.text.includes('opener failed twice'));
    expect(v?.sub).toContain('no json');
    expect(v?.sub).toContain('still no json');
  });
});

describe('finding 7 — a resumed run says so', () => {
  it('renders the resume in both feeds and stamps meta.resumed for the header badge', () => {
    const { activity, verbose, meta } = buildActivity([
      START,
      {
        event: 'run_resumed',
        tasks: 7,
        previously_completed: 3,
        detail: 'reused the previous plan; research and planning skipped. Completed tasks are re-run.',
      },
    ]);
    const text = 'Resumed — reused the previous plan (7 tasks); planning skipped; 3 finished tasks re-run';
    expect(activity.some((r) => r.text === text)).toBe(true);
    expect(verbose.some((r) => r.text === text)).toBe(true);
    expect(meta?.resumed).toEqual({ tasks: 7, previouslyCompleted: 3 });
  });
});

describe('finding 8 — a wiped-and-restreamed lane has an on-screen cause', () => {
  const RESTREAM = {
    event: 'judge_restream',
    task_id: 'ledgerd-core',
    nudge: 3,
    reason: 'delivery defect: owed a structured reply',
    abandoned_thinking_chars: 41213,
    abandoned_tool_calls: 2,
    established_chars: 900,
  };

  it('renders a judge-act warn line from the event facts', () => {
    const { verbose } = buildActivity([START, RESTREAM]);
    const row = verbose.find((r) => r.text.includes('wiped and re-streamed ledgerd-core'));
    expect(row?.kind).toBe('judge-act');
    expect(row?.tone).toBe('warn');
    expect(row?.sub).toContain('delivery defect');
    expect(row?.sub).toContain('41,213');
  });

  it('stamps the lane with a restream count that survives a retry (event carry, not a digest field)', () => {
    const folded = foldEvents(
      [
        START,
        { event: 'task_dispatched', ts, task_id: 'ledgerd-core', device: 'mac-mihai-x' },
        RESTREAM,
        { event: 'task_retry', ts, task_id: 'ledgerd-core', error: 'stream died' },
        RESTREAM,
      ],
      {}
    );
    expect(folded.lanes.find((l) => l.taskId === 'ledgerd-core')?.restreams).toBe(2);
  });

  it('delivery_defect_steer renders the defect list and the look number', () => {
    const { verbose } = buildActivity([
      START,
      {
        event: 'delivery_defect_steer',
        task_id: 'ledgerd-core',
        look: 4,
        defects: ['app/cli.py never written'],
      },
    ]);
    const row = verbose.find((r) => r.text === 'Delivery-defect steer to ledgerd-core (look 4)');
    expect(row?.tone).toBe('warn');
    expect(row?.sub).toBe('app/cli.py never written');
  });
});

describe('finding 9 — the REPAIR-tail decisions are visible', () => {
  it('known_bugs UNIONS into knownActiveBugs and a later complete_result snapshot supersedes it', () => {
    const events = [
      START,
      { event: 'known_bugs', round: 1, count: 2, findings: ['a', 'b'] },
      { event: 'known_bugs', round: 2, count: 2, findings: ['b', 'c'] },
    ];
    expect(buildActivity(events).knownActiveBugs).toEqual(['a', 'b', 'c']);
    expect(
      buildActivity([...events, { event: 'complete_result', known_active_bugs: ['c', 'd'] }])
        .knownActiveBugs
    ).toEqual(['c', 'd']);
  });

  it('boot_repair_exhausted is a compact bad line carrying the boot error', () => {
    const { activity } = buildActivity([
      START,
      { event: 'boot_repair_exhausted', attempts: 3, detail: 'no progress between attempts', boot_error: 'ModuleNotFoundError: web' },
    ]);
    const row = activity.find((r) => r.text === 'Boot repair gave up after 3 attempts — the app does not start');
    expect(row?.tone).toBe('bad');
    expect(row?.sub).toBe('ModuleNotFoundError: web');
  });

  it('best_tree_restored distinguishes a rollback from a FAILED rollback', () => {
    const restored = buildActivity([
      START,
      { event: 'best_tree_restored', from_round: 2, best_findings: 1, best_established: true, final_findings: 4, final_ran: true, final_established: true, restored: true },
    ]).activity;
    expect(
      restored.some((r) => r.text === 'Rolled back to the round-2 tree — 1 established finding vs 4 in the final tree')
    ).toBe(true);
    const failed = buildActivity([
      START,
      { event: 'best_tree_restored', from_round: 2, best_findings: 1, best_established: true, final_findings: 4, final_ran: true, final_established: true, restored: false },
    ]).activity;
    expect(failed.find((r) => r.text.includes('restore FAILED'))?.tone).toBe('bad');
  });

  it('complete_fix_converged explains why the waves stopped', () => {
    const { activity } = buildActivity([
      START,
      { event: 'complete_fix_converged', round: 5, findings: 3, detail: 'the wave changed nothing on the tree' },
    ]);
    const row = activity.find((r) => r.text === 'Fix waves stopped — converged with 3 findings standing');
    expect(row?.sub).toContain('changed nothing');
  });

  it('boot_repaired is a good-tone verbose line', () => {
    const { verbose } = buildActivity([
      START,
      { event: 'boot_repaired', attempts: 2, detail: 'the app now binds and answers' },
    ]);
    expect(
      verbose.find((r) => r.text === 'Boot repaired after 2 attempts — the app binds and answers')?.tone
    ).toBe('good');
  });
});

describe('finding 10 — swarm_answer reaches the panel, not just a dotfile', () => {
  it('accumulates qa in event order and renders a feed line', () => {
    const { activity, qa } = buildActivity([
      START,
      { event: 'swarm_answer', question_file: 'q1.txt', question: 'which port?', answer: '8850', model: 'mihai-qwen' },
      { event: 'swarm_answer', question_file: 'q2.txt', question: 'sqlite or pg?', answer: 'sqlite', model: 'gabee-qwen' },
    ]);
    expect(qa).toEqual([
      { question: 'which port?', answer: '8850', model: 'mihai-qwen', questionFile: 'q1.txt' },
      { question: 'sqlite or pg?', answer: 'sqlite', model: 'gabee-qwen', questionFile: 'q2.txt' },
    ]);
    expect(activity.some((r) => r.text === 'Answered your question: which port?')).toBe(true);
  });
});

describe('finding 4 — a tree defect annotates the board without repainting engine state', () => {
  const EVENTS = [
    START,
    {
      event: 'plan_loaded',
      tasks: [
        { id: 'ledgerd-core', files: ['app/ledger.py'] },
        { id: 'web-ui', files: ['app/web.py'], deps: ['ledgerd-core'] },
      ],
    },
    { event: 'task_dispatched', task_id: 'ledgerd-core', device: 'mac-mihai-x' },
    { event: 'task_completed', task_id: 'ledgerd-core', status: 'completed', device: 'mac-mihai-x' },
    { event: 'task_dispatched', task_id: 'web-ui', device: 'mac-mihai-x' },
    {
      event: 'tree_defect',
      task_id: 'web-ui',
      dependency: 'ledgerd-core',
      detail: 'app/ledger.py holds only the engine stub',
    },
  ];

  it('the dependency keeps its unverified state and gains the warden detail; the dependent gets a note', () => {
    const phases = buildPhaseTodo(EVENTS, {}, { clarifyPending: false });
    const build = phases.find((p) => p.key === 'build');
    const dep = build?.items.find((i) => i.id === 'b-ledgerd-core');
    expect(dep?.state).toBe('unverified'); // never red — the engine keeps it Done
    expect(dep?.detail).toContain('tree warden: app/ledger.py holds only the engine stub');
    const dependent = build?.items.find((i) => i.id === 'b-web-ui');
    expect(dependent?.detail).toContain('building on flagged ledgerd-core');
  });

  it('the annotation persists through the e2e green promotion', () => {
    const promoted = buildPhaseTodo(
      [
        ...EVENTS,
        { event: 'task_completed', task_id: 'web-ui', status: 'completed', device: 'mac-mihai-x' },
        { event: 'complete_result', passed: true, verified: true },
      ],
      {},
      { clarifyPending: false }
    );
    const dep = promoted.find((p) => p.key === 'build')?.items.find((i) => i.id === 'b-ledgerd-core');
    expect(dep?.state).toBe('done'); // engine-truth promotion still happens — the stub may have been repaired
    expect(dep?.detail).toContain('tree warden:');
  });

  it('the feed carries the warden finding as a verbose line', () => {
    const { verbose } = buildActivity(EVENTS);
    const row = verbose.find((r) =>
      r.text.includes('ledgerd-core reported done but left a defect on disk')
    );
    expect(row?.sub).toBe('app/ledger.py holds only the engine stub');
  });
});

import { beforeEach, describe, expect, it } from 'vitest';
import {
  buildActivity,
  buildPhaseTodo,
  deriveTaskBoard,
  foldEvents,
  isPlanningDigestKey,
  resetFoldCache,
} from './useSwarmRun';

/**
 * THE SPLIT (2c S1) AND ITS MERGER ON THE PANEL, pinned against r6e's real events
 * (local-sb7-swarm-r6e-…-30b1c4fb2/run.jsonl, seq 464/522/523/533/548). The fixture is the run that
 * was KILLED for exactly what these rows must show: `viz3d-engine` measured fat (11 sections, one
 * file), split into 8 shards + a merger by ONE plan patch, then the merger dispatched with
 * `merge_dossier{pieces: 0, readmes_missing: <all 8>}` because a repair had stripped its shard deps.
 * Before this file the patch rendered as "Plan patched (round 0)" in the review feed, the dossier
 * rendered nowhere, and the merger's board row read like any other build task.
 */
const SHARDS = [
  'viz3d-engine-data-scene',
  'viz3d-engine-rendering-core',
  'viz3d-engine-pick-buffer',
  'viz3d-engine-camera-inertia',
  'viz3d-engine-labels-culling',
  'viz3d-engine-linked-brush',
  'viz3d-engine-streaming-diffs',
  'viz3d-engine-vs7dbg-boot',
];
const CLEAN = {
  tasks_sharing_a_file: 0,
  shared_files: [],
  tasks_owning_nothing: [],
  module_package_collisions: [],
  unassigned_endpoints: [],
};
const OPEN = { event: 'phase', phase: 'open' };
const SYNTHESIS = { event: 'phase', phase: 'synthesis' };
const REPAIRED_PLAN = {
  event: 'plan_repaired',
  source: 'plan',
  before: { tasks: 8, ...CLEAN, tasks_sharing_a_file: 1, shared_files: ['app/__main__.py'] },
  after: { tasks: 8, ...CLEAN },
  actions: ['shared file `app/__main__.py`: kept by `skeleton`'],
};
const FAT = {
  event: 'plan_flag',
  kind: 'fat_task',
  task: 'viz3d-engine',
  files: ['web/viz.js'],
  sections: 11,
  section_chars: 18508,
  brief_chars: 38202,
  sections_per_file: 11.0,
  chars_per_file: 18508.0,
  median: 2.875,
  mean: 4.0,
  stddev: 3.243583409338094,
  threshold: 7.24,
};
const PATCH = {
  event: 'plan_patched',
  source: 'split',
  module: 'viz3d-engine',
  shards: SHARDS,
  exports_declared: 23,
  replace: 1,
  add: 8,
  remove: 0,
  after: { tasks: 16, distinct_files: 29, ...CLEAN },
};
const REPAIRED_SPLIT = {
  event: 'plan_repaired',
  source: 'split',
  before: { tasks: 16, ...CLEAN },
  after: { tasks: 16, ...CLEAN },
  actions: [
    ...SHARDS.map((s) => `\`${s}\`: its brief names \`web/viz.js\`, owned by \`viz3d-engine\``),
    '`integrate-verify` did not depend on the shards: the join waits on every task — added',
    '`viz3d-engine` was gated on docs-only shards: dep dropped',
  ],
};
const PLAN_LOADED = {
  event: 'plan_loaded',
  task_count: 16,
  tasks: [
    { id: 'skeleton', files: ['app/__main__.py'], deps: [] },
    { id: 'viz3d-engine', files: ['web/viz.js'], deps: [] },
    ...SHARDS.map((id) => ({
      id,
      files: [`.swarm/shards/viz3d-engine/${id.slice('viz3d-engine-'.length)}/README.md`],
      deps: [],
    })),
    { id: 'integrate-verify', files: [], deps: ['skeleton', 'viz3d-engine', ...SHARDS] },
  ],
};
const BUILD = { event: 'phase', phase: 'build' };
const DISPATCH_MERGER = {
  event: 'task_dispatched',
  task_id: 'viz3d-engine',
  device: 'mac-gabee-qwen3.8-27b',
  model: 'gabee-qwen3.8-27b',
  attempt: 0,
  owned_files: ['web/viz.js'],
};
const DECLARED = [
  'vs7dbg',
  'vs7dbg.layout',
  'vs7dbg.sceneDigest',
  'vs7dbg.camera',
  'vs7dbg.setCamera',
  'vs7dbg.pick',
  'vs7dbg.pickPixel',
  'vs7dbg.brush',
  'vs7dbg.frames',
  'viz3d.toggle',
  'viz3d.clear',
  'viz3d.setBrush',
  'project',
  'renderFrame',
  'requestRender',
  'refreshPick',
  'pickAt',
  'pickPixelAt',
  'maybeClick',
  'applyBrushDim',
  'buildInstance',
  'computeSceneDigest',
  'updateLabels',
];
const DOSSIER_EMPTY = {
  event: 'merge_dossier',
  task_id: 'viz3d-engine',
  module: 'viz3d-engine',
  shards: SHARDS,
  pieces: 0,
  pieces_with_parse_errors: 0,
  readmes_missing: SHARDS,
  duplicates: [],
  declared_missing: DECLARED,
  signature_disagreements: [],
  assumptions_unmet: [],
  unfinished: [],
  second_pass: false,
  final_on_disk: [],
};
const R6E = [
  OPEN,
  SYNTHESIS,
  REPAIRED_PLAN,
  FAT,
  PATCH,
  REPAIRED_SPLIT,
  PLAN_LOADED,
  BUILD,
  DISPATCH_MERGER,
  DOSSIER_EMPTY,
];

// The merger's completion events, in the engine's shapes (shards.rs record_merge_result, scheduler.rs
// splice_merge_gaps) — r6e never reached them, so these are the shapes, not a measured stream.
const PIECE_DROPPED = {
  event: 'merge_piece_dropped',
  module: 'viz3d-engine',
  task_id: 'viz3d-engine',
  shard: 'viz3d-engine-pick-buffer',
  symbol: 'pickPixelAt',
  referenced: true,
};
const SIGNATURE = {
  event: 'merge_signature_mismatch',
  module: 'viz3d-engine',
  task_id: 'viz3d-engine',
  symbol: 'renderFrame',
  declared: 'renderFrame(dt)',
  found: 'renderFrame()',
};
const GAP = {
  event: 'merge_gap',
  module: 'viz3d-engine',
  shard: 'viz3d-engine-gap-1',
  task_id: 'viz3d-engine',
  missing: 'label culling for overlapping rows',
  folder: '.swarm/shards/viz3d-engine/gap-1',
};
const GAP_REPEATED = {
  event: 'merge_gap_repeated',
  task_id: 'viz3d-engine',
  gap: 'viz3d-engine-gap-2',
  missing: 'label culling for overlapping rows',
  landed_as: 'viz3d-engine-labels-culling',
};
const GAP_REFUSED = {
  event: 'merge_gap_refused',
  task_id: 'viz3d-engine',
  gaps: ['viz3d-engine-gap-3'],
  reason: 'splice refused: gap owns web/viz.js, already owned by viz3d-engine',
};
const GAP_OPEN = {
  event: 'merge_gap_open',
  module: 'viz3d-engine',
  task_id: 'viz3d-engine',
  shard: 'viz3d-engine-streaming-diffs',
  item: 'byte accounting on the SSE stream',
};
const CHECKED_NOT_PROMOTED = {
  event: 'merge_checked',
  module: 'viz3d-engine',
  task_id: 'viz3d-engine',
  files: ['web/viz.js'],
  parse_errors: [],
  parse: 'checked',
  unchecked: [],
  declared_present: DECLARED.slice(0, 20),
  declared_missing: DECLARED.slice(20),
  signature_mismatch: 1,
  dropped: 1,
  dropped_referenced: 1,
  gaps_open: 1,
  gaps_sent: [],
  merge_readme_present: true,
  promoted: false,
};
const CHECKED_PROMOTED = {
  ...CHECKED_NOT_PROMOTED,
  declared_present: DECLARED,
  declared_missing: [],
  signature_mismatch: 0,
  dropped: 0,
  dropped_referenced: 0,
  gaps_open: 0,
  promoted: true,
};
const DECLINED = {
  event: 'split_declined',
  task: 'viz3d-engine',
  reason: 'split request did not return: stream decode error',
};

const rows = (events: Array<Record<string, unknown>>) => {
  const { activity, verbose } = buildActivity(events);
  return { activity, verbose };
};

describe('the split is a PATCH on the feed — never a re-plan, never a review round', () => {
  it('names the module, the shard count, the exports and the plan size after', () => {
    const { activity, verbose } = rows(R6E);
    const row = activity.find((r) => r.text.startsWith('Plan patched'));
    expect(row?.text).toBe('Plan patched — fat task `viz3d-engine` split into 8 shards + a merger');
    expect(row?.kind).toBe('plan');
    expect(row?.tone).toBe('good');
    expect(row?.sub).toContain('23 exports declared');
    expect(row?.sub).toContain('1 task rewired · 8 added · 0 removed');
    expect(row?.sub).toContain('plan now 16 tasks');
    // The verbose twin lists the shards by name; the compact one does not carry eight ids.
    const v = verbose.find((r) => r.text.startsWith('Plan patched'));
    expect(v?.sub).toContain('viz3d-engine-vs7dbg-boot');
    expect(row?.sub).not.toContain('viz3d-engine-vs7dbg-boot');
    // Not a review round, not a re-plan — anywhere on either feed.
    for (const r of [...activity, ...verbose]) {
      expect(r.text).not.toMatch(/round/i);
      expect(r.text).not.toMatch(/re-plan/i);
    }
  });

  it('the fat-task measurement that summoned the split renders with its numbers', () => {
    const { activity } = rows(R6E);
    const row = activity.find((r) => r.text.startsWith('Fat task'));
    expect(row?.text).toBe('Fat task `viz3d-engine` — asking synthesis for a split');
    expect(row?.sub).toBe('11 spec sections for 1 file · 11.0/file · threshold 7.2 · median 2.9');
  });

  it('the split plan walking the door again is labelled as the split plan, the first repair unchanged', () => {
    const { activity } = rows(R6E);
    const repairs = activity.filter((r) => /repaired|needed no repair/.test(r.text));
    expect(repairs.map((r) => r.text)).toEqual([
      'Plan repaired — 1 deterministic fix',
      'Split plan repaired — 10 deterministic fixes',
    ]);
  });

  it('a declined split is a warning that the fat task builds as one lane', () => {
    const { activity } = rows([OPEN, SYNTHESIS, FAT, DECLINED, PLAN_LOADED]);
    const row = activity.find((r) => r.text.startsWith('Split of'));
    expect(row?.text).toBe('Split of `viz3d-engine` declined — the fat task builds as one lane');
    expect(row?.tone).toBe('warn');
    expect(row?.sub).toBe('split request did not return: stream decode error');
  });

  it('the legacy review-round patch (no source) keeps its archived rendering', () => {
    const { verbose, activity } = rows([
      OPEN,
      { event: 'phase', phase: 'review' },
      {
        event: 'review_findings',
        round: 1,
        findings: ['x'],
        new: 1,
        repeated: 0,
        patch_touches: 1,
      },
      { event: 'plan_patched', round: 1, replace: 1, add: 0, remove: 0 },
    ]);
    expect(verbose.some((r) => r.text === 'Plan patched (round 1)' && r.kind === 'review')).toBe(
      true
    );
    expect(activity.some((r) => r.text.startsWith('Plan patched'))).toBe(false);
  });
});

describe('merge_dossier renders as the truth of what the merger was handed', () => {
  it('r6e: zero pieces and every README missing is a BAD row, never a clean pass', () => {
    const { activity, verbose } = rows(R6E);
    const row = activity.find((r) => r.text.startsWith('Merger'));
    expect(row?.text).toBe(
      'Merger `viz3d-engine` handed 0 pieces from 8 shards — 8 READMEs missing'
    );
    expect(row?.tone).toBe('bad');
    expect(row?.sub).toContain('declared exports undefined 23');
    expect(row?.sub).toContain('parse errors 0');
    const v = verbose.find((r) => r.text.startsWith('Merger'));
    expect(v?.sub).toContain('no README: viz3d-engine-data-scene');
  });

  it('a full hand-off is good; a partial one is a warning', () => {
    const full = { ...DOSSIER_EMPTY, pieces: 8, readmes_missing: [], declared_missing: [] };
    expect(rows([full]).activity[0]).toMatchObject({
      text: 'Merger `viz3d-engine` handed 8 pieces from 8 shards',
      tone: 'good',
    });
    const partial = {
      ...DOSSIER_EMPTY,
      pieces: 6,
      readmes_missing: SHARDS.slice(0, 2),
      declared_missing: ['updateLabels'],
      second_pass: true,
    };
    expect(rows([partial]).activity[0]).toMatchObject({
      text: 'Merger `viz3d-engine` handed 6 pieces from 8 shards — 2 READMEs missing (second pass)',
      tone: 'warn',
    });
  });
});

describe('the merger completion events — each failure twin renders, none looks like a clean pass', () => {
  it('a dropped piece the merged file still calls is bad; a signature mismatch says both sides', () => {
    const { activity } = rows([PIECE_DROPPED, SIGNATURE]);
    expect(activity[0]).toMatchObject({
      text: 'Merge dropped `pickPixelAt` from viz3d-engine-pick-buffer — the merged file still calls it',
      tone: 'bad',
    });
    expect(activity[1]).toMatchObject({
      text: 'Merge signature mismatch — `renderFrame` in viz3d-engine',
      tone: 'warn',
      sub: 'declared renderFrame(dt) · found renderFrame()',
    });
  });

  it('gap dispatched / repeated / refused / left open are four distinct rows', () => {
    const { activity } = rows([GAP, GAP_REPEATED, GAP_REFUSED, GAP_OPEN]);
    expect(activity.map((r) => [r.text, r.tone])).toEqual([
      ['Merge gap in viz3d-engine — shard `viz3d-engine-gap-1` dispatched', 'warn'],
      [
        'Merge gap repeated — `label culling for overlapping rows` already landed as viz3d-engine-labels-culling; refused',
        'warn',
      ],
      ['Merge gap refused for viz3d-engine — viz3d-engine-gap-3', 'bad'],
      [
        'Merge left `byte accounting on the SSE stream` open — shard viz3d-engine-streaming-diffs, neither filled nor sent out',
        'warn',
      ],
    ]);
    expect(activity[0].sub).toBe(
      'label culling for overlapping rows · .swarm/shards/viz3d-engine/gap-1'
    );
    expect(activity[2].sub).toContain('splice refused');
  });

  it('merge_checked: promoted is good; a referenced drop makes it bad and says why', () => {
    const bad = rows([CHECKED_NOT_PROMOTED]).activity[0];
    expect(bad.text).toBe('Merge of viz3d-engine checked — not promoted');
    expect(bad.tone).toBe('bad');
    expect(bad.sub).toContain('declared exports undefined 3');
    expect(bad.sub).toContain('pieces dropped 1 (1 still referenced)');
    expect(bad.sub).toContain('gaps open 1 · sent 0');
    const good = rows([CHECKED_PROMOTED]).activity[0];
    expect(good.text).toBe(
      'Merge of viz3d-engine checked — every declared export defined, promoted'
    );
    expect(good.tone).toBe('good');
  });
});

describe('the checklist and the board carry the split as engine truth', () => {
  const todo = (events: Array<Record<string, unknown>>) =>
    buildPhaseTodo(events, {}, { clarifyPending: false });
  const items = (events: Array<Record<string, unknown>>, key: string) =>
    todo(events).find((p) => p.key === key)?.items ?? [];

  // VA-138: the split is its OWN step after Synthesize — its rows moved out of the synthesis list.
  it('split: the split row is done with its facts; the flag row is superseded by it', () => {
    const split = items(R6E, 'split');
    const row = split.find((i) => i.id === 's-split-viz3d-engine');
    expect(row?.label).toBe('Fat task viz3d-engine split into 8 shards + a merger');
    expect(row?.state).toBe('done');
    expect(row?.detail).toBe('23 exports declared · plan now 16 tasks');
    expect(split.some((i) => i.id === 's-fat-viz3d-engine')).toBe(false);
    const synthesis = items(R6E, 'synthesis');
    expect(synthesis.some((i) => i.id.startsWith('s-split') || i.id.startsWith('s-fat'))).toBe(false);
    // The fixture's plan_loaded is trimmed to 11 of r6e's 16 tasks; the label counts what it lists.
    expect(synthesis.find((i) => i.id === 's-done')?.label).toBe('Plan wired — 11 tasks');
  });

  it('a flagged task is RUNNING while the plan is open, and an ADVISORY once the plan loaded without a split', () => {
    const live = items([OPEN, SYNTHESIS, FAT], 'split').find(
      (i) => i.id === 's-fat-viz3d-engine'
    );
    expect(live?.state).toBe('running');
    expect(live?.label).toBe('Fat task viz3d-engine — asking synthesis for a split');
    expect(live?.detail).toBe('11 spec sections for 1 file · 11.0/file vs threshold 7.2');
    const gone = items([OPEN, SYNTHESIS, FAT, PLAN_LOADED], 'split').find(
      (i) => i.id === 's-fat-viz3d-engine'
    );
    expect(gone?.state).toBe('advisory');
    expect(gone?.label).toBe('Fat task viz3d-engine flagged — no split event followed');
    const declined = items([OPEN, SYNTHESIS, FAT, DECLINED, PLAN_LOADED], 'split');
    expect(declined.some((i) => i.id === 's-fat-viz3d-engine')).toBe(false);
    expect(declined.find((i) => i.id === 's-split-declined-viz3d-engine')).toMatchObject({
      state: 'advisory',
      detail: 'split request did not return: stream decode error',
    });
  });

  it('build: eight shard rows say whose piece they are; the merger row carries its dossier', () => {
    const build = items(R6E, 'build');
    const shardRows = build.filter((i) => SHARDS.includes(i.id.slice(2)));
    expect(shardRows).toHaveLength(8);
    for (const r of shardRows) expect(r.detail).toBe('shard of viz3d-engine');
    const merger = build.find((i) => i.id === 'b-viz3d-engine');
    expect(merger?.state).toBe('running');
    expect(merger?.detail).toBe(
      'merger: 0/8 pieces handed, 8 READMEs missing, 23 declared exports undefined'
    );
    // No re-plan bookkeeping row anywhere — the split never was one.
    expect(
      todo(R6E)
        .flatMap((p) => p.items)
        .some((i) => /^b-replan-/.test(i.id))
    ).toBe(false);
  });

  it('the merger row appends the merge check verdict at completion', () => {
    const done = { event: 'task_completed', task_id: 'viz3d-engine', status: 'ok' };
    const merger = items([...R6E, done, CHECKED_NOT_PROMOTED], 'build').find(
      (i) => i.id === 'b-viz3d-engine'
    );
    expect(merger?.detail).toContain(
      'merge checked — not promoted (3 exports undefined, 1 referenced pieces dropped, 1 gaps open)'
    );
  });

  it('the board counts the shards as work and reports nothing re-planned', () => {
    const { plan } = buildActivity(R6E);
    const folded = foldEvents(R6E, {});
    const board = deriveTaskBoard({
      plan,
      phaseTodo: todo(R6E),
      lanes: folded.lanes,
      fixLanes: folded.fixLanes,
    });
    expect(board.addedByReplan).toBe(0);
    expect(board.running.map((r) => r.id)).toEqual(['viz3d-engine']);
    expect(board.queued.filter((r) => SHARDS.includes(r.id))).toHaveLength(8);
  });
});

describe("the split's own model call is a planning lane beside synthesis", () => {
  beforeEach(() => resetFoldCache());
  // r6e's split-viz3d-engine.json: one final_output call after 72k chars of thinking on workhorse.
  const SPLIT_DIGEST = {
    tool_calls: 1,
    errors: 0,
    malformed: 0,
    recent: ['recipe__final_output ok'],
    inflight: [],
    last_text: 'buffer fill → first render → open stream), the visible 3D-unavailable notice…',
    calls: [{ name: 'recipe__final_output', summary: '{"interface":{"exports":[…]}}', ok: true }],
    thinking_chars: 72163,
    last_thinking: 'Layout: 11 lines as above.',
    model: 'workhorse-qwen3.8-27b',
    attempt: 0,
    phase: 'done',
  };

  it('split-<task> is a planning digest key, labelled with its identity before the caption', () => {
    expect(isPlanningDigestKey('split-viz3d-engine')).toBe(true);
    const folded = foldEvents([OPEN, SYNTHESIS, FAT], { 'split-viz3d-engine': SPLIT_DIGEST });
    const lane = folded.planningLanes.find((l) => l.taskId === 'split-viz3d-engine');
    expect(lane?.description).toBe(
      'Split viz3d-engine · declaring the shard interface of a fat task'
    );
    expect(lane?.status).toBe('done');
    expect(lane?.thinkingChars).toBe(72163);
    expect(lane?.device).toBe('workhorse');
  });

  it('a worker task the plan named split-* stays a task lane and is not painted twice', () => {
    const events = [
      OPEN,
      SYNTHESIS,
      { event: 'plan_loaded', tasks: [{ id: 'split-view', files: ['web/split.js'], deps: [] }] },
      { event: 'task_dispatched', task_id: 'split-view', device: 'mac-gabee-qwen3.8-27b' },
    ];
    const folded = foldEvents(events, { 'split-view': { ...SPLIT_DIGEST, phase: undefined } });
    expect(folded.lanes.some((l) => l.taskId === 'split-view')).toBe(true);
    expect(folded.planningLanes.some((l) => l.taskId === 'split-view')).toBe(false);
  });
});

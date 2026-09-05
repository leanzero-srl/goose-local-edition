import { describe, expect, it } from 'vitest';
import { buildActivity, buildPhaseTodo } from './useSwarmRun';

/**
 * REPAIR v2's per-shard events (repair_waves.rs) were appended to run.jsonl and dropped at the fold's
 * `default:` — a shard that never re-ran its finding's check, or whose fix never flipped it, rendered
 * exactly like a landed fix. Every payload here is the engine's own shape: `repro_verdict` → the four
 * repro events; `decide_promotion` → the five decision events; the merge faults; `complete_fix_completed`
 * closing the row with `promoted`.
 */
const ev = (event: string, rest: Record<string, unknown>) => ({ event, ...rest });
const shard = (round: number, file: string, task: string) => ({ round, shard: file, task_id: task });

const A = shard(1, 'app/api.py', 'complete-fix::app-api');
const B = shard(1, 'app/db.py', 'complete-fix::app-db');
const C = shard(1, 'web/viz.js', 'complete-fix::web-viz');
const D = shard(1, 'app/cli.py', 'complete-fix::app-cli');
const E = shard(2, 'app/api.py', 'complete-fix::app-api-r2');

const STREAM = [
  ev('run_started', { prompt: '# Build app', pool: [{ id: 'mac-a', model_id: 'a-qwen' }] }),
  ev('complete_verify', { findings: 5 }),
  ev('finding_shards', { round: 1, files: 4, shards: 4 }),
  // A: the model way — repro first, the check flips, promoted.
  ev('complete_fix_dispatched', { ...A, finding_index: 0, model: 'a-qwen', baseline_findings: 5, owned: ['app/api.py'], conflict_retry: false }),
  ev('repro_confirmed', { ...A, finding: 'GET /records 500s on ?cursor=', check: 'probe:/records', calls: 3, unparseable_rows: 0, detail: { call: 'curl -s http://127.0.0.1:8000/records?cursor=1' } }),
  ev('finding_flipped', { ...A, finding: 'GET /records 500s on ?cursor=', check: 'probe:/records', command: 'curl -s http://127.0.0.1:8000/records?cursor=1', fails_before: 2, fails_after: 0 }),
  ev('shard_promoted', { task_id: A.task_id, files: ['app/api.py'], three_way_merged: true, created_copied: 0 }),
  ev('complete_fix_completed', { ...A, finding_index: 0, model: 'a-qwen', secs: 610, agent_ok: true, promoted: true, shard_changed: true, conflicted: false, merge_unavailable: false, setup_failed: null }),
  ev('repair_tree_regraded', { round: 1, after_finding: 'GET /records 500s on ?cursor=', findings: 4, baseline_in_force: 4, tree_version: 2 }),
  // B: never re-ran the check, still failing, not promoted.
  ev('complete_fix_dispatched', { ...B, finding_index: 1, model: 'a-qwen', baseline_findings: 4, owned: ['app/db.py'], conflict_retry: false }),
  ev('repro_never_ran', { ...B, finding: 'schema missing the idempotency index', check: 'smoke:sqlite-schema', calls: 2, unparseable_rows: 0, detail: {} }),
  ev('finding_still_failing', { ...B, finding: 'schema missing the idempotency index', check: 'smoke:sqlite-schema', fails_on_preview: 1, quote: 'no such index: idx_idem' }),
  ev('complete_fix_completed', { ...B, finding_index: 1, model: 'a-qwen', secs: 420, agent_ok: true, promoted: false, shard_changed: true, conflicted: false, merge_unavailable: false, setup_failed: null }),
  // C: edited before repro and the preview regressed.
  ev('complete_fix_dispatched', { ...C, finding_index: 2, model: 'a-qwen', baseline_findings: 4, owned: ['web/viz.js'], conflict_retry: false }),
  ev('edit_before_repro', { ...C, finding: 'brush ReferenceError', check: 'render:web/viz.js', calls: 4, unparseable_rows: 1, detail: { first_edit: 'web/viz.js' } }),
  ev('preview_regressed', { ...C, finding: 'brush ReferenceError', check: 'render:web/viz.js', new_failures: [{ check: 'render:web/index.html', quote: 'Uncaught TypeError: draw is not a function' }] }),
  ev('fix_claimed_without_edit', { ...C, finding_n: 1, finding: 'brush ReferenceError', said: 'FIXED — the brush now binds' }),
  ev('dismissed_without_replay', { ...C, finding_n: 2, finding: 'brush ReferenceError', said: 'NOT REAL' }),
  ev('complete_fix_completed', { ...C, finding_index: 2, model: 'a-qwen', secs: 900, agent_ok: true, promoted: false, shard_changed: false, conflicted: false, merge_unavailable: false, setup_failed: null }),
  // D: no authoring check, then a conflict re-arm and a lost promotion on the retry.
  ev('complete_fix_dispatched', { ...D, finding_index: 3, model: 'a-qwen', baseline_findings: 4, owned: ['app/cli.py'], conflict_retry: false }),
  ev('repro_unobservable', { ...D, finding: 'cli exits 0 on a bad flag', check: null, calls: 0, unparseable_rows: 0, detail: { why: 'no calls capture for this shard (primary and mirror empty)' } }),
  ev('finding_unverifiable', { ...D, finding: 'cli exits 0 on a bad flag', rule: 'never promotes', verified_findings: 3, baseline_findings: 4, promote: false }),
  ev('merge_conflict', { ...D, files: [{ file: 'app/cli.py', hunks: 2 }] }),
  ev('complete_fix_completed', { ...D, finding_index: 3, model: 'a-qwen', secs: 300, agent_ok: true, promoted: false, shard_changed: true, conflicted: true, merge_unavailable: false, setup_failed: null }),
  ev('repair_shard_setup_failed', { round: 1, finding: 'cli exits 0 on a bad flag', shard: 'app/cli.py', error: 'no preview temp dir' }),
  // Round 2: A's file again — closed by a sibling meanwhile; another shard loses its promotion.
  ev('complete_fix_dispatched', { ...E, finding_index: 0, model: 'a-qwen', baseline_findings: 3, owned: ['app/api.py'], conflict_retry: true }),
  ev('finding_closed_by_sibling', { ...E, finding: 'GET /records 500s on ?cursor=', check: 'probe:/records' }),
  ev('merge_unavailable', { ...E, said: [{ file: 'app/api.py', why: 'base missing' }] }),
  ev('shard_promotion_lost', { ...E }),
  ev('complete_fix_completed', { ...E, finding_index: 0, model: 'a-qwen', secs: 30, agent_ok: true, promoted: false, shard_changed: true, conflicted: false, merge_unavailable: true, setup_failed: null }),
];

const texts = (feed: Array<{ text: string; sub?: string }>) => feed.map((r) => `${r.text}${r.sub ? ` | ${r.sub}` : ''}`);

describe('REPAIR v2 shard events fold into per-finding rows', () => {
  it('one row per (round, shard): repro verdict, promotion decision and promoted from the engine events', () => {
    const { repairFindings } = buildActivity(STREAM);
    expect(repairFindings.map((r) => `${r.round}:${r.shard}`)).toEqual([
      '1:app/api.py',
      '1:app/db.py',
      '1:web/viz.js',
      '1:app/cli.py',
      '2:app/api.py',
    ]);
    const [a, b, c, d, e] = repairFindings;
    expect(a).toMatchObject({ repro: 'confirmed', decision: 'flipped', failsBefore: 2, failsAfter: 0, promoted: true, check: 'probe:/records' });
    expect(a.reproDetail).toBe('curl -s http://127.0.0.1:8000/records?cursor=1');
    expect(b).toMatchObject({ repro: 'never_ran', decision: 'still_failing', failsAfter: 1, promoted: false });
    expect(c).toMatchObject({ repro: 'edited_first', reproDetail: 'web/viz.js', decision: 'regressed', promoted: false });
    expect(d).toMatchObject({ repro: 'unobservable', decision: 'unverifiable', conflicted: true, promoted: false, setupError: 'no preview temp dir' });
    expect(e).toMatchObject({ decision: 'closed_by_sibling', unavailable: true, promotionLost: true, promoted: false });
  });

  it('a shard still running has promoted: null — never a claim before complete_fix_completed', () => {
    const cut = STREAM.findIndex((x) => x.event === 'finding_flipped') + 1;
    const { repairFindings } = buildActivity(STREAM.slice(0, cut));
    expect(repairFindings).toHaveLength(1);
    expect(repairFindings[0]).toMatchObject({ repro: 'confirmed', decision: 'flipped', promoted: null });
  });

  it('every event is a feed line in the engine’s own terms; the decisions reach the compact feed', () => {
    const { activity, verbose } = buildActivity(STREAM);
    const v = texts(verbose);
    const a = texts(activity);
    expect(v).toContain('Repair round 1 — 4 finding shards across 4 files');
    expect(v).toContain("Repro confirmed — app/api.py (round 1): the finding's own check ran before any edit | curl -s http://127.0.0.1:8000/records?cursor=1");
    expect(a).toContain('Finding flipped — app/api.py (round 1): probe:/records fails 2 → 0 on the preview | GET /records 500s on ?cursor=');
    expect(v).toContain('Shard complete-fix::app-api promoted — 1 file written (three-way merged) | app/api.py');
    expect(v).toContain('Tree re-graded — 4 findings after a landed fix | after: GET /records 500s on ?cursor= · tree v2');
    expect(a).toContain("Repro never ran — app/db.py (round 1): the finding's check was not re-run | check: smoke:sqlite-schema · schema missing the idempotency index");
    expect(a).toContain('Finding still failing — app/db.py (round 1): smoke:sqlite-schema fails 1 on the preview — not promoted | no such index: idx_idem');
    expect(a).toContain("Edited before repro — web/viz.js (round 1): the first edit came before the finding's check | first edit web/viz.js · check: render:web/viz.js · brush ReferenceError");
    expect(a).toContain('Preview regressed — web/viz.js (round 1): 1 new failure — not promoted | render:web/index.html: Uncaught TypeError: draw is not a function');
    expect(a).toContain('FIXED claimed without an edit — web/viz.js (round 1) finding 1: the shadow never diverged from the tree | FIXED — the brush now binds');
    expect(a).toContain('NOT REAL without a replay — web/viz.js (round 1) finding 2: a probe/render finding dismissed without quoting the request and response | NOT REAL');
    expect(v).toContain('Repro unobservable — app/cli.py (round 1) | no calls capture for this shard (primary and mirror empty)');
    expect(a).toContain('Finding unverifiable — app/cli.py (round 1): no authoring check recorded, nothing vouches for the fix — never promotes | cli exits 0 on a bad flag · preview 3 vs tree 4 findings');
    expect(a).toContain('Merge conflict — app/cli.py (round 1): 1 file — the finding re-arms on the new tree | app/cli.py (2 hunks)');
    expect(a).toContain('Repair shard setup failed — app/cli.py (round 1): the harness, not the model | no preview temp dir · cli exits 0 on a bad flag');
    expect(v).toContain("Finding closed by a sibling's landed fix — app/api.py (round 2); this shard is discarded | GET /records 500s on ?cursor=");
    expect(a).toContain('Merge unavailable — app/api.py (round 2): the three-way merge could not run | app/api.py: base missing');
    expect(a).toContain('Promotion lost — app/api.py (round 2): the tree moved between grade and landing; nothing was written');
    for (const line of [...a, ...v]) expect(line).not.toContain('undefined');
  });

  it('the Repair checklist rows count the proof: repro before edit, promoted on the flip, the faults, the re-grade', () => {
    const repair = buildPhaseTodo(STREAM, {}, { clarifyPending: false }).find((p) => p.key === 'repair')!;
    const row = (id: string) => repair.items.find((i) => i.id === id)!;
    expect(row('x-repro-v2')).toMatchObject({
      label: "Repro before the edit — 1 of 4 shards re-ran the finding's own check first",
      state: 'unverified',
      detail: '1 edited first · 1 never ran it · 1 unobservable',
    });
    expect(row('x-flipped')).toMatchObject({
      label: 'Promoted on the flip — 1 of 4 finding shards made their check fail less',
      state: 'unverified',
      detail: '1 still failing · 1 regressed the preview · 1 closed by a sibling · 1 unverifiable (no check) · 1 shard promoted',
    });
    expect(row('x-shard-faults')).toMatchObject({
      label: '6 shard faults — fixes that did not land or claims without proof',
      state: 'failed',
      detail: '1 merge conflicts · 1 merge unavailable · 1 promotion lost · 1 harness setup failed · 1 FIXED claimed without an edit · 1 NOT REAL without a replay',
    });
    expect(row('x-regraded')).toMatchObject({
      label: 'Tree re-graded after the last landed fix — 4 findings remain',
      state: 'done',
      detail: 'tree v2',
    });
  });

  it('a wave whose every shard confirmed its repro and flipped reads done', () => {
    const clean = STREAM.filter((x) => {
      const r = x as Record<string, unknown>;
      return (
        r['event'] === 'run_started' ||
        r['event'] === 'complete_verify' ||
        (r['shard'] === 'app/api.py' && r['round'] === 1) ||
        r['event'] === 'shard_promoted'
      );
    });
    const repair = buildPhaseTodo(clean, {}, { clarifyPending: false }).find((p) => p.key === 'repair')!;
    expect(repair.items.find((i) => i.id === 'x-repro-v2')).toMatchObject({ state: 'done', detail: undefined });
    expect(repair.items.find((i) => i.id === 'x-flipped')).toMatchObject({ state: 'done', detail: '1 shard promoted' });
    expect(repair.items.some((i) => i.id === 'x-shard-faults')).toBe(false);
  });
});

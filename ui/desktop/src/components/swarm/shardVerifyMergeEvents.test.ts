import { describe, expect, it } from 'vitest';
import { buildActivity, buildPhaseTodo } from './useSwarmRun';

/**
 * SPLIT v2's verification, holes and assembly events (shard_verify.rs, merge_holes.rs, shards/assembly.rs,
 * shards.rs) were dropped at the fold's `default:`. The engine emits NEGATIVES ONLY for verification, so
 * a shard row exists here only once the engine said something — the absence of a row never reads as
 * "verified clean". Payloads are the emit shapes verbatim.
 */
const ev = (event: string, rest: Record<string, unknown>) => ({ event, ...rest });
const SH = (shard: string) => ({ module: 'web-viz', shard, task_id: `web-viz-shard-${shard}`, folder: `.swarm/shards/web-viz/${shard}` });

const STREAM = [
  ev('run_started', { prompt: '# Build app', pool: [{ id: 'mac-a', model_id: 'a-qwen' }] }),
  ev('shard_piece_unparsed', { ...SH('camera'), piece: 'camera.js', error: 'Unexpected token (12:3)' }),
  ev('shard_undefined_ref', { ...SH('camera'), names: ['rebuildPickFBO', 'project'], pieces_scanned: 2 }),
  ev('shard_check_unavailable', { ...SH('render'), check: 'parse', tool: 'node', pieces: ['render.js'] }),
  ev('shard_check_unavailable', { ...SH('render'), check: 'free_identifier_scan', tool: null, ext: 'glsl', pieces: ['shader.glsl'], reason: 'no free-identifier scan for this extension (js/mjs/cjs and py only)' }),
  ev('shard_pieces_absent', { ...SH('legend'), readme_present: true }),
  ev('merge_note_missing', { ...SH('legend'), reason: 'the shard wrote no README' }),
  ev('merge_dossier_incomplete', { module: 'web-viz', task_id: 'web-viz', missing: ['legend'], pieces_absent: ['legend'], undefined_refs: [{ shard: 'camera', names: ['rebuildPickFBO', 'project'] }], pieces_unparsed: [{ shard: 'camera', piece: 'camera.js', error: 'Unexpected token (12:3)' }] }),
  ev('merge_hole', { module: 'web-viz', task_id: 'web-viz', shards_missing: ['legend'], readmes_missing: ['legend'], merge_readme_present: false }),
  ev('merge_assembled', { module: 'web-viz', task_id: 'web-viz', path: 'web/viz.js', ext: 'js', pieces: 5, pieces_skipped: [{ path: 'camera.js', why: 'did not parse' }], definitions: 23, ordered_by_interface: 21, appended_unknown: [{ shard: 'render', names: ['drawGrid'] }], duplicates: [{ name: 'project', shards: ['camera', 'render'] }], imports: 3, statements: [{ shard: 'render', blocks: 2 }], declared_missing: ['rebuildPickFBO'], order_source: 'declared_interface', glue_needed: ['window.vs7dbg'], bytes: 41000, lines: 1180 }),
  ev('merge_duplicate_definition', { module: 'web-viz', task_id: 'web-viz', name: 'project', shards: ['camera', 'render'], kept: 'both, under a MERGE_DUPLICATE marker — the merger resolves' }),
  ev('merge_gap_predictable', { module: 'web-viz', task_id: 'web-viz', item: 'the legend panel', shard: 'legend', unfinished: 'UNFINISHED: legend panel not started' }),
  ev('merge_gap_requested', { module: 'web-viz', shard: 'web-viz-gap-1', task_id: 'web-viz', missing: 'the legend panel', folder: '.swarm/shards/web-viz/gap-1' }),
  ev('merge_promoted', { module: 'web-viz', task_id: 'web-viz', files: ['web/viz.js'] }),
];

const texts = (feed: Array<{ text: string; sub?: string }>) => feed.map((r) => `${r.text}${r.sub ? ` | ${r.sub}` : ''}`);

describe('SPLIT v2 verification and assembly events reach the fold', () => {
  it('shard verification rows exist only for shards the engine said something about', () => {
    const { shardVerify, mergeAssembled } = buildActivity(STREAM);
    expect(shardVerify.map((r) => r.shard).sort()).toEqual(['camera', 'legend', 'render']);
    const camera = shardVerify.find((r) => r.shard === 'camera')!;
    expect(camera).toMatchObject({ taskId: 'web-viz-shard-camera', module: 'web-viz', undefinedRefs: ['rebuildPickFBO', 'project'], piecesScanned: 2, piecesAbsent: false });
    expect(camera.piecesUnparsed).toEqual([{ piece: 'camera.js', error: 'Unexpected token (12:3)' }]);
    const render = shardVerify.find((r) => r.shard === 'render')!;
    expect(render.checksUnavailable).toEqual([
      { check: 'parse', reason: 'node is not installed' },
      { check: 'free_identifier_scan', reason: 'no free-identifier scan for this extension (js/mjs/cjs and py only)' },
    ]);
    expect(shardVerify.find((r) => r.shard === 'legend')).toMatchObject({ piecesAbsent: true });
    expect(mergeAssembled).toHaveLength(1);
    expect(mergeAssembled[0]).toMatchObject({
      module: 'web-viz',
      path: 'web/viz.js',
      pieces: 5,
      definitions: 23,
      lines: 1180,
      orderSource: 'declared_interface',
      glueNeeded: ['window.vs7dbg'],
      declaredMissing: ['rebuildPickFBO'],
      duplicates: [{ name: 'project', shards: ['camera', 'render'] }],
      piecesSkipped: [{ path: 'camera.js', why: 'did not parse' }],
    });
    // A clean split says nothing, and the fold claims nothing.
    expect(buildActivity(STREAM.slice(0, 1)).shardVerify).toEqual([]);
  });

  it('every event is a feed line from its own fields; the faults reach the compact feed', () => {
    const { activity, verbose } = buildActivity(STREAM);
    const a = texts(activity);
    const v = texts(verbose);
    expect(a).toContain('Shard camera of web-viz — piece camera.js did not parse | Unexpected token (12:3)');
    expect(a).toContain('Shard camera of web-viz — 2 undefined names across 2 scanned pieces | rebuildPickFBO, project');
    expect(v).toContain('Shard render of web-viz — parse unavailable for 1 piece | node is not installed · render.js');
    expect(v).toContain('Shard render of web-viz — free identifier scan unavailable for 1 piece | no free-identifier scan for this extension (js/mjs/cjs and py only) · shader.glsl');
    expect(a).toContain('Shard legend of web-viz delivered no pieces — README present, folder empty | .swarm/shards/web-viz/legend');
    expect(a).toContain('Shard legend of web-viz left no README — the merger reads its folder blind | the shard wrote no README · .swarm/shards/web-viz/legend');
    expect(a).toContain('Merger web-viz dispatched over an incomplete dossier — READMEs missing 1 · pieces absent 1 · undefined refs 1 · unparsed 1 | no README: legend\nempty: legend\ncamera: rebuildPickFBO, project\ncamera camera.js: Unexpected token (12:3)');
    expect(a).toContain('Merge of web-viz has holes — 1 shard missing and unexplained | missing: legend · no README: legend · MERGE.md missing');
    expect(a).toContain('Merge of web-viz assembled — 5 pieces, 23 definitions, 1,180 lines into web/viz.js | ordered by declared interface · glue needed: window.vs7dbg · declared but missing: rebuildPickFBO · 1 duplicate definition · 1 piece skipped');
    expect(v).toContain('Merge of web-viz — `project` defined by 2 shards; both kept under a MERGE_DUPLICATE marker | camera, render');
    expect(a).toContain('Merge gap was predictable — web-viz: `the legend panel` was listed UNFINISHED by legend | UNFINISHED: legend panel not started');
    expect(v).toContain('Merge gap requested — web-viz: `the legend panel` → shard web-viz-gap-1 | .swarm/shards/web-viz/gap-1');
    expect(v).toContain('Merge of web-viz promoted — 1 file | web/viz.js');
    // "undefined names" / "undefined refs" are the engine's own terms; a bare `undefined` is a missing field.
    for (const line of [...a, ...v]) expect(line).not.toMatch(/\bundefined\b(?! (names?|refs))/);
  });

  it('the Split checklist carries a row per said shard, the holes, the dossier and the assembly', () => {
    const split = buildPhaseTodo(STREAM, {}, { clarifyPending: false }).find((p) => p.key === 'split')!;
    const row = (id: string) => split.items.find((i) => i.id === id)!;
    expect(row('s-verify-web-viz-shard-camera')).toMatchObject({
      label: 'Shard camera of web-viz verified with 3 problems',
      state: 'failed',
      detail: '1 piece did not parse · 2 undefined names',
    });
    expect(row('s-verify-web-viz-shard-render')).toMatchObject({
      label: 'Shard render of web-viz verified with 0 problems',
      state: 'advisory',
      detail: '2 checks unavailable',
    });
    expect(row('s-verify-web-viz-shard-legend')).toMatchObject({
      label: 'Shard legend of web-viz delivered no pieces',
      state: 'failed',
    });
    expect(row('s-hole-web-viz')).toMatchObject({ state: 'unverified', detail: '1 README missing' });
    expect(row('s-dossier-web-viz').detail).toBe('READMEs missing 1 · pieces absent 1 · undefined refs 1 · unparsed 1');
    expect(row('s-assembled-web-viz')).toMatchObject({
      label: 'Merge of web-viz assembled — 5 pieces, 23 definitions, 1,180 lines',
      state: 'done',
      detail: 'glue needed for 1 name · 1 declared export missing · 1 duplicate definition · 1 piece skipped · promoted',
    });
    // Unpromoted and unclean: the assembly stays unverified.
    const unpromoted = buildPhaseTodo(STREAM.filter((x) => x.event !== 'merge_promoted'), {}, { clarifyPending: false }).find((p) => p.key === 'split')!;
    expect(unpromoted.items.find((i) => i.id === 's-assembled-web-viz')!.state).toBe('unverified');
  });
});

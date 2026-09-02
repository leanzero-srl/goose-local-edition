import { cleanup, render, renderHook, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import FormationRibbon from './FormationRibbon';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { fmtPhaseDuration, formationPhasesFor, phaseDurationMs } from './formationVisualState';
import {
  buildActivity,
  buildPhaseTodo,
  foldEvents,
  foldRunPhase,
  resetFoldCache,
  resetLiveChannelMemory,
  useSwarmRun,
} from './useSwarmRun';
import { planningLanesFor, unclaimedPlanningLanes } from './phaseList';

/**
 * VA-138 (Mihai, r6j, 2026-09-02 22:3x, watching the desktop: "wasn't synthesize supposed to take a few
 * minutes?" — "oh man we need to update this UI to reflect the steps better"). The panel showed
 * SYNTHESIS for 35 minutes while synthesis itself took 12 and a `split-web-viz` lane ran the other 23
 * under the same chip, listed in the trailing "Planning calls" group with nothing saying which step
 * it belonged to.
 *
 * The stream below is r6j's own run.jsonl (benchmark/runs/build/swarm-3node-r0), cut to the events the
 * steps derive from, timestamps verbatim: `phase: open` 15:36:04 → `slices_opened` 16:28:14 (52m) →
 * research 16:28:14 → `phase: synthesis` 18:52:32 (2h 24m) → `plan_synthesized` … `plan_flag{fat_task}`
 * 19:04:30 (12m) → the split lane, still forming at 19:27:40 (23m). Nothing here is an engine change:
 * every step is derived from events the engine already emits.
 */
type Ev = Record<string, unknown>;

const POOL = [
  { id: 'mac-gabee-qwen3.8-27b', model_id: 'gabee-qwen3.8-27b', weight: 1, speed_weight: 1 },
  { id: 'local-mihai-qwen3.8-27b', model_id: 'mihai-qwen3.8-27b', weight: 1, speed_weight: 2 },
  {
    id: 'worksmacstudio-workhorse-qwen3.8-27b',
    model_id: 'workhorse-qwen3.8-27b',
    weight: 1,
    speed_weight: 3,
  },
];
const SLICES = [
  'ledgerd-core',
  'ledgerd-api',
  'ledgerd-webhooks-drafts',
  'notifierd',
  'web-page',
  'web-viz',
];
const SECTIONS: Record<string, number> = {
  'ledgerd-api': 9,
  'ledgerd-core': 7,
  'ledgerd-webhooks-drafts': 3,
  notifierd: 1,
  'web-page': 4,
  'web-viz': 10,
};

const at = (ts: string, e: Ev): Ev => ({ ...e, ts });

const dispatch = (ts: string, slice: string, rank: number, host: string): Ev[] => [
  at(ts, {
    event: 'research_dispatch_order',
    slice,
    sections: SECTIONS[slice],
    rank,
    host,
    host_speed: 1,
  }),
  at(ts, {
    event: 'research_dispatched',
    slice,
    derives: true,
    q_indexes: [],
    model: host,
    activity_key: `research-${slice}`,
  }),
];
const landed = (ts: string, slice: string, q: number, kind: string, chars: number): Ev[] => [
  at(ts, { event: 'research_question_kind', slice, q_index: q, kind, source: 'model', cite: '' }),
  at(ts, {
    event: 'research_answered',
    slice,
    q_index: q,
    chars,
    raised: 0,
    secs: 120,
    batch: 0,
    model: 'x',
  }),
  at(ts, {
    event: 'research_answer_landed',
    task: `research-${slice}`,
    slice,
    q_index: q,
    kind,
    status: 'answered',
    chars,
    raised: 0,
    via: 'tool',
  }),
];
const closed = (ts: string, slice: string, next: number, n: number): Ev =>
  at(ts, {
    event: 'research_unanswered',
    slice,
    q_index: next,
    reason: 'remainder_empty',
    detail: `${n} question(s) landed through research_answer; the final reply added none and listed builder_decides`,
    secs: 2049,
    model: 'x',
  });

const OPENING: Ev[] = [
  at('2026-09-02T15:36:03.606000+00:00', {
    event: 'run_started',
    prompt: '# Build `app` — Meridian Payments Console\n\nAn operations product.',
    pool: POOL,
  }),
  at('2026-09-02T15:36:03.640000+00:00', { event: 'pool_resolved', devices: POOL }),
  at('2026-09-02T15:36:04.206822+00:00', { event: 'phase', phase: 'open' }),
];
const OPENED: Ev[] = [
  at('2026-09-02T16:28:14.126495+00:00', {
    event: 'slices_opened',
    count: 6,
    weights: [5, 5, 4, 3, 4, 5],
    slices: SLICES,
    secs: 3129,
  }),
  at('2026-09-02T16:28:14.134042+00:00', {
    event: 'research_planned',
    lanes: 6,
    per_slice_sections: SECTIONS,
    resumed_slices: [],
    decisions: 0,
  }),
  at('2026-09-02T16:28:14.134086+00:00', { event: 'phase', phase: 'research' }),
];
const RESEARCH: Ev[] = [
  ...dispatch('2026-09-02T16:28:14.135685+00:00', 'web-viz', 0, 'workhorse-qwen3.8-27b'),
  ...dispatch('2026-09-02T16:28:14.135791+00:00', 'ledgerd-api', 1, 'mihai-qwen3.8-27b'),
  ...dispatch('2026-09-02T16:28:14.135570+00:00', 'ledgerd-core', 2, 'gabee-qwen3.8-27b'),
  ...landed('2026-09-02T16:49:25.155494+00:00', 'web-viz', 0, 'design', 2007),
  ...landed('2026-09-02T16:50:08.544460+00:00', 'web-viz', 1, 'design', 569),
  ...landed('2026-09-02T16:51:15.639067+00:00', 'web-viz', 2, 'design', 890),
  ...landed('2026-09-02T16:59:21.532020+00:00', 'web-viz', 3, 'design', 1181),
  closed('2026-09-02T17:02:23.220532+00:00', 'web-viz', 4, 4),
  ...dispatch('2026-09-02T17:02:23.220768+00:00', 'web-page', 3, 'workhorse-qwen3.8-27b'),
  ...landed('2026-09-02T17:21:55.379099+00:00', 'web-page', 0, 'design', 2860),
  closed('2026-09-02T17:24:46.898000+00:00', 'web-page', 1, 1),
  ...dispatch('2026-09-02T17:24:46.898779+00:00', 'ledgerd-webhooks-drafts', 4, 'workhorse-qwen3.8-27b'),
  ...landed('2026-09-02T17:25:49.456313+00:00', 'ledgerd-core', 0, 'external', 636),
  ...landed('2026-09-02T17:28:59.742524+00:00', 'ledgerd-core', 1, 'external', 674),
  ...landed('2026-09-02T17:33:21.718386+00:00', 'ledgerd-core', 2, 'design', 3187),
  closed('2026-09-02T17:36:35.638000+00:00', 'ledgerd-core', 3, 3),
  ...dispatch('2026-09-02T17:36:35.638859+00:00', 'notifierd', 5, 'gabee-qwen3.8-27b'),
  ...landed('2026-09-02T18:03:42.050842+00:00', 'notifierd', 0, 'design', 1172),
  closed('2026-09-02T18:05:00.000000+00:00', 'notifierd', 1, 1),
  ...landed('2026-09-02T18:10:33.301730+00:00', 'ledgerd-webhooks-drafts', 0, 'external', 908),
  ...landed('2026-09-02T18:11:54.395088+00:00', 'ledgerd-webhooks-drafts', 1, 'external', 821),
  closed('2026-09-02T18:14:12.000000+00:00', 'ledgerd-webhooks-drafts', 2, 2),
  ...landed('2026-09-02T18:39:31.183889+00:00', 'ledgerd-api', 0, 'design', 1723),
  ...landed('2026-09-02T18:43:17.642071+00:00', 'ledgerd-api', 1, 'design', 1005),
  closed('2026-09-02T18:51:06.411020+00:00', 'ledgerd-api', 2, 2),
];
const SYNTHESIS: Ev[] = [
  at('2026-09-02T18:52:32.534479+00:00', { event: 'phase', phase: 'synthesis' }),
  at('2026-09-02T19:04:30.590000+00:00', {
    event: 'plan_synthesized',
    tasks: 7,
    distinct_files: 24,
    ids: [...SLICES, 'integrate-verify'],
  }),
  at('2026-09-02T19:04:30.600000+00:00', {
    event: 'plan_repaired',
    source: 'plan',
    before: { tasks: 8 },
    after: { tasks: 8 },
    actions: ['shared file `app/notifierd/__main__.py`: kept by `skeleton`'],
  }),
  at('2026-09-02T19:04:30.605894+00:00', {
    event: 'research_answer_routed',
    from_slice: 'web-viz',
    to_task: 'web-page',
    q_index: 3,
    matched: 'file',
    value: 'web/styles.css',
    owner: 'web-page',
    arm: 'owned_here',
  }),
  at('2026-09-02T19:04:30.606005+00:00', {
    event: 'research_answer_unowned',
    from_slice: 'ledgerd-api',
    q_index: 0,
    names: ['webhooks.py/drafts.py'],
  }),
  at('2026-09-02T19:04:30.606026+00:00', {
    event: 'research_answer_routed',
    from_slice: 'ledgerd-api',
    to_task: 'ledgerd-webhooks-drafts',
    q_index: 0,
    matched: 'file',
    value: 'app/ledgerd/webhooks.py',
    owner: 'ledgerd-webhooks-drafts',
    arm: 'owned_here',
  }),
  at('2026-09-02T19:04:30.608426+00:00', {
    event: 'plan_weighted',
    unit: 'claimed spec sections',
    weights: SECTIONS,
  }),
];
// The measurement that summons the split — r6j seq 751, verbatim numbers.
const FAT: Ev = at('2026-09-02T19:04:30.608684+00:00', {
  event: 'plan_flag',
  kind: 'fat_task',
  task: 'web-viz',
  files: ['web/viz.js'],
  sections: 10,
  section_chars: 16488,
  brief_chars: 48800,
  sections_per_file: 10.0,
  chars_per_file: 16488.0,
  median: 1.45,
  mean: 2.6222222222222222,
  stddev: 3.324525400083818,
  threshold: 5.94674762230604,
  floor: 2.9,
});
const SHARDS = ['web-viz-data-scene', 'web-viz-render', 'web-viz-interaction'];
const SPLIT_DONE: Ev[] = [
  at('2026-09-02T19:27:40.000000+00:00', {
    event: 'split_sized',
    module: 'web-viz',
    declared: 5,
    hosts: 3,
    shards: 3,
    groups: [['data-scene', 'render-core'], ['pick-buffer', 'camera'], ['labels-brush-stream']],
    weights: [2, 3, 1, 2, 4],
    source: 'fleet — clusters grouped contiguously onto the free hosts, largest group minimised',
  }),
  at('2026-09-02T19:27:40.100000+00:00', {
    event: 'plan_patched',
    source: 'split',
    module: 'web-viz',
    shards: SHARDS,
    exports_declared: 12,
    replace: 1,
    add: 3,
    remove: 0,
    after: { tasks: 10, distinct_files: 27 },
  }),
  at('2026-09-02T19:27:40.200000+00:00', {
    event: 'plan_repaired',
    source: 'split',
    before: { tasks: 10 },
    after: { tasks: 10 },
    actions: [],
  }),
];
const LOADED: Ev[] = [
  at('2026-09-02T19:27:41.000000+00:00', {
    event: 'plan_loaded',
    task_count: 10,
    tasks: [
      ...SLICES.map((id) => ({ id, description: `Build ${id}`, files: [`app/${id}.py`], deps: [] })),
      ...SHARDS.map((id) => ({ id, description: `Shard ${id}`, files: [], deps: [] })),
      { id: 'integrate-verify', description: 'Sink', files: [], deps: SLICES },
    ],
  }),
  at('2026-09-02T19:27:42.000000+00:00', {
    event: 'task_dispatched',
    task_id: 'web-viz-data-scene',
    device: 'worksmacstudio-workhorse-qwen3.8-27b',
    model: 'workhorse-qwen3.8-27b',
  }),
];

const MID_SPLIT: Ev[] = [...OPENING, ...OPENED, ...RESEARCH, ...SYNTHESIS, FAT];
const BUILDING: Ev[] = [...MID_SPLIT, ...SPLIT_DONE, ...LOADED];
/** The same run with no fat task: synthesis → plan_loaded straight through. */
const NO_FAT: Ev[] = [...OPENING, ...OPENED, ...RESEARCH, ...SYNTHESIS, ...LOADED];
/** 22:27 local on the vigil's clock — 19:27:40Z, the moment Mihai asked. */
const NOW = Date.parse('2026-09-02T19:27:40.000000+00:00');

const MIN = 60_000;

describe('VA-138 — the steps the engine actually walks, clocked from its own timestamps', () => {
  it('foldRunPhase enters SPLIT on plan_flag{fat_task} and leaves it on plan_loaded', () => {
    const visited: string[] = [];
    for (let i = 1; i <= BUILDING.length; i += 1) {
      const { phase } = foldRunPhase(BUILDING.slice(0, i));
      if (phase && visited[visited.length - 1] !== phase) visited.push(phase);
    }
    expect(visited).toEqual(['open', 'research', 'synthesize', 'split', 'build']);
    expect(foldRunPhase(MID_SPLIT).phase).toBe('split');
    expect(foldRunPhase(MID_SPLIT).observed.split).toBe(true);
    // No open decision on this run: ask was never entered, so it is never offered.
    expect(foldRunPhase(BUILDING).observed.ask).toBeUndefined();
  });

  it('each step is clocked from the event timestamps — open 52m · research 2h 24m · synthesize 12m · split 23m', () => {
    const { spans } = foldRunPhase(MID_SPLIT);
    const minutes = (key: Parameters<typeof phaseDurationMs>[1]) =>
      Math.round((phaseDurationMs(spans, key, NOW) ?? NaN) / MIN);
    expect(minutes('open')).toBe(52);
    expect(minutes('research')).toBe(144);
    expect(minutes('synthesize')).toBe(12);
    // The live step reads against `now`; it was entered at 19:04:30.
    expect(minutes('split')).toBe(23);
    expect(phaseDurationMs(spans, 'build', NOW)).toBeNull();
    expect(fmtPhaseDuration(phaseDurationMs(spans, 'research', NOW)!)).toBe('2h 24m');
    expect(fmtPhaseDuration(phaseDurationMs(spans, 'synthesize', NOW)!)).toBe('12m');
    // Once the plan loaded the split is closed: its clock stops at 19:27:41 whatever `now` says.
    const after = foldRunPhase(BUILDING).spans;
    expect(Math.round(phaseDurationMs(after, 'split', NOW + 90 * MIN)! / MIN)).toBe(23);
    expect(after.phases.split?.since).toBeNull();
    // A run that is no longer live reads its open step against the NEWEST EVENT, never a live clock.
    expect(Math.round(phaseDurationMs(after, 'build')! / MIN)).toBe(0);
    expect(after.lastTs).toBe(Date.parse('2026-09-02T19:27:42.000000+00:00'));
  });

  it('the ribbon draws Split as its own chip with 23m under it, and no Ask chip on a run that asked nothing', () => {
    const { phase, observed, spans } = foldRunPhase(MID_SPLIT);
    render(
      <FormationRibbon
        phase={phase}
        evidence={observed}
        spans={spans}
        now={NOW}
        nodes={[
          { device: 'gabee', working: false },
          { device: 'mihai', working: false },
          { device: 'workhorse', working: true },
        ]}
      />
    );
    const chips = within(screen.getByRole('list', { name: 'Run phases' })).getAllByRole('listitem');
    // The chip is the li's first child; its clock is the sibling under it.
    expect(chips.map((li) => `${li.firstElementChild?.textContent?.trim()}:${li.dataset.state}`)).toEqual([
      'Open:complete',
      'Research:complete',
      'Synthesize:complete',
      'Split:active',
      'Build:upcoming',
      'Integrate:upcoming',
      'Repair:upcoming',
      'Done:upcoming',
    ]);
    const duration = (key: string) =>
      document.querySelector(`[data-testid="formation-duration"][data-duration-phase="${key}"]`)
        ?.textContent;
    expect(duration('open')).toBe('52m');
    expect(duration('research')).toBe('2h 24m');
    expect(duration('synthesize')).toBe('12m');
    expect(duration('split')).toBe('23m');
    expect(duration('build')).toBe('');
    // The fleet sits under SPLIT, not under Synthesize.
    expect(
      within(document.querySelector('[data-formation-phase="split"]') as HTMLElement).getAllByTestId(
        'formation-node'
      )
    ).toHaveLength(3);
    expect(
      document.querySelector('[data-formation-phase="synthesize"] [data-testid="formation-node"]')
    ).toBeNull();
  });

  it('the step list is derived: the same run with no fat task has no Split step anywhere', () => {
    const { phase, observed } = foldRunPhase(NO_FAT);
    expect(phase).toBe('build');
    expect(observed.split).toBeUndefined();
    expect(formationPhasesFor(observed).map((s) => s.key)).toEqual([
      'open',
      'research',
      'synthesize',
      'build',
      'integrate',
      'repair',
      'done',
    ]);
    const todo = buildPhaseTodo(NO_FAT, {}, { clarifyPending: false });
    expect(todo.find((p) => p.key === 'split')!.items).toHaveLength(0);
    expect(planningLanesFor('split', { ...foldEvents(NO_FAT, {}), planningLanes: [] })?.lanes).toEqual([]);
  });

  describe('the checklist: every step lists its lanes and what each delivered', () => {
    const todo = (events: Ev[]) => buildPhaseTodo(events, {}, { clarifyPending: false });
    const phase = (events: Ev[], key: string) => todo(events).find((p) => p.key === key)!;

    it('SPLIT: the fat task while the call is out, then its sizing and its one patch', () => {
      const live = phase(MID_SPLIT, 'split');
      expect(live.active).toBe(true);
      expect(live.items.map((i) => `${i.id}:${i.state}`)).toEqual(['s-fat-web-viz:running']);
      expect(live.items[0].label).toBe('Fat task web-viz — asking synthesis for a split');
      expect(live.items[0].detail).toBe('10 spec sections for 1 file · 10.0/file vs threshold 5.9');
      // Synthesize is CLOSED behind it — never "still running" through the split.
      expect(phase(MID_SPLIT, 'synthesis').active).toBe(false);
      expect(phase(MID_SPLIT, 'synthesis').items.some((i) => i.state === 'running')).toBe(false);

      const done = phase(BUILDING, 'split');
      expect(done.items.map((i) => `${i.id}:${i.state}`)).toEqual([
        's-sized-web-viz:done',
        's-split-web-viz:done',
      ]);
      expect(done.items[0].label).toBe('web-viz sized to 3 shards on 3 free hosts');
      expect(done.items[0].detail).toBe('5 declared · weights 2, 3, 1, 2, 4');
      expect(done.items[1].label).toBe('Fat task web-viz split into 3 shards + a merger');
      expect(done.items[1].detail).toBe('12 exports declared · plan now 10 tasks');
    });

    it('SYNTHESIZE: the call closes at plan_synthesized and says what it routed', () => {
      const s = phase(MID_SPLIT, 'synthesis');
      expect(s.items.map((i) => `${i.id}:${i.state}`)).toEqual(['s-wired:done', 's-routed:done']);
      expect(s.items[0].detail).toBe(
        '7 tasks · the deterministic repairs and any split run before the plan loads'
      );
      expect(s.items[1].label).toBe(
        'Routed 2 research answers into the briefs of the tasks that own their files'
      );
      expect(s.items[1].detail).toBe('1 answer named files no task owns — left unrouted');
      expect(phase(BUILDING, 'synthesis').items.map((i) => i.id)).toEqual([
        's-routed',
        's-done',
      ]);
    });

    it('RESEARCH: six lane rows in dispatch order — landed, kinds, rank, host — closed by remainder_empty, never a miss', () => {
      const r = phase(MID_SPLIT, 'research');
      const lanes = r.items.filter((i) => i.id.startsWith('r2-lane-'));
      expect(lanes.map((i) => i.label)).toEqual([
        'web-viz lane',
        'ledgerd-api lane',
        'ledgerd-core lane',
        'web-page lane',
        'ledgerd-webhooks-drafts lane',
        'notifierd lane',
      ]);
      expect(lanes[0]).toMatchObject({
        state: 'done',
        device: 'workhorse',
        detail:
          'landed 4 · 4 design · rank 0 · 10 sections · closed — the final reply added nothing behind the landed answers',
      });
      expect(lanes[2]).toMatchObject({ device: 'gabee', state: 'done' });
      expect(lanes[2].detail).toContain('landed 3 · 1 design, 2 external · rank 2 · 7 sections');
      // The lane closers are outcome rows for builder_decides (research.rs), not questions kept raw
      // in a brief: the summary counts 13 of 13, and no miss row appears.
      expect(r.items[0].label).toBe(
        'Research — 13 of 13 derived questions answered across 6 lanes'
      );
      expect(r.items.some((i) => i.id === 'r2-miss')).toBe(false);
      // Mid-research the lanes still out read running.
      const early = phase([...OPENING, ...OPENED, ...RESEARCH.slice(0, 6)], 'research');
      expect(early.items.find((i) => i.id === 'r2-lane-web-viz')).toMatchObject({
        state: 'running',
        detail: 'landed 0 · rank 0 · 10 sections · running',
      });
    });

    it('the header badge reads Splitting, and the feed carries the sizing', () => {
      expect(buildActivity(MID_SPLIT).phase).toBe('Splitting web-viz');
      const { activity } = buildActivity(BUILDING);
      const sized = activity.find((r) => r.text.startsWith('Split of `web-viz` sized'))!;
      expect(sized.text).toBe('Split of `web-viz` sized — 3 shards from 5 declared on 3 free hosts');
      expect(sized.sub).toBe(
        'cluster weights 2, 3, 1, 2, 4 · fleet — clusters grouped contiguously onto the free hosts, largest group minimised'
      );
      const scarce = buildActivity([
        ...MID_SPLIT,
        { event: 'split_hosts_scarce', task: 'web-viz', free_hosts: 1, detail: 'fewer than two free hosts at split time' },
      ]).activity.find((r) => r.text.includes('sized by its declaration'))!;
      expect(scarce.text).toBe('Split of `web-viz` sized by its declaration — 1 free host at split time');
      expect(scarce.tone).toBe('warn');
    });
  });
});

// ── Rendered: the Split step in the planning zone with its lane, the ribbon's clock, the header badge. ──

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

/** r6j's split-web-viz.json at 22:29 local: two shell calls, 83k of thinking, still forming on workhorse. */
const SPLIT_DIGEST = {
  tool_calls: 2,
  errors: 0,
  malformed: 0,
  recent: ['shell ok', 'shell ok'],
  inflight: [],
  last_text: '',
  calls: [
    { name: 'shell', summary: 'ls -la; find . -maxdepth 2 -type d | head -50', ok: true },
    { name: 'shell', summary: 'ls -la .swarm/ledger .swarm/activity | head -30', ok: true },
  ],
  thinking_chars: 83676,
  last_thinking:
    'Partition the 10 claimed sections into shards that can be written independently: data → scene, rendering, pick buffer…',
  model: 'workhorse-qwen3.8-27b',
  attempt: 0,
  dispatched_at: '2026-09-02T19:04:30.661641+00:00',
};
const DONE_DIGEST = (model: string, text: string) => ({
  tool_calls: 3,
  last_text: text,
  model,
  attempt: 0,
  phase: 'done',
});

describe('VA-138 rendered — what r6j’s panel would have shown at 22:27', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    // The run's own offsets, slid so plan_flag landed 23 minutes before the test's clock: the panel
    // reads a live run against Date.now(), and the numbers under the chips must be r6j's.
    const shift = Date.now() - 23 * MIN - Date.parse(FAT['ts'] as string);
    const events = MID_SPLIT.map((e) => ({
      ...e,
      ts: new Date(Date.parse(e['ts'] as string) + shift).toISOString(),
    }));
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-3node-r0',
      dir: '/tmp/build',
      events,
      activity: {
        open: DONE_DIGEST('gabee-qwen3.8-27b', 'six balanced slices'),
        'research-web-viz': DONE_DIGEST('workhorse-qwen3.8-27b', 'four answers landed'),
        synthesis: DONE_DIGEST('mihai-qwen3.8-27b', 'seven tasks wired'),
        'split-web-viz': SPLIT_DIGEST,
      },
      activityMtimes: { 'split-web-viz': Date.now() },
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    e.fleetStatus = vi.fn(async () => ({}));
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('Split · web-viz · 23m · 1 lane — instead of "Synthesis 35m"', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(result.current.runPhase).toBe('split');
    expect(result.current.phase).toBe('Splitting web-viz');
    // The split lane is the split step's, and the trailing planning-calls group has nothing left.
    const folded = result.current;
    expect(planningLanesFor('split', folded)?.lanes.map((l) => l.taskId)).toEqual(['split-web-viz']);
    expect(planningLanesFor('synthesis', folded)?.lanes.map((l) => l.taskId)).toEqual(['synthesis']);
    expect(planningLanesFor('open', folded)?.lanes.map((l) => l.taskId)).toEqual(['open']);
    expect(unclaimedPlanningLanes(folded.planningLanes)).toEqual([]);

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
    const ribbon = await screen.findByTestId('formation-ribbon');
    expect(ribbon).toHaveAttribute('data-active-phase', 'split');
    const duration = (key: string) =>
      ribbon.querySelector(`[data-testid="formation-duration"][data-duration-phase="${key}"]`)
        ?.textContent;
    expect(duration('synthesize')).toBe('12m');
    expect(duration('split')).toBe('23m');
    expect(duration('research')).toBe('2h 24m');
    expect(duration('open')).toBe('52m');
    expect(ribbon.querySelector('[data-duration-phase="ask"]')).toBeNull();

    // The planning zone: Split has its row, its lane, and the running state; Synthesize is closed.
    const split = await screen.findByTestId('planning-phase-split');
    expect(split.dataset.phaseState).toBe('running');
    expect(split.textContent).toContain('Fat task web-viz — asking synthesis for a split');
    expect(split.textContent).toContain('Split calls · 1 lane');
    const lanes = split.querySelectorAll('[data-testid="turn-lane"]');
    expect(lanes).toHaveLength(1);
    expect(lanes[0].textContent).toContain('Split web-viz · declaring the shard interface of a fat task');
    expect(lanes[0].textContent).toContain('workhorse');
    const synthesis = screen.getByTestId('planning-phase-synthesis');
    expect(synthesis.dataset.phaseState).toBe('done');
    expect(synthesis.textContent).toContain('Synthesis call · 1 lane');
    expect(synthesis.textContent).not.toContain('Wiring the slices into a task DAG…');
    expect(screen.queryByText('Planning calls', { exact: false })).toBeNull();
    // The header badge names the step the run is actually in.
    expect(screen.getByText('Splitting web-viz')).toBeInTheDocument();
  });
});

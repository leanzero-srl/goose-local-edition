import { describe, it, expect } from 'vitest';
import {
  deriveFleet,
  resolvePool,
  foldEvents,
  buildActivity,
  cleanTaskTitle,
  foldSupervision,
  DIGEST_FRESH_MS,
  DIGEST_OPEN_CALL_FRESH_MS,
  JUDGE_SPAN_MAX_MS,
} from './useSwarmRun';
import type { TurnLane } from './useSwarmRun';

// Shapes taken verbatim from a measured benchmark run log (swarm-3node-r0, 2026-08-17): a 3-device
// pool where the fleet strip read "FLEET · 2 NODES · 2 WORKING" because the idle third device had no
// lane, and where the whole 19-minute repair wave (complete_fix_*) showed every node "idle — no task".
const POOL = [
  { id: 'mac-gabee-qwen3.6-27b-fable-fusi', model_id: 'gabee-qwen3.6-27b-fable-fusion-711-mtp', weight: 2 },
  { id: 'local-mihai-qwen3.6-27b-fable-fusi', model_id: 'mihai-qwen3.6-27b-fable-fusion-711-mtp', weight: 2 },
  {
    id: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
    model_id: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp',
    weight: 2,
  },
];
const RUN_STARTED = { event: 'run_started', pool: POOL };
const POOL_RESOLVED = { event: 'pool_resolved', devices: POOL, worker_count: 3 };

const lane = (device: string, status: TurnLane['status'], taskId = `t-${device}`): TurnLane => ({
  taskId,
  device,
  status,
  seq: 0,
});

describe('resolvePool — the fleet size comes from the engine, not from who happens to have a task', () => {
  it('reads pool_resolved.devices as canonical node names', () => {
    expect(resolvePool([RUN_STARTED, POOL_RESOLVED])).toEqual(['gabee', 'mihai', 'workhorse']);
  });

  it('falls back to run_started.pool when pool_resolved is absent', () => {
    expect(resolvePool([RUN_STARTED])).toEqual(['gabee', 'mihai', 'workhorse']);
  });

  it('is empty when neither event exists (older log) — the strip then degrades to lane devices', () => {
    expect(resolvePool([{ event: 'plan_loaded' }])).toEqual([]);
  });
});

describe('deriveFleet — every pool node renders; idle is a state, never absence', () => {
  it('renders the idle third node the old lane-only derivation dropped (the FLEET · 2 NODES bug)', () => {
    const { devices, workingByDevice } = deriveFleet({
      pool: ['gabee', 'mihai', 'workhorse'],
      laneSources: [lane('gabee', 'running'), lane('mihai', 'running')],
      digests: {},
      digestMtimes: {},
      now: 1000,
    });
    expect(devices).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(workingByDevice.size).toBe(2);
    expect(workingByDevice.has('workhorse')).toBe(false);
  });

  describe('a running lane is a CLAIM — LM Studio and the digest get to disagree', () => {
    // gabee rendered "Review 1 · working", with a nudge quoted under it, while `lms ps` showed all
    // three nodes IDLE. Its lane had been re-streamed 13 times and the stream was gone; nothing ever
    // closed the lane, so the claim stood forever.
    const deadLane = (over: Partial<Parameters<typeof deriveFleet>[0]> = {}) =>
      deriveFleet({
        pool: ['gabee', 'mihai'],
        laneSources: [lane('gabee', 'running')],
        digests: { 't-gabee': { calls: [{ ok: true }] } },
        digestMtimes: { 't-gabee': 0 },
        now: 10 * 60_000,
        busyNodes: ['mihai'],
        ...over,
      });

    it('drops the claim when LM Studio says the node is not busy AND the digest is stale', () => {
      expect(deadLane().workingByDevice.has('gabee')).toBe(false);
    });

    it('keeps it when LM Studio still reports that node busy — the digest alone may not demote', () => {
      expect(deadLane({ busyNodes: ['gabee', 'mihai'] }).workingByDevice.has('gabee')).toBe(true);
    });

    it('keeps it when the digest is fresh, even though the node is not in busyNodes', () => {
      expect(
        deadLane({ digestMtimes: { 't-gabee': 10 * 60_000 - 1_000 } }).workingByDevice.has('gabee')
      ).toBe(true);
    });

    it('keeps it when nothing is reporting fleet state — a cloud node never appears in lms ps', () => {
      expect(deadLane({ busyNodes: [] }).workingByDevice.has('gabee')).toBe(true);
      expect(deadLane({ busyNodes: undefined }).workingByDevice.has('gabee')).toBe(true);
    });

    it('keeps it while a tool call is open, which legitimately streams nothing for minutes', () => {
      expect(
        deadLane({
          digests: { 't-gabee': { calls: [{ ok: null }] } },
          digestMtimes: { 't-gabee': 10 * 60_000 - 5 * 60_000 },
        }).workingByDevice.has('gabee')
      ).toBe(true);
    });
  });

  it('keeps a lane device the pool missed', () => {
    const { devices } = deriveFleet({
      pool: ['gabee'],
      laneSources: [lane('mihai', 'done')],
      digests: {},
      digestMtimes: {},
      now: 1000,
    });
    expect(devices).toEqual(['gabee', 'mihai']);
  });

  it('counts a node with an open, fresh digest as WORKING even with no lane (the realtime fix)', () => {
    const now = 1_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee', 'mihai', 'workhorse'],
      laneSources: [],
      digests: {
        // mid-stream digest: the engine omits `phase` while tokens flow
        'verify::api': { model: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp', last_text: 'checking…' },
        // seeded at dispatch, before the first token
        'scout-architecture': { model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp', phase: 'processing' },
      },
      digestMtimes: { 'verify::api': now - 2000, 'scout-architecture': now - 500 },
      now,
    });
    expect(workingByDevice.get('workhorse')?.taskId).toBe('verify::api');
    expect(workingByDevice.get('workhorse')?.description).toBe('Verifying api');
    expect(workingByDevice.get('gabee')?.phase).toBe('processing');
    expect(workingByDevice.has('mihai')).toBe(false);
  });

  it("a digest stamped phase:'done' means the call ended — the node reads idle immediately", () => {
    const now = 1_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee'],
      laneSources: [],
      digests: { 'scout-architecture': { model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp', phase: 'done' } },
      digestMtimes: { 'scout-architecture': now - 100 },
      now,
    });
    expect(workingByDevice.size).toBe(0);
  });

  it('a stale open digest (crashed worker, no terminal stamp) does not read as working forever', () => {
    const now = 1_000_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee'],
      laneSources: [],
      digests: { 'scout-architecture': { model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp' } },
      digestMtimes: { 'scout-architecture': now - DIGEST_FRESH_MS - 1 },
      now,
    });
    expect(workingByDevice.size).toBe(0);
  });

  it('engine lifecycle beats digest freshness: a completed task cannot re-mark its node working', () => {
    const now = 1_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee'],
      laneSources: [lane('gabee', 'done', 'api')],
      digests: { api: { model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp' } },
      digestMtimes: { api: now - 100 },
      now,
    });
    expect(workingByDevice.size).toBe(0);
  });
});

describe('digest freshness — an OPEN tool call beats the short mtime window', () => {
  it('an open LANE stays WORKING even when its digest mtime is 3 minutes old', () => {
    const now = 10_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee'],
      laneSources: [lane('gabee', 'running', 'api')],
      digests: { api: { model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp' } },
      digestMtimes: { api: now - 180_000 },
      now,
    });
    expect(workingByDevice.get('gabee')?.taskId).toBe('api');
  });

  it("a LANELESS digest whose last call is pending (ok: null) survives past DIGEST_FRESH_MS — one long shell call streams no tokens", () => {
    const now = 10_000_000;
    const digest = {
      model: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp',
      calls: [
        { name: 'shell', summary: 'cargo build', ok: true },
        { name: 'shell', summary: 'python3 -m pytest -q', ok: null },
      ],
    };
    const { workingByDevice } = deriveFleet({
      pool: ['workhorse'],
      laneSources: [],
      digests: { 'verify::api': digest },
      digestMtimes: { 'verify::api': now - 180_000 }, // 3 min — past the 120s window
      now,
    });
    expect(workingByDevice.get('workhorse')?.taskId).toBe('verify::api');
  });

  it('the open-call grace is not forever — past DIGEST_OPEN_CALL_FRESH_MS it still drops out', () => {
    const now = 100_000_000;
    const digest = {
      model: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp',
      calls: [{ name: 'shell', summary: 'python3 -m pytest -q', ok: null }],
    };
    const { workingByDevice } = deriveFleet({
      pool: ['workhorse'],
      laneSources: [],
      digests: { 'verify::api': digest },
      digestMtimes: { 'verify::api': now - DIGEST_OPEN_CALL_FRESH_MS - 1 },
      now,
    });
    expect(workingByDevice.size).toBe(0);
  });

  it('a stale digest with NO open call still reads idle within the old window (unchanged behavior)', () => {
    const now = 10_000_000;
    const { workingByDevice } = deriveFleet({
      pool: ['gabee'],
      laneSources: [],
      digests: {
        x: {
          model: 'gabee-qwen3.6-27b-fable-fusion-711-mtp',
          calls: [{ name: 'shell', summary: 'ls', ok: true }],
        },
      },
      digestMtimes: { x: now - DIGEST_FRESH_MS - 1 },
      now,
    });
    expect(workingByDevice.size).toBe(0);
  });
});

// Shapes VERBATIM from the live incident (swarm-3node-r0, 2026-08-17): workhorse showed "idle — no task"
// while LM Studio had it processing 2 requests. The log tail at that moment — the node's real work was
// SUPERVISION: a judge generation (judge_observed with no verdict yet) that creates no task lane.
const SUPERVISION_TAIL = [
  {
    event: 'task_completed',
    task_id: 'verify::web',
    status: 'done',
    device: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
    ts: '2026-08-17T16:36:11.000000+00:00',
  },
  {
    event: 'pre_review',
    task_id: 'web-js',
    device: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp', // model-id spelling — the OTHER device spelling
    had_findings: false,
    secs: 124.0,
    ts: '2026-08-17T16:38:33.000000+00:00',
  },
  {
    event: 'judge_verdict',
    task_id: 'verify::web',
    device: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
    judge_node: 'gabee-qwen3.6-27b-fable-fusion-711-mtp',
    verdict: 'ok',
    action: 'observed',
    ts: '2026-08-17T16:39:06.000000+00:00',
  },
  {
    event: 'judge_observed',
    task_id: 'verify::meridian',
    elapsed_secs: 90,
    tool_calls: 6,
    ts: '2026-08-17T16:39:07.000000+00:00',
  },
];

describe('supervision — judge generations count as WORKING (the "idle while LM Studio shows 2 requests" bug)', () => {
  const NOW = Date.parse('2026-08-17T16:39:40.000000+00:00');

  it('foldSupervision: an unmatched judge_observed is an open span; verdict/skip/completion closes it', () => {
    const open = foldSupervision([RUN_STARTED, POOL_RESOLVED, ...SUPERVISION_TAIL]);
    expect(open).toHaveLength(1);
    expect(open[0].taskId).toBe('verify::meridian');
    expect(open[0].label).toBe('Judging · verify::meridian');
    const closed = foldSupervision([
      ...SUPERVISION_TAIL,
      { event: 'judge_verdict', task_id: 'verify::meridian', verdict: 'ok', action: 'observed' },
    ]);
    expect(closed).toHaveLength(0);
    const skipped = foldSupervision([
      ...SUPERVISION_TAIL,
      { event: 'judge_skipped', task_id: 'verify::meridian', reason: 'no_idle_device' },
    ]);
    expect(skipped).toHaveLength(0);
    // A verdict on finished work never arrives — the task's own completion closes the span too.
    const done = foldSupervision([
      ...SUPERVISION_TAIL,
      { event: 'task_completed', task_id: 'verify::meridian', status: 'done' },
    ]);
    expect(done).toHaveLength(0);
  });

  it('OLD derivation read the supervising node as idle; with the busy join it is WORKING and says why', () => {
    const supervision = foldSupervision([RUN_STARTED, POOL_RESOLVED, ...SUPERVISION_TAIL]);
    // verify::meridian's own worker is busy on mihai; workhorse is the LM-Studio-busy node with no lane.
    const laneSources = [lane('mihai', 'running', 'verify::meridian')];
    const before = deriveFleet({
      pool: ['gabee', 'mihai', 'workhorse'],
      laneSources,
      digests: {},
      digestMtimes: {},
      now: NOW,
    });
    expect(before.workingByDevice.has('workhorse')).toBe(false); // the measured lie
    const after = deriveFleet({
      pool: ['gabee', 'mihai', 'workhorse'],
      laneSources,
      digests: {},
      digestMtimes: {},
      now: NOW,
      supervision,
      busyNodes: ['workhorse', 'mihai'],
    });
    const w = after.workingByDevice.get('workhorse');
    expect(w?.description).toBe('Judging · verify::meridian');
    expect(w?.phase).toBe('supervision');
    expect(after.unattributed).toHaveLength(0);
  });

  it('with no busy node to pin it to, the span is returned unattributed — real work is never dropped', () => {
    const supervision = foldSupervision([RUN_STARTED, POOL_RESOLVED, ...SUPERVISION_TAIL]);
    const { workingByDevice, unattributed } = deriveFleet({
      pool: ['gabee', 'mihai', 'workhorse'],
      laneSources: [],
      digests: {},
      digestMtimes: {},
      now: NOW,
      supervision,
    });
    expect(workingByDevice.size).toBe(0);
    expect(unattributed).toHaveLength(1);
    expect(unattributed[0].label).toBe('Judging · verify::meridian');
  });

  it('a span older than JUDGE_SPAN_MAX_MS is a crashed run leftover, not live work', () => {
    const supervision = foldSupervision([...SUPERVISION_TAIL]);
    const { workingByDevice, unattributed } = deriveFleet({
      pool: ['workhorse'],
      laneSources: [],
      digests: {},
      digestMtimes: {},
      now: NOW + JUDGE_SPAN_MAX_MS + 1,
      supervision,
      busyNodes: ['workhorse'],
    });
    expect(workingByDevice.size).toBe(0);
    expect(unattributed).toHaveLength(0);
  });
});

describe('foldEvents — the repair wave has lanes (the 19-minute "all idle" gap)', () => {
  const dispatched = (twin: number, model: string) => ({
    event: 'complete_fix_dispatched',
    round: 0,
    twin,
    model,
    task_id: `complete-fix::twin${twin}`,
  });

  it('an open fix twin is a running lane on its canonical node', () => {
    const { fixLanes } = foldEvents(
      [RUN_STARTED, POOL_RESOLVED, dispatched(0, 'mihai-qwen3.6-27b-fable-fusion-711-mtp')],
      {}
    );
    expect(fixLanes).toHaveLength(1);
    expect(fixLanes[0].device).toBe('mihai');
    expect(fixLanes[0].status).toBe('running');
  });

  it("a completed event WITHOUT task_id (the race arm's real shape) still closes its twin", () => {
    // Run 9, round 1, verbatim shape: the race arm emitted twin/model and NO task_id, and the
    // panel showed two idle nodes as "Repairing…" for 10+ minutes. The fallback reconstructs
    // the id from the twin index.
    const { fixLanes } = foldEvents(
      [
        RUN_STARTED,
        dispatched(0, 'mihai-qwen3.6-27b-fable-fusion-711-mtp'),
        dispatched(2, 'workhorse-qwen3.6-27b-fable-fusion-711-mtp'),
        {
          event: 'complete_fix_completed',
          round: 1,
          twin: 2,
          model: 'workhorse-qwen3.6-27b-fable-fusion-711-mtp',
          verified_findings: 1,
          baseline_findings: 1,
        },
      ],
      {}
    );
    const byId = Object.fromEntries(fixLanes.map((l) => [l.taskId, l.status]));
    expect(byId['complete-fix::twin2']).toBe('done');
    expect(byId['complete-fix::twin0']).toBe('running');
  });

  it('complete_fix_completed closes exactly that twin', () => {
    const { fixLanes } = foldEvents(
      [
        RUN_STARTED,
        dispatched(0, 'mihai-qwen3.6-27b-fable-fusion-711-mtp'),
        dispatched(1, 'workhorse-qwen3.6-27b-fable-fusion-711-mtp'),
        { event: 'complete_fix_completed', round: 0, twin: 0, task_id: 'complete-fix::twin0' },
      ],
      {}
    );
    const byId = Object.fromEntries(fixLanes.map((l) => [l.taskId, l.status]));
    expect(byId['complete-fix::twin0']).toBe('done');
    expect(byId['complete-fix::twin1']).toBe('running');
  });

  it('spec_repair_wave closes any straggler twin (early close loses no node to a forever-spinner)', () => {
    const { fixLanes } = foldEvents(
      [
        RUN_STARTED,
        dispatched(0, 'mihai-qwen3.6-27b-fable-fusion-711-mtp'),
        { event: 'spec_repair_wave', round: 0, early_close: true },
      ],
      {}
    );
    expect(fixLanes[0].status).toBe('done');
  });
});

describe('event log — an action must be distinguishable from an observation at a glance', () => {
  const look = (task: string, established: string, next: string) => ({
    event: 'judge_look',
    task_id: task,
    verdict: 'looping',
    established,
    next,
  });
  const nudge = (task: string, established: string, next: string) => ({
    event: 'judge_nudge',
    task_id: task,
    delivery: 'restream',
    established,
    next,
  });

  // MEASURED from a real run: the engine emits judge_look then judge_nudge in the same breath carrying
  // identical established/next, so the log rendered every action twice and the three rows that changed
  // the run were lost among fifteen that only watched it.
  it('folds a nudge into the look it repeats, leaving ONE row marked as an action', () => {
    const { verbose: items } = buildActivity([
      look('open-coverage-1', 'Identified components', 'Build the coverage table'),
      nudge('open-coverage-1', 'Identified components', 'Build the coverage table'),
    ]);
    const judged = items.filter((i) => i.kind === 'judge' || i.kind === 'judge-act');
    expect(judged).toHaveLength(1);
    expect(judged[0].kind).toBe('judge-act');
    expect(judged[0].text).toContain('steered');
  });

  it('keeps a look that led to no action as an observation', () => {
    const { verbose: items } = buildActivity([look('open-coverage-2', 'Enumerated parts', 'Keep going')]);
    const judged = items.filter((i) => i.kind === 'judge' || i.kind === 'judge-act');
    expect(judged).toHaveLength(1);
    expect(judged[0].kind).toBe('judge');
    expect(judged[0].text).toContain('looked at');
  });

  it('does NOT fold a nudge whose direction differs from the look before it', () => {
    const { verbose: items } = buildActivity([
      look('open-coverage-1', 'Identified components', 'Build the coverage table'),
      nudge('open-coverage-1', 'Identified components', 'Something genuinely different'),
    ]);
    expect(items.filter((i) => i.kind === 'judge' || i.kind === 'judge-act')).toHaveLength(2);
  });

  it('does NOT fold across different tasks', () => {
    const { verbose: items } = buildActivity([
      look('open-coverage-1', 'A', 'B'),
      nudge('open-coverage-2', 'A', 'B'),
    ]);
    expect(items.filter((i) => i.kind === 'judge' || i.kind === 'judge-act')).toHaveLength(2);
  });
});

describe('a task title must name the work, never a heading from its brief', () => {
  // MEASURED: the fleet strip showed "Answers to slice questions" on two nodes at once while they were
  // building vendor-sync-engine and frontend-table-interactions. A task's description IS its research
  // brief now, so its first heading is not a title.
  it('falls back to the id when the description opens with a markdown heading', () => {
    expect(cleanTaskTitle('## Answers to Slice Questions\n\nThe module...', 'vendor-sync-engine')).toBe(
      'Vendor sync engine'
    );
    expect(cleanTaskTitle('# Questions Answered\n\nblah', 'frontend-css-styling')).toBe(
      'Frontend css styling'
    );
  });

  it('two different tasks can never render the same label', () => {
    const a = cleanTaskTitle('## Answers to slice questions\n\nx', 'frontend-drafts-panel');
    const b = cleanTaskTitle('## Answers to slice questions\n\ny', 'frontend-table-interactions');
    expect(a).not.toBe(b);
  });

  it('still prefers a real prose description', () => {
    expect(
      cleanTaskTitle('Build the notifications feed UI component that displays outbox events', 'x')
    ).toContain('notifications feed');
  });
});

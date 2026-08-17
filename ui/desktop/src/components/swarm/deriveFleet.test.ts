import { describe, it, expect } from 'vitest';
import { deriveFleet, resolvePool, foldEvents, DIGEST_FRESH_MS } from './useSwarmRun';
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

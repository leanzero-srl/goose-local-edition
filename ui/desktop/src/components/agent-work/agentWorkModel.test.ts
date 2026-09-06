import { describe, expect, it } from 'vitest';
import {
  classifyKey,
  countdown,
  digestFields,
  foldDesk,
  liveness,
  manifestYaml,
  scheduleLine,
  type AgentWorkRead,
  type DeskState,
} from './agentWorkModel';

const NOW = Date.parse('2026-09-07T08:00:00Z');

function state(over: Partial<DeskState> = {}): DeskState {
  return {
    status: 'ticking',
    agent: 'axpo',
    title: 'Axpo desk',
    pid: 4242,
    run_id: 'agent-axpo-1',
    tick: 3,
    phase: 'lanes',
    phase_started_at: new Date(NOW - 65_000).toISOString(),
    next_tick_at: null,
    next_tick_reason: '',
    next_tick_local: '',
    cadence: '30m',
    timezone: 'Europe/Zurich',
    window_open: true,
    last_tick: null,
    lanes_planned: 2,
    lanes_done: 0,
    hold_reason: null,
    decisions_applied: 0,
    planner_model: 'big-27b',
    devices: [
      { id: 'a', model_id: 'small-9b', weight: 2, supervision: false },
      { id: 's', model_id: 'big-27b', weight: 1, supervision: true },
    ],
    updated_at: new Date(NOW).toISOString(),
    ...over,
  };
}

function read(over: Partial<AgentWorkRead> = {}): AgentWorkRead {
  return {
    dir: '/desks/axpo',
    manifest: { name: 'axpo', cadence: '30m', timezone: 'Europe/Zurich', window: { days: ['mon', 'fri'], from: '09:00', to: '18:00' } },
    state: state(),
    pid: 4242,
    heartbeatMs: NOW - 2_000,
    events: [
      { event: 'lane_queued', tick: 3, key: 't3-ithub-1', lane: 'ithub-1' },
      { event: 'lane_dispatched', tick: 3, key: 't3-ithub-1', lane: 'ithub-1', surgeon: 'access', item: 'ITHUB-1', model: 'small-9b', ts: new Date(NOW - 40_000).toISOString() },
      { event: 'lane_queued', tick: 3, key: 't3-ithub-2', lane: 'ithub-2' },
      { event: 'lane_done', tick: 2, key: 't2-old', lane: 'old', secs: 10 },
    ],
    ticks: [{ tick: 3, orient: { summary: 's', lanes: [{ id: 'ithub-1', surgeon: 'access', item: 'ITHUB-1', objective: 'find who owns it', kind: 'research' }], asks: [], drop: [] } }],
    lanes: {
      't3-orient': { model: 'big-27b', last_text: 'planned two lanes', said_at: 'x' },
      't3-ithub-1': { model: 'small-9b', last_thinking: 'reading the ticket\nchecking groups', tool_calls: 3, forming: [] },
    },
    laneMtimes: { 't3-orient': NOW - 30_000, 't3-ithub-1': NOW - 1_000 },
    prepared: [
      { id: 'd1', tick: 2, lane: 'x', surgeon: 'access', target: 'ITHUB-9', kind: 'comment', body: 'hi', evidence: [], status: 'staged', staged_at: 'x' },
      { id: 'd0', tick: 1, lane: 'y', surgeon: 'access', target: 'ITHUB-8', kind: 'comment', body: 'yo', evidence: [], status: 'posted', staged_at: 'x' },
    ],
    asks: [
      { id: 'a1', tick: 2, question: 'which group?', why: 'two match', status: 'open', raised_at: 'x' },
      { id: 'a0', tick: 1, question: 'old?', why: '', status: 'answered', raised_at: 'x', answer: 'yes' },
    ],
    ledger: {
      kinds: {
        tick: [{ tick: 1, lanes: 2, staged: 1, posted: 0, lane_secs: 120 }, { tick: 2, lanes: 1, staged: 1, posted: 1, lane_secs: 60 }],
        fact: [{ tick: 1, fact: 'A' }, { tick: 2, fact: 'B' }],
        ask: [{ tick: 2 }],
      },
    },
    scratchpad: '',
    pending: '',
    dailyLog: '',
    engineLog: '',
    now: NOW,
    ...over,
  };
}

describe('agentWorkModel', () => {
  it('classifies the engine lane keys', () => {
    expect(classifyKey('t3-orient')).toEqual({ tick: 3, kind: 'orient', laneId: 'orient' });
    expect(classifyKey('t3-synthesis')).toEqual({ tick: 3, kind: 'synthesis', laneId: 'synthesis' });
    expect(classifyKey('t3-ithub-4821-access')).toEqual({ tick: 3, kind: 'lane', laneId: 'ithub-4821-access' });
    expect(classifyKey('t3-ithub-4821-access-lens-voice')).toEqual({ tick: 3, kind: 'lens', laneId: 'ithub-4821-access', lens: 'voice' });
    expect(classifyKey('open-coverage-1')).toBeNull();
  });

  it('liveness needs a live pid AND a fresh heartbeat', () => {
    expect(liveness(null, NOW, NOW)).toBe('stopped');
    expect(liveness(1, NOW - 1_000, NOW)).toBe('running');
    expect(liveness(1, NOW - 60_000, NOW)).toBe('stale');
    expect(liveness(1, null, NOW)).toBe('stale');
  });

  it('the digest join prefers forming, then a said answer, then thinking', () => {
    expect(digestFields({ forming: [{ id: '1', name: 'shell', since_ms: 1, args_preview: 'ls' }], last_text: 'a' }, NOW, NOW).liveLine).toBe('forming shell — ls');
    expect(digestFields({ last_text: 'answer line', said_at: 't', last_thinking: 'thinking' }, NOW, NOW).liveLine).toBe('answer line');
    expect(digestFields({ last_thinking: 'one\ntwo' }, NOW, NOW).liveLine).toBe('two');
    expect(digestFields({ phase: 'processing' }, NOW - 5000, NOW)).toMatchObject({ liveLine: 'processing the prompt…', digestAgeMs: 5000 });
  });

  it('folds the current tick: lanes from events + digests, queue, node occupancy, needs-you, totals', () => {
    const m = foldDesk(read(), NOW)!;
    expect(m.liveness).toBe('running');
    expect(m.tick).toBe(3);
    expect(m.phase).toBe('lanes');
    expect(m.phaseElapsedMs).toBe(65_000);
    const keys = m.lanes.map((l) => `${l.kind}:${l.key}:${l.status}`);
    expect(keys).toEqual(['orient:t3-orient:done', 'lane:t3-ithub-1:running', 'lane:t3-ithub-2:queued']);
    const lane = m.lanes.find((l) => l.key === 't3-ithub-1')!;
    expect(lane.surgeon).toBe('access');
    expect(lane.item).toBe('ITHUB-1');
    expect(lane.objective).toBe('find who owns it');
    expect(lane.liveLine).toBe('checking groups');
    expect(lane.toolCalls).toBe(3);
    expect(m.queue.map((l) => l.key)).toEqual(['t3-ithub-2']);
    expect(m.nodes.find((n) => n.model_id === 'small-9b')).toMatchObject({ free: 1, running: [expect.objectContaining({ key: 't3-ithub-1' })] });
    expect(m.openAsks.map((a) => a.id)).toEqual(['a1']);
    expect(m.pendingDrafts.map((d) => d.id)).toEqual(['d1']);
    expect(m.settledDrafts.map((d) => d.id)).toEqual(['d0']);
    expect(m.totals).toEqual({ ticks: 2, lanes: 3, staged: 2, posted: 1, asks: 1, laneMinutes: 3 });
    expect(m.facts.map((f) => f.fact)).toEqual(['B', 'A']);
  });

  it('a stopped process shows idle with no countdown, and a lane never reads as running', () => {
    const m = foldDesk(read({ pid: null, state: state({ status: 'waiting', next_tick_at: new Date(NOW + 60_000).toISOString() }) }), NOW)!;
    expect(m.liveness).toBe('stopped');
    expect(m.status).toBe('stopped');
    expect(m.phase).toBe('idle');
    expect(m.nextTickInMs).toBeNull();
    expect(m.nodes.every((n) => n.running.length === 0)).toBe(true);
  });

  it('the clock counts down to the next tick', () => {
    const m = foldDesk(read({ state: state({ status: 'waiting', phase: 'idle', next_tick_at: new Date(NOW + 754_000).toISOString(), next_tick_reason: 'cadence' }) }), NOW)!;
    expect(m.nextTickInMs).toBe(754_000);
    expect(countdown(m.nextTickInMs)).toBe('in 12m 34s');
    expect(countdown(-90_000)).toBe('overdue 1m 30s');
    expect(countdown(0)).toBe('now');
  });

  it('schedule line and manifest yaml', () => {
    expect(scheduleLine({ cadence: '30m', timezone: 'Europe/Zurich', window: { days: ['mon', 'tue', 'fri'], from: '09:00', to: '18:00' } })).toBe('every 30m · Mon–Fri 09:00–18:00 Europe/Zurich');
    const y = manifestYaml({
      name: 'axpo', title: 'Axpo desk', timezone: 'Europe/Zurich', from: '09:00', to: '18:00', days: ['mon', 'fri'], always: false,
      cadence: '30m', envFile: 'references/credentials.env', poll: ['python3 scripts/aj.py checkin'], guard: [],
      surgeons: [{ name: 'access', brief: 'permissions', readOnly: true }], lenses: ['factual', 'voice'],
      postCommand: 'python3 scripts/post.py --prepared {id}', approval: 'human', commit: true,
    });
    expect(y).toContain('name: axpo');
    expect(y).toContain('env_file: "references/credentials.env"');
    expect(y).toContain('  - name: access');
    expect(y).toContain('  command: "python3 scripts/post.py --prepared {id}"');
    expect(y).toContain('  approval: human');
  });
});

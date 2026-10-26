import { describe, expect, it } from 'vitest';
import {
  BAR_FILL,
  COUNT_SCALE,
  JOBS_DEFAULTS,
  inferenceCount,
  isTerminal,
  liveJobs,
  loadBars,
  ownParts,
  parseBool,
  parseNumber,
  parsePolicy,
  USER_ACTIVE_POLICIES,
  parseStrategy,
  STRATEGIES,
  waitingJobs,
} from './macLoad';
import { brief, busyLoad } from './macLoad.fixtures';

const pctOf = (load: ReturnType<typeof busyLoad>) =>
  Object.fromEntries(loadBars(load).map((b) => [b.id, b.pct]));

describe('macLoad: the three bars', () => {
  it('reads own/inference/jobs in that order for a busy Mac', () => {
    const bars = loadBars(busyLoad());
    expect(bars.map((b) => b.id)).toEqual(['own', 'inference', 'jobs']);
    // own = worst of cpu 35, memory used 62, gpu 48
    expect(pctOf(busyLoad())).toEqual({ own: 62, inference: 75, jobs: 50 });
  });

  it('own is the busiest resource, whichever it is', () => {
    const l = busyLoad();
    expect(pctOf({ ...l, own: { ...l.own, gpu_util_pct: 91 } }).own).toBe(91);
    expect(pctOf({ ...l, own: { ...l.own, cpu_load_per_core: 1.7 } }).own).toBe(100);
  });

  it('unknown readings count as 0 and never NaN', () => {
    const l = busyLoad();
    const blank = {
      ...l,
      own: { ...l.own, cpu_load_per_core: null, mem_free_pct: null, gpu_util_pct: null },
    };
    expect(pctOf(blank).own).toBe(0);
    expect(ownParts(blank)).toMatchObject({ cpuPct: null, memUsedPct: null, gpuPct: null });
  });

  it('clamps out-of-range readings into 0..100', () => {
    const l = busyLoad();
    expect(ownParts({ ...l, own: { ...l.own, mem_free_pct: 130 } }).memUsedPct).toBe(0);
    expect(ownParts({ ...l, own: { ...l.own, gpu_util_pct: -5 } }).gpuPct).toBe(0);
    expect(ownParts({ ...l, own: { ...l.own, gpu_util_pct: Number.NaN } }).gpuPct).toBe(0);
  });

  it('count bars fill against COUNT_SCALE and cap at full', () => {
    const l = busyLoad({ inference: { engine_active: 9, router_slots_used: 3 } });
    expect(inferenceCount(l)).toBe(12);
    expect(pctOf(l).inference).toBe(100);
    expect(
      pctOf(busyLoad({ inference: { engine_active: 1, router_slots_used: 0 } })).inference
    ).toBe(100 / COUNT_SCALE);
  });

  it('ended jobs are listed but do not fill the jobs bar', () => {
    const jobs = [
      brief('a', 'done'),
      brief('b', 'failed'),
      brief('c', 'killed'),
      brief('d', 'cancelled'),
    ];
    expect(liveJobs(jobs)).toEqual([]);
    expect(pctOf(busyLoad({ jobs })).jobs).toBe(0);
    expect(liveJobs(busyLoad().jobs).map((j) => j.job_id)).toEqual(['j-run', 'j-wait']);
    expect(isTerminal('queued')).toBe(false);
    expect(isTerminal('running')).toBe(false);
    expect(isTerminal('cancelled')).toBe(true);
  });

  it('carries the idle seconds rounded and the user flag', () => {
    const l = busyLoad();
    expect(ownParts({ ...l, own: { ...l.own, user_idle_s: 12.6 } })).toMatchObject({
      idleS: 13,
      userActive: false,
    });
  });

  it('uses three distinct solid fills', () => {
    const fills = Object.values(BAR_FILL);
    expect(new Set(fills).size).toBe(3);
    for (const f of fills) expect(f).toMatch(/^bg-\[#[0-9A-F]{6}\]$/);
  });
});

describe('macLoad: config parsers', () => {
  it('parsePolicy accepts allow and refuse, reads the old last_resort as allow, else the default', () => {
    expect(USER_ACTIVE_POLICIES).toEqual(['allow', 'refuse']);
    expect(JOBS_DEFAULTS.policy).toBe('allow');
    expect(parsePolicy('allow')).toBe('allow');
    // Busy is measured load only since 3.0.131: someone at the keyboard never ranks a Mac last.
    expect(parsePolicy(' Last-Resort ')).toBe('allow');
    expect(parsePolicy('last_resort')).toBe('allow');
    expect(parsePolicy('REFUSE')).toBe('refuse');
    expect(parsePolicy('maybe')).toBe(JOBS_DEFAULTS.policy);
    expect(parsePolicy(null)).toBe(JOBS_DEFAULTS.policy);
  });

  it('parseNumber reads numbers and numeric strings, null otherwise', () => {
    expect(parseNumber(300)).toBe(300);
    expect(parseNumber('20')).toBe(20);
    expect(parseNumber('  ')).toBeNull();
    expect(parseNumber('x')).toBeNull();
    expect(parseNumber(Number.POSITIVE_INFINITY)).toBeNull();
    expect(parseNumber(undefined)).toBeNull();
  });

  it('parseBool is true only for true / "true"', () => {
    expect(parseBool(true)).toBe(true);
    expect(parseBool(' TRUE ')).toBe(true);
    expect(parseBool('yes')).toBe(false);
    expect(parseBool(1)).toBe(false);
  });
});

describe('macLoad: the origin queue and the placement strategy', () => {
  const waiting = (id: string, position: number) =>
    brief(id, 'queued', {
      queue: {
        position,
        strategy: 'local-first',
        refusals: [{ node_id: 'n-gabee', code: 'memory_headroom', reason: 'no room' }],
      },
    });

  it('a job waiting in the origin queue is not live and does not fill the jobs bar', () => {
    const jobs = [brief('j-run', 'running'), waiting('q-2', 2), waiting('q-1', 1)];
    expect(liveJobs(jobs).map((j) => j.job_id)).toEqual(['j-run']);
    expect(pctOf(busyLoad({ jobs })).jobs).toBe(100 / COUNT_SCALE);
    // First in line first, whatever order the node listed them in.
    expect(waitingJobs(jobs).map((j) => j.job_id)).toEqual(['q-1', 'q-2']);
    // A plain `queued` brief (accepted on that Mac, starting) is live, not waiting.
    expect(waitingJobs([brief('j-q', 'queued')])).toEqual([]);
    expect(liveJobs([brief('j-q', 'queued')]).length).toBe(1);
  });

  it('parses the strategy the way leanzero-link does, local-first by default', () => {
    expect(STRATEGIES).toEqual(['local-first', 'selfish']);
    expect(JOBS_DEFAULTS.strategy).toBe('local-first');
    expect(parseStrategy('selfish')).toBe('selfish');
    expect(parseStrategy(' Local_First ')).toBe('local-first');
    expect(parseStrategy('greedy')).toBe('local-first');
    expect(parseStrategy(null)).toBe('local-first');
  });
});

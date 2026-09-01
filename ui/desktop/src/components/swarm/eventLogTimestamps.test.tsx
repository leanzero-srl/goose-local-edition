import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmRunPanel, { eventClock } from './SwarmRunPanel';
import { buildActivity } from './useSwarmRun';

/**
 * THE EVENT LOG GUTTER SHOWS THE ENGINE'S CLOCK. Every run.jsonl row is stamped by `EventLog::write_line`
 * (swarm.rs) with `ts` = `chrono::Utc::now().to_rfc3339()`; the fold carries it onto the feed line as
 * `ActivityItem.at` and the gutter renders it as a local HH:MM:SS. A row WITHOUT `ts` keeps the ordinal —
 * the time is never invented, and never inherited from the previous row.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const T_START = '2026-08-29T20:00:01.123456789+00:00';
const T_DONE = '2026-08-29T20:12:00+00:00';

// run_started and task_completed carry the engine's stamp; slices_opened and task_dispatched do not.
const MIXED = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: T_START },
  { event: 'phase', phase: 'open' },
  { event: 'slices_opened', count: 2, weights: [3, 2], slices: ['store', 'api'], secs: 41 },
  { event: 'phase', phase: 'build' },
  { event: 'task_dispatched', task_id: 'store', device: POOL[0].id, model: POOL[0].model_id },
  {
    event: 'task_completed',
    task_id: 'store',
    status: 'done',
    device: POOL[0].id,
    attempts: 1,
    elapsed_ms: 1000,
    tool_calls: [],
    ts: T_DONE,
  },
];
const stripTs = (events: Array<Record<string, unknown>>) =>
  events.map(({ ts: _ts, ...rest }) => rest);
const withTs = (events: Array<Record<string, unknown>>) =>
  events.map((e, i) => ({ ...e, ts: e['ts'] ?? `2026-08-29T20:0${i}:00+00:00` }));

describe("the fold carries the row's own ts onto the feed line", () => {
  it('a row with ts yields at; a row without yields no at key at all', () => {
    const { activity, verbose } = buildActivity(MIXED);
    const started = activity.find((a) => a.text === 'Starting the build');
    const done = activity.find((a) => a.text === 'store done');
    const sliced = activity.find((a) => a.text.startsWith('Cut into 2 slices'));
    const dispatched = activity.find((a) => a.text === 'Building store');
    expect(started?.at).toBe(T_START);
    expect(done?.at).toBe(T_DONE);
    // Absent means ABSENT — not undefined-valued, not the previous row's clock.
    expect(sliced && 'at' in sliced).toBe(false);
    expect(dispatched && 'at' in dispatched).toBe(false);
    // The verbose feed is stamped the same way from the same rows.
    for (const v of verbose.filter((a) => a.text.startsWith('Cut into')))
      expect('at' in v).toBe(false);
    expect(verbose.find((a) => a.text === 'store done')?.at).toBe(T_DONE);
  });

  it('a run with no ts anywhere produces no at anywhere', () => {
    const { activity, verbose } = buildActivity(stripTs(MIXED));
    expect(activity.length).toBeGreaterThan(0);
    for (const a of [...activity, ...verbose]) expect('at' in a).toBe(false);
  });

  it('a non-string ts is not a time', () => {
    const { activity } = buildActivity([{ ...MIXED[0], ts: 1756497601 }]);
    expect(activity.length).toBeGreaterThan(0);
    for (const a of activity) expect('at' in a).toBe(false);
  });
});

describe('eventClock', () => {
  const local = (iso: string) => {
    const d = new Date(Date.parse(iso));
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  };
  it("renders chrono's to_rfc3339 forms (nanos, micros, none, Z) as local HH:MM:SS", () => {
    for (const iso of [
      T_START,
      '2026-08-17T13:54:13.000000+00:00',
      '2026-08-29T20:00:01+00:00',
      '2026-08-29T20:00:01Z',
    ]) {
      expect(eventClock(iso)).toBe(local(iso));
      expect(eventClock(iso)).toMatch(/^\d\d:\d\d:\d\d$/);
    }
  });
  it('is null for an absent or unparseable at — the ordinal shows, nothing is invented', () => {
    expect(eventClock(undefined)).toBeNull();
    expect(eventClock('')).toBeNull();
    expect(eventClock('not a time')).toBeNull();
  });
});

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

describe('the EVENT LOG gutter', () => {
  const mockRun = (events: Array<Record<string, unknown>>) => {
    electron().readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-ts',
      dir: '/tmp/build',
      events,
      activity: {},
      activityMtimes: {},
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
  };
  beforeEach(() => {
    const e = electron();
    e.fleetStatus = vi.fn(async () => ({}));
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  const openLog = async (events: Array<Record<string, unknown>>) => {
    mockRun(events);
    const utils = render(<SwarmRunPanel workingDir="/tmp/build" />);
    const toggle = await utils.findByRole('button', { name: /Event log/ });
    if (toggle.getAttribute('aria-expanded') !== 'true') fireEvent.click(toggle);
    const list = await utils.findByRole('list', { name: 'Event log' });
    return { ...utils, list };
  };

  it('shows the engine clock on rows that carried ts, the ordinal on rows that did not, one gutter width', async () => {
    const { list } = await openLog(MIXED);
    const rows = Array.from(list.querySelectorAll('li[data-kind]'));
    const times = Array.from(list.querySelectorAll('time'));
    // Only the two stamped rows' clocks appear (run_started folds to several lines, all at T_START);
    // no other time exists anywhere in the list.
    expect(new Set(times.map((t) => t.getAttribute('datetime')))).toEqual(
      new Set([T_START, T_DONE])
    );
    expect(times.filter((t) => t.getAttribute('datetime') === T_DONE).length).toBe(1);
    for (const t of times) {
      expect(t.textContent).toBe(eventClock(t.getAttribute('datetime') ?? undefined));
      expect(t.className).toContain('tnum');
      expect(t.className).toContain('text-lz-ink-3');
    }
    // The rows without a time (slices_opened, task_dispatched) keep their ordinal, aligned on the
    // clock's width.
    const ordinals = Array.from(list.querySelectorAll('li > div > span[aria-hidden]')).filter((s) =>
      /^\d+$/.test(s.textContent ?? '')
    );
    expect(ordinals.length).toBe(2);
    expect(times.length + ordinals.length).toBe(rows.length);
    for (const el of [...times, ...ordinals]) expect(el.className).toContain('w-16');
  });

  it('a run whose rows all carry ts shows a clock on every row', async () => {
    const { list } = await openLog(withTs(MIXED));
    const rows = Array.from(list.querySelectorAll('li[data-kind]'));
    expect(rows.length).toBeGreaterThan(0);
    expect(list.querySelectorAll('time').length).toBe(rows.length);
  });

  it('a run with no ts shows only ordinals, on the narrow gutter — no time is invented', async () => {
    const { list } = await openLog(stripTs(MIXED));
    expect(list.querySelectorAll('time').length).toBe(0);
    const ordinals = Array.from(list.querySelectorAll('li > div > span[aria-hidden]')).filter((s) =>
      /^\d+$/.test(s.textContent ?? '')
    );
    expect(ordinals.length).toBe(list.querySelectorAll('li[data-kind]').length);
    for (const el of ordinals) expect(el.className).toContain('w-9');
  });
});

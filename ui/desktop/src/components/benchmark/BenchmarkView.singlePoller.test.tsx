import { render, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * The /benchmark route must run ONE poller on the run directory. BenchmarkView already polls it for the
 * phase strip, so it has to hand that state to SwarmRunPanel via `run` — mounting the panel without the
 * prop started a second 500ms poller on the same dir, which is the exact defect the prop was added to
 * prevent (BaseChat has always passed it).
 */

const { panelProps } = vi.hoisted(() => ({
  panelProps: [] as Array<{
    workingDir?: string;
    run?: { present?: boolean; runId?: string | null };
  }>,
}));

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = (props: {
    workingDir?: string;
    run?: { present?: boolean; runId?: string | null };
  }) => {
    panelProps.push(props);
    return React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  };
  return { SwarmRunPanel: Stub, default: Stub };
});

vi.mock('../swarm/useSamplingDefaults', () => ({
  useSaveSamplingDefaults: () => () => {},
}));

import BenchmarkView from './BenchmarkView';

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];

const EVENTS = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`',
    pool: POOL,
    ts: '2026-08-17T13:54:13.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

describe('BenchmarkView — one poller on the run dir', () => {
  beforeEach(() => {
    panelProps.length = 0;
    const e = electron();
    // The re-attach path: main reports a run already in flight, which is what mounts the panel.
    e.benchmarkStatus = vi.fn(async () => ({
      running: true,
      workdir: '/tmp/bench',
      startedAt: '2026-08-17T13:54:13.000Z',
      sampling: {},
    }));
    e.benchmarkRead = vi.fn(async () => null);
    e.benchmarkIdentity = vi.fn(async () => ({ handle: 'mihai' }));
    e.benchmarkShots = vi.fn(async () => []);
    e.readSwarmRun = vi.fn(async (dir: string) => ({
      runId: 'swarm-bench',
      dir,
      events: EVENTS,
      activity: {},
      activityMtimes: {},
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    e.fleetStatus = vi.fn(async () => ({}));
  });

  it('hands SwarmRunPanel the run it already polls, on the same directory', async () => {
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    await waitFor(() => {
      const last = panelProps[panelProps.length - 1];
      expect(last?.run?.present).toBe(true);
    });

    const last = panelProps[panelProps.length - 1];
    expect(last.workingDir).toBe('/tmp/bench');
    expect(last.run?.runId).toBe('swarm-bench');

    // Every panel mount got a run — never the undefined that makes the panel start its own poller.
    expect(panelProps.every((p) => p.run !== undefined)).toBe(true);

    // And the state it was handed is polled from the dir it was handed, so the panel cannot go blank.
    const readSwarmRun = electron().readSwarmRun as ReturnType<typeof vi.fn>;
    for (const call of readSwarmRun.mock.calls) expect(call[0]).toBe('/tmp/bench');
  });
});

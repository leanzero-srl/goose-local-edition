import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useSwarmRun } from './useSwarmRun';
import { shouldAdoptResidentRun, SWARM_HEARTBEAT_STALE_MS } from './swarmRunLiveness';

/**
 * Pass E — the stale swarm board purge. A NEW session opened in a project whose folder still holds a
 * previous run's .swarm state must open CLEAN: the resident attach adopts a run only when it is LIVE
 * (fresh heartbeat) or started under this mount. The motivating case is this very repo: two July run
 * JSONLs with NO heartbeat file rendered a 47-day-old planning board ("1127h ago") into every fresh
 * session. Ungated surfaces (BenchmarkView) keep adopting finished runs unconditionally.
 */

const POOL = [{ id: 'workhorse-mlx', model_id: 'workhorse-qwen3.5-9b-4bit-mlx', weight: 2 }];

const eventsAt = (iso: string) => [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: iso },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
];

type Payload = {
  runId: string;
  heartbeat: number | null;
  heartbeatExited?: boolean;
  events: Array<Record<string, unknown>>;
};

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const mockRun = (p: Payload) => {
  electron().readSwarmRun = vi.fn(async () => ({
    runId: p.runId,
    dir: '/tmp/build',
    events: p.events,
    activity: {},
    activityMtimes: {},
    clarify: null,
    mtime: Date.now(),
    heartbeat: p.heartbeat,
    heartbeatExited: p.heartbeatExited ?? false,
    pauseRequested: false,
  }));
};

function Probe({ gated }: { gated: boolean }) {
  const run = useSwarmRun('/tmp/build', 60, gated ? { residentGate: true } : undefined);
  if (run.loading) return <div data-testid="probe">loading</div>;
  return <div data-testid="probe">{run.present ? `present:${run.runId}` : 'clean'}</div>;
}

beforeEach(() => {
  electron().onSwarmDelta = vi.fn(() => () => {});
});

describe('shouldAdoptResidentRun — the pure rule', () => {
  const now = Date.now();

  it('refuses a heartbeat-less leftover that started before this mount (the July board)', () => {
    expect(
      shouldAdoptResidentRun(
        { heartbeat: null, heartbeatExited: false, startedAt: now - 47 * 24 * 3600_000 },
        now - 1000,
        now
      )
    ).toBe(false);
  });

  it('refuses an exited run and a silent (frozen-heartbeat) run', () => {
    expect(
      shouldAdoptResidentRun(
        { heartbeat: now - 10_000, heartbeatExited: true, startedAt: now - 3600_000 },
        now - 1000,
        now
      )
    ).toBe(false);
    expect(
      shouldAdoptResidentRun(
        {
          heartbeat: now - SWARM_HEARTBEAT_STALE_MS - 1000,
          heartbeatExited: false,
          startedAt: now - 3600_000,
        },
        now - 1000,
        now
      )
    ).toBe(false);
  });

  it('adopts a run whose heartbeat is fresh, wherever it came from', () => {
    expect(
      shouldAdoptResidentRun(
        { heartbeat: now - 5000, heartbeatExited: false, startedAt: now - 3600_000 },
        now - 1000,
        now
      )
    ).toBe(true);
  });

  it('adopts a run started under this mount even before its first heartbeat lands', () => {
    expect(
      shouldAdoptResidentRun(
        { heartbeat: null, heartbeatExited: false, startedAt: now + 200 },
        now,
        now + 300
      )
    ).toBe(true);
  });
});

describe('useSwarmRun residentGate — what a fresh session actually shows', () => {
  it('renders CLEAN over a stale heartbeat-less leftover run', async () => {
    mockRun({
      runId: 'run-swarm-20260713-084849803',
      heartbeat: null,
      events: eventsAt('2026-07-13T08:48:49.000000+00:00'),
    });
    render(<Probe gated />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('clean'));
  });

  it('renders CLEAN over an exited run', async () => {
    mockRun({
      runId: 'exited',
      heartbeat: Date.now() - 20_000,
      heartbeatExited: true,
      events: eventsAt('2026-07-13T08:48:49.000000+00:00'),
    });
    render(<Probe gated />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('clean'));
  });

  it('attaches to a LIVE run (fresh heartbeat) exactly as before', async () => {
    mockRun({
      runId: 'live-run',
      heartbeat: Date.now(),
      events: eventsAt('2026-07-13T08:48:49.000000+00:00'),
    });
    render(<Probe gated />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('present:live-run'));
  });

  it('attaches to a run started under this mount before its heartbeat exists', async () => {
    mockRun({
      runId: 'just-started',
      heartbeat: null,
      events: eventsAt(new Date(Date.now() + 50).toISOString()),
    });
    render(<Probe gated />);
    await waitFor(() =>
      expect(screen.getByTestId('probe')).toHaveTextContent('present:just-started')
    );
  });

  it('ungated surfaces (BenchmarkView) still adopt a stale run unconditionally', async () => {
    mockRun({
      runId: 'archived',
      heartbeat: null,
      events: eventsAt('2026-07-13T08:48:49.000000+00:00'),
    });
    render(<Probe gated={false} />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('present:archived'));
  });
});

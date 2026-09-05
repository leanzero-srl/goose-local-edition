import { render, renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { useSwarmRun } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * ONE POLLER PER RUN. `run` exists so a host that already polls a run directory can hand the panel the
 * state it has, instead of the panel mounting a second 500ms `useSwarmRun` on the same directory —
 * which doubles the IPC and lets the two copies disagree about the phase for a poll at a time. This is
 * the guard that the injected path actually renders AND actually skips the poller: the /benchmark route
 * shipped with both pollers live because nothing failed when the prop was left off.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];

const EVENTS = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`\n\nA small operations tool.',
    pool: POOL,
    ts: '2026-08-17T13:54:13.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

describe('SwarmRunPanel — an injected run replaces the poller, it does not add one', () => {
  beforeEach(() => {
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-injected',
      dir: '/tmp/build',
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
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
  });

  it('renders the run it was handed and issues no read-swarm-run of its own', async () => {
    const { result, unmount } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    const hostRun = result.current;
    unmount();

    const readSwarmRun = electron().readSwarmRun as ReturnType<typeof vi.fn>;
    readSwarmRun.mockClear();

    const { findByText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={hostRun} />
      </IntlTestWrapper>
    );

    await findByText('Swarm run');
    await findByText('vendorsync');
    expect(readSwarmRun).not.toHaveBeenCalled();
  });

  it('still polls for itself when no run is handed in', async () => {
    const readSwarmRun = electron().readSwarmRun as ReturnType<typeof vi.fn>;
    const { findByText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );

    await findByText('Swarm run');
    expect(readSwarmRun).toHaveBeenCalledWith('/tmp/build');
  });
});

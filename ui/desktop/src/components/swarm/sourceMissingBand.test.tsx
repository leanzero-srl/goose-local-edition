import { render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { resetFoldCache, useSwarmRun } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE TWO PANEL-SIDE HOOKS the wave-1 desktop agent left to the panel (U-H2 / U-M5).
 *
 * 1. A run whose files VANISHED (readSwarmRun → null after a run was on screen) sets `sourceMissing`
 *    and nulls the heartbeat. The panel must SAY so — a distinct band over the last state read — and
 *    the elapsed/ETA clock must stop: nothing below can advance, so a counting clock claims a run that
 *    cannot be observed. Before this the flag was rendered by nothing and the clock kept ticking.
 * 2. The dead-lane corroboration feed is TRUTH, not display: it polls LM Studio even when the
 *    'showLmStudioFleet' display setting is off (the default install), so `reportedNodes` can arm
 *    deriveFleet's demotion. Before this the feed was `useFleetStatus(1500, lmStudioVisible)`, which
 *    polled nothing on a default install and left a dead node "working" for as long as the panel stayed open.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const NOW_ISO = new Date().toISOString();
const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: NOW_ISO },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'build' },
  { event: 'task_dispatched', task_id: 'store', model: POOL[0].model_id, device: POOL[0].id, ts: NOW_ISO },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const BAND = /This run's files no longer resolve \(archived or deleted\) — showing the last state read\./;

describe('SwarmRunPanel — a vanished run gets its band and loses its clock', () => {
  beforeEach(() => {
    resetFoldCache();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-vanish',
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
  afterEach(() => {
    delete electron().fleetStatus;
  });

  const hostRun = async () => {
    const { result, unmount } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    const run = result.current;
    unmount();
    return run;
  };

  it('a live run shows the elapsed/ETA clock and no band', async () => {
    const run = await hostRun();
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={run} />
      </IntlTestWrapper>
    );
    await screen.findByText('vendorsync');
    // HeaderMetrics: before the first checklist item completes there is no basis to estimate from.
    expect(screen.getByText('estimating…')).toBeInTheDocument();
    expect(screen.queryByTestId('run-source-missing')).toBeNull();
  });

  it('sourceMissing renders the solid band over the last state, and the clock stops', async () => {
    const run = await hostRun();
    render(
      <IntlTestWrapper>
        <SwarmRunPanel
          workingDir="/tmp/build"
          run={{ ...run, sourceMissing: true, heartbeat: null, heartbeatExited: false }}
        />
      </IntlTestWrapper>
    );
    await screen.findByText('vendorsync');
    const band = screen.getByTestId('run-source-missing');
    expect(band.textContent).toMatch(BAND);
    // The dot says what the colour means.
    expect(band.querySelector('[data-testid="lz-status-dot"]')).toHaveAttribute(
      'aria-label',
      'Run files no longer resolve'
    );
    // The clock is gone: an unobservable run has no elapsed to tick.
    expect(screen.queryByText('estimating…')).toBeNull();
    expect(screen.queryByText(/left$/)).toBeNull();
    // And it is NOT the hard-killed banner: the heartbeat was nulled, so liveness is unknown, not silent.
    expect(screen.queryByText(/No heartbeat/)).toBeNull();
    expect(screen.queryByText(/most likely hard-killed/)).toBeNull();
  });
});

describe('SwarmRunPanel — the dead-lane corroboration feed polls regardless of the fleet display setting', () => {
  beforeEach(() => {
    resetFoldCache();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-feed',
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
    e.fleetStatus = vi.fn(async () => ({ 'mihai-qwen3.6-27b': 'idle' }));
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
    window.localStorage.removeItem('showLmStudioFleet');
  });
  afterEach(() => {
    delete electron().fleetStatus;
  });

  it('asks LM Studio for the fleet on a default install (display off), so reportedNodes can arm the demotion', async () => {
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    await screen.findByText('vendorsync');
    const fleetStatus = electron().fleetStatus as ReturnType<typeof vi.fn>;
    await waitFor(() => expect(fleetStatus).toHaveBeenCalled());
    // The DISPLAY stays gated: with the setting off, no LM Studio dot is drawn on the fleet row.
    const row = await screen.findByTestId('fleet-node');
    expect(row.querySelector('[aria-label^="LM Studio:"]')).toBeNull();
  });
});

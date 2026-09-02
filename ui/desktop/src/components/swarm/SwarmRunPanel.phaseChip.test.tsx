import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * The header's phase chip is a LIVENESS claim: "Building" with a spinner says the engine is
 * working now. It may only render while the heartbeat is fresh — a silent heartbeat drops the
 * chip (the counts say "interrupted"), an EXITED stamp renders the stopped-tone "Stopped" chip,
 * and a project with no run renders no chip at all. Driven through the real panel and the real
 * poller mock with heartbeat facts, like SwarmRunPanel.staleTruth.test.tsx.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const TS = '2026-08-17T13:54:13.000000+00:00';

const BUILDING_EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: TS },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
  {
    event: 'plan_loaded',
    task_count: 1,
    tasks: [{ id: 'store', description: 'Build the store', files: ['store.py'], deps: [] }],
  },
  { event: 'task_dispatched', task_id: 'store', device: POOL[0].id, model: POOL[0].model_id, ts: TS },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const mockRun = (over: { heartbeat?: number | null; heartbeatExited?: boolean } | null) => {
  electron().readSwarmRun = vi.fn(async () =>
    over === null
      ? null
      : {
          runId: 'phase-chip',
          dir: '/tmp/build',
          events: BUILDING_EVENTS,
          activity: {},
          activityMtimes: {},
          clarify: null,
          mtime: Date.now(),
          heartbeat: over.heartbeat === undefined ? Date.now() : over.heartbeat,
          heartbeatExited: over.heartbeatExited ?? false,
          pauseRequested: false,
        }
  );
};

const mount = () =>
  render(
    <IntlTestWrapper>
      <SwarmRunPanel workingDir="/tmp/build" />
    </IntlTestWrapper>
  );

beforeEach(() => {
  const e = electron();
  e.fleetStatus = vi.fn(async () => ({}));
  e.swarmSetPaused = vi.fn(async () => true);
  e.swarmAddNote = vi.fn(async () => true);
  e.writeFile = vi.fn(async () => true);
  e.onSwarmDelta = vi.fn(() => () => {});
});

describe('the header phase chip never claims work on a run whose engine is gone', () => {
  it("a fresh heartbeat mid-build renders the accent 'Building' chip", async () => {
    mockRun({});
    mount();
    const chip = await screen.findByText('Building');
    expect(chip.closest('[data-tone]')?.getAttribute('data-tone')).toBe('accent');
  });

  it("a SILENT heartbeat drops the chip and the counts read 'interrupted' — never a spinning 'Building'", async () => {
    mockRun({ heartbeat: Date.now() - 60_000 });
    mount();
    await screen.findByText(/No heartbeat for/);
    expect(screen.queryByText('Building')).toBeNull();
    expect(screen.getByText(/1 interrupted/)).toBeInTheDocument();
    expect(screen.queryByTestId('run-outcome-chip')).toBeNull();
  });

  it("an EXITED stamp renders the stopped-tone 'Stopped' chip in the chip's place", async () => {
    mockRun({ heartbeat: Date.now() - 10_000, heartbeatExited: true });
    mount();
    const outcome = await screen.findByTestId('run-outcome-chip');
    expect(outcome.textContent).toBe('Stopped');
    expect(outcome.querySelector('[data-tone]')?.getAttribute('data-tone')).toBe('stopped');
    expect(screen.queryByText('Building')).toBeNull();
  });

  it('a project with no run renders no panel and no chip', async () => {
    mockRun(null);
    const { container } = mount();
    await vi.waitFor(() => expect(electron().readSwarmRun).toHaveBeenCalled());
    expect(container.textContent).toBe('');
    expect(screen.queryByText('Building')).toBeNull();
  });
});

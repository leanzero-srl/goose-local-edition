import { render, cleanup, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';

/**
 * Rendered smoke test of the WHOLE panel against measured event shapes — the guard against a redesign
 * that typechecks but is dead on arrival (it has happened: two committed UI changes compiled and never
 * rendered). Asserts the named-zone structure Mihai asked for: one register of zone headers, node NAMES
 * (never the "fusi, fusi, fable" model-id fragments), and the WORK board's three groups.
 */

const POOL = [
  {
    id: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model_id: 'gabee-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
  {
    id: 'local-mihai-qwen3.6-27b-fable-fusi',
    model_id: 'mihai-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
  {
    id: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
    model_id: 'workhorse-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
];

const EVENTS = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`\n\nA small operations tool.',
    pool: POOL,
    ts: '2026-08-17T13:54:13.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 3 },
  {
    event: 'plan_loaded',
    task_count: 3,
    plan_confidence: 88,
    ask_floor: 85,
    tasks: [
      {
        id: 'store',
        description: 'Build the store',
        files: ['store.py'],
        deps: [],
        difficulty: 'medium',
      },
      {
        id: 'api',
        description: 'Build the api',
        files: ['api.py'],
        deps: ['store'],
        difficulty: 'hard',
      },
      {
        id: 'integrate-verify',
        description: 'Sink',
        files: [],
        deps: ['store', 'api'],
        difficulty: 'hard',
      },
    ],
  },
  {
    event: 'task_dispatched',
    task_id: 'store',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model: POOL[0].model_id,
  },
  {
    event: 'task_completed',
    task_id: 'store',
    status: 'done',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    attempts: 1,
    elapsed_ms: 155142,
    tool_calls: [],
  },
  {
    event: 'task_dispatched',
    task_id: 'api',
    device: 'local-mihai-qwen3.6-27b-fable-fusi',
    model: POOL[1].model_id,
  },
];

type ElectronMock = Record<string, unknown>;

describe('SwarmRunPanel — the named-zone view actually renders', () => {
  beforeEach(() => {
    localStorage.removeItem('goose.swarm.logMode');
    localStorage.removeItem('goose.swarm.verboseActivity');
    const electron = (window as unknown as { electron: ElectronMock }).electron;
    electron.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-test',
      dir: '/tmp/build',
      events: EVENTS,
      activity: {},
      activityMtimes: {},
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      pauseRequested: false,
    }));
    electron.fleetStatus = vi.fn(async () => ({}));
    electron.swarmSetPaused = vi.fn(async () => true);
    electron.swarmAddNote = vi.fn(async () => true);
    electron.revealInFinder = vi.fn(async () => undefined);
    electron.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('mounts every zone in the one header register, with node NAMES and the three WORK groups', async () => {
    const { findByText, findByRole, getAllByRole, queryByText, getAllByText, container } = render(
      <SwarmRunPanel workingDir="/tmp/build" />
    );

    // RUN HEADER zone: the register label + the app's identity from the brief's heading.
    await findByText('Swarm run');
    await findByText('vendorsync');

    // The named zones, each present exactly as a labeled band.
    await findByText('Planning');
    await findByText('Fleet');
    await findByText('Work');
    await findByText('Event log');

    const detailModes = await findByRole('radiogroup', { name: 'Run detail' });
    expect(detailModes).toBeInTheDocument();
    const choices = getAllByRole('radio');
    expect(choices.map((choice) => choice.textContent)).toEqual([
      'Compact',
      'Verbose',
      'Developer',
    ]);
    expect(choices[1]).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(choices[0]);
    expect(choices[0]).toHaveAttribute('aria-checked', 'true');

    // FLEET: canonical node names — never the truncated model-id fragments Mihai saw ("fusi, fusi, fable").
    await findByText('gabee');
    await findByText('mihai');
    await findByText('workhorse');
    expect(container.textContent).not.toContain('fusi,');
    expect(queryByText('fusi')).toBeNull();

    // WORK board: running + queued + done groups, with the sink named for what it is.
    // ('Done' also names a pipeline step in the header breadcrumb, so assert at-least-one.)
    await findByText('Running');
    await findByText('Queued');
    expect(getAllByText('Done').length).toBeGreaterThanOrEqual(1);
    await findByText('Integrate & verify');

    // The mislabel is gone: no build work sits under a "Drafting the plan" header.
    expect(container.textContent).not.toContain('Drafting the plan ·');
  });
});

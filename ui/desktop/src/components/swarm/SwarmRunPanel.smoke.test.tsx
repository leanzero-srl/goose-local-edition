import { render, cleanup } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

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
  { event: 'run_started', prompt: '# Build `vendorsync`\n\nA small operations tool.', pool: POOL, ts: '2026-08-17T13:54:13.000000+00:00' },
  { event: 'pool_resolved', devices: POOL, worker_count: 3 },
  // The rewritten planning flow, verbatim: OPEN -> ASK -> RESEARCH -> SYNTHESIS -> REVIEW.
  { event: 'phase', phase: 'open' },
  { event: 'slices_opened', count: 2, weights: [3, 2], slices: ['store', 'api'], secs: 41 },
  { event: 'phase', phase: 'ask' },
  { event: 'clarify_proxy_armed', mode: 'after_wait', wait_secs: 300, questions: 1 },
  {
    event: 'clarify_proxy_answered',
    questions: ['which storage backend'],
    answers: ['sqlite'],
    source: 'proxy',
  },
  { event: 'phase', phase: 'research' },
  { event: 'research_completed', slices: 2, brief_chars: [4200, 3100], secs: 260 },
  { event: 'phase', phase: 'synthesis' },
  { event: 'phase', phase: 'review' },
  { event: 'review_findings', round: 1, new: 1, repeated: 0, findings: ['no export command'], patch_touches: 1 },
  { event: 'plan_patched', round: 1, replace: 1, add: 0, remove: 0 },
  { event: 'review_findings', round: 2, new: 1, repeated: 0, findings: ['plan is sound'], patch_touches: 0 },
  {
    event: 'plan_loaded',
    task_count: 3,
    plan_confidence: 88,
    ask_floor: 85,
    tasks: [
      { id: 'store', description: 'Build the store', files: ['store.py'], deps: [], difficulty: 'medium' },
      { id: 'api', description: 'Build the api', files: ['api.py'], deps: ['store'], difficulty: 'hard' },
      { id: 'integrate-verify', description: 'Sink', files: [], deps: ['store', 'api'], difficulty: 'hard' },
    ],
  },
  { event: 'task_dispatched', task_id: 'store', device: 'mac-gabee-qwen3.6-27b-fable-fusi', model: POOL[0].model_id },
  {
    event: 'task_completed',
    task_id: 'store',
    status: 'done',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    attempts: 1,
    elapsed_ms: 155142,
    tool_calls: [],
  },
  { event: 'task_dispatched', task_id: 'api', device: 'local-mihai-qwen3.6-27b-fable-fusi', model: POOL[1].model_id },
  // A GREEN run that still shipped imperfect — the case the panel used to render as flawless.
  { event: 'defects_rated', round: 1, critical: 0, minor: 2, engine_forced: 0, minors: ['--json flag is undocumented', 'store rejects an empty ledger'] },
];

// Mid-RESEARCH: the whole fleet is writing slice specs and nothing has been dispatched yet. This is the
// window the panel used to render as three idle nodes behind an empty lane list.
const RESEARCHING = EVENTS.slice(0, EVENTS.findIndex((e) => e.event === 'phase' && e.phase === 'synthesis'));

type ElectronMock = Record<string, unknown>;

describe('SwarmRunPanel — the named-zone view actually renders', () => {
  const mockRun = (events: Array<Record<string, unknown>>) => {
    const electron = (window as unknown as { electron: ElectronMock }).electron;
    electron.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-test',
      dir: '/tmp/build',
      events,
      activity: {
        'slice-store': { model: POOL[0].model_id, last_text: 'the store owns the ledger file' },
        'slice-api': { model: POOL[1].model_id, last_text: 'the api exposes add and list' },
        synthesis: { model: POOL[2].model_id, last_text: 'wiring the dag' },
      },
      activityMtimes: { 'slice-store': Date.now(), 'slice-api': Date.now(), synthesis: Date.now() },
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
  };

  beforeEach(() => {
    const electron = (window as unknown as { electron: ElectronMock }).electron;
    mockRun(EVENTS);
    electron.fleetStatus = vi.fn(async () => ({}));
    electron.swarmSetPaused = vi.fn(async () => true);
    electron.swarmAddNote = vi.fn(async () => true);
    electron.revealInFinder = vi.fn(async () => undefined);
    electron.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('mounts every zone in the one header register, with node NAMES and the three WORK groups', async () => {
    const { findByText, queryByText, getAllByText, container } = render(
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

  it('renders the rewritten pipeline: the ribbon phase, the slice fan, the proxy answer, and the known bugs', async () => {
    const { findByText, findByTestId, container } = render(<SwarmRunPanel workingDir="/tmp/build" />);

    // The RIBBON draws the engine's eight phases and lights the one the engine is actually in — read from
    // the events, never from a label. The newest lifecycle event here is defects_rated, so it is Repair.
    const ribbon = await findByTestId('formation-ribbon');
    expect(ribbon).toHaveAttribute('data-active-phase', 'repair');
    for (const step of ['Open', 'Research', 'Synthesize', 'Review', 'Build', 'Integrate', 'Repair']) {
      expect(ribbon.textContent).toContain(step);
    }
    // The stages this engine no longer runs must not appear as ribbon steps.
    expect(ribbon.textContent).not.toContain('Contracts');

    // KNOWN ACTIVE BUGS: green does not mean flawless, and the imperfections have their own surface.
    await findByText('Known active bugs');
    await findByText('--json flag is undocumented');
    expect(container.textContent).toContain('the run passed — these are what it passed WITH');
  });

  it('shows the slice fan and who answered the open decisions while the run is still planning', async () => {
    mockRun(RESEARCHING);
    const { findAllByText, findByText, findByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );

    expect(await findByTestId('formation-ribbon')).toHaveAttribute('data-active-phase', 'research');

    // The slice fan has lanes, so RESEARCH is not an empty list while three nodes generate.
    await findByText('Slice · store');
    await findByText('Slice · api');

    // WHO answered — a run that answered its own questions must never read like a steered one. It lands
    // in both the clarify surface and the event log, and both are the point.
    expect(await findAllByText('Answered by goose — you did not reply')).not.toHaveLength(0);
    await findByText('Request cut into 2 slices');
  });
});

import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { assertStudioClean } from '../lz/assertStudioClean';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE OWNER'S CASE, RENDERED (Mihai, 2026-09-02): a build-phase task carries a 400-char brief, the WORK
 * row shows 90 characters of it — clicking the row's summary must bring the whole brief up, readable,
 * copyable, and Escape must put the reader back where they were. Each clipped site in the panel is
 * driven here through the real panel against measured event shapes, never through a stand-in.
 */

const POOL = [
  { id: 'gabee-qwen3.6-27b', model_id: 'gabee-qwen3.6-27b', weight: 2 },
  { id: 'mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 },
];

export const BRIEF =
  'Build the ledger store: a SQLite-backed append-only table of payments with (id, vendor, amount_cents, ' +
  'currency, posted_at) and a `Store.add(payment)` that rejects a duplicate id with StoreError, ' +
  'a `Store.list(vendor=None, since=None)` returning rows newest-first, and a `Store.export_csv(path)` ' +
  'that writes the RFC 4180 form the vendor docs at /v3/docs describe. Owns store.py and tests/test_store.py; ' +
  'the api slice imports Store from it and nothing else.';

const ts = new Date(Date.now() - 60_000).toISOString();

export const EVENTS: Array<Record<string, unknown>> = [
  { event: 'run_started', prompt: '# Build `vendorsync`\n\nA small operations tool.', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 2 },
  { event: 'phase', phase: 'open', ts },
  { event: 'phase', phase: 'synthesis', ts },
  {
    event: 'plan_loaded',
    ts,
    task_count: 3,
    plan_confidence: 88,
    ask_floor: 85,
    tasks: [
      { id: 'store', description: BRIEF, files: ['store.py'], deps: [], difficulty: 'medium' },
      { id: 'api', description: 'Build the api', files: ['api.py'], deps: ['store'], difficulty: 'hard' },
      { id: 'integrate-verify', description: 'Sink', files: [], deps: ['store', 'api'], difficulty: 'hard' },
    ],
  },
  { event: 'phase', phase: 'build', ts },
  { event: 'task_dispatched', ts, task_id: 'store', device: 'gabee-qwen3.6-27b', model: POOL[0].model_id },
];

export const ACTIVITY = {
  store: {
    model: POOL[0].model_id,
    thinking_chars: 1200,
    last_thinking: 'the store must reject duplicates before the insert',
    full_thinking: 'the store must reject duplicates before the insert',
    calls: [{ name: 'shell', summary: 'pytest -q tests/test_store.py -k duplicate --maxfail=1 --disable-warnings', ok: true }],
    tool_calls: 1,
  },
};

type ElectronMock = Record<string, unknown>;

export function mockRun(events = EVENTS, activity: Record<string, unknown> = ACTIVITY) {
  const electron = (window as unknown as { electron: ElectronMock }).electron;
  electron.readSwarmRun = vi.fn(async () => ({
    runId: 'swarm-test',
    dir: '/tmp/build',
    events,
    activity,
    activityMtimes: Object.fromEntries(Object.keys(activity).map((k) => [k, Date.now()])),
    clarify: null,
    mtime: Date.now(),
    heartbeat: Date.now(),
    heartbeatExited: false,
    pauseRequested: false,
  }));
  electron.fleetStatus = vi.fn(async () => ({}));
  electron.swarmSetPaused = vi.fn(async () => true);
  electron.swarmAddNote = vi.fn(async () => true);
  electron.revealInFinder = vi.fn(async () => undefined);
  electron.writeFile = vi.fn(async () => true);
  electron.readSwarmActivityLog = vi.fn(async () => null);
}

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

// jsdom lays nothing out: every measured span reports as overflowing, the way a long line does in the app.
beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
  Object.defineProperty(HTMLElement.prototype, 'scrollWidth', { configurable: true, get: () => 1000 });
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 100 });
});
afterAll(() => {
  vi.unstubAllGlobals();
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
});
beforeEach(() => mockRun());
afterEach(() => vi.restoreAllMocks());

/** The panel under the intl provider some of its empty states need. */
function renderPanel() {
  return render(
    <IntlTestWrapper>
      <SwarmRunPanel workingDir="/tmp/build" />
    </IntlTestWrapper>
  );
}

/** Click a clipped control, read the reveal, Escape it, and prove focus came back. */
export function revealAndClose(control: HTMLElement, expectText: string) {
  fireEvent.click(control);
  const dialog = screen.getByRole('dialog');
  expect(screen.getByTestId('reveal-body')).toHaveTextContent(expectText);
  assertStudioClean(dialog);
  fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(control);
}

/** The store row's summary among the board's rows — the one whose title is the whole brief. */
async function findBriefSummary(): Promise<HTMLElement> {
  return waitFor(() => {
    const hit = screen.getAllByTestId('board-row-summary').find((el) => el.getAttribute('title') === BRIEF);
    if (!hit) throw new Error('no board row carries the brief');
    return hit;
  });
}

describe('WORK board — the 400-char brief behind a 90-char summary', () => {
  it('the running row summary is clipped, titled with the brief, and reveals it whole', async () => {
    renderPanel();
    const summary = await findBriefSummary();
    expect(summary).toHaveAttribute('data-clipped', 'true');
    expect(summary).toHaveAttribute('role', 'button');
    expect(summary).toHaveAttribute('title', BRIEF);
    expect(summary.textContent).not.toContain('nothing else.');
    revealAndClose(summary, BRIEF);
    const dialog = (() => {
      fireEvent.click(summary);
      return screen.getByRole('dialog');
    })();
    expect(dialog).toHaveTextContent('Task brief');
    expect(within(dialog).getByText('store')).toBeInTheDocument();
    expect(dialog).toHaveTextContent('running');
    fireEvent.click(screen.getByTestId('reveal-close'));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('opening the reveal does not toggle the row it lives in', async () => {
    renderPanel();
    const summary = await findBriefSummary();
    const row = summary.closest('[data-testid="board-row"]') as HTMLElement;
    const before = row.querySelectorAll('*').length;
    fireEvent.click(summary);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(row.querySelectorAll('*').length).toBe(before);
  });

  it("the running row's live call line reveals the whole command, monospace", async () => {
    renderPanel();
    const live = await screen.findByTestId('board-row-live');
    expect(live).toHaveAttribute('data-clipped', 'true');
    expect(live).toHaveAttribute('title', ACTIVITY.store.calls[0].summary);
    fireEvent.click(live);
    expect(screen.getByTestId('reveal-body').className).toContain('font-mono');
    expect(screen.getByTestId('reveal-body')).toHaveTextContent('pytest -q tests/test_store.py');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
  });

  it('a queued row says what it waits on, and the wait list is a door too', async () => {
    renderPanel();
    const deps = await screen.findAllByTestId('board-row-deps');
    expect(deps[0]).toHaveAttribute('title', 'after store');
    revealAndClose(deps[0], 'after store');
  });
});

describe('Planning — a checklist row detail is a door too', () => {
  it('the timed-out ask row reveals its whole detail line', async () => {
    mockRun(
      [
        EVENTS[0],
        EVENTS[1],
        { event: 'phase', phase: 'ask', ts },
        { event: 'low_confidence_ask', ts, questions: [{ question: 'D1?' }, { question: 'D2?' }, { question: 'D3?' }] },
        { event: 'low_confidence_ask_timeout', ts, waited_secs: 5, questions_unanswered: 3, detail: 'no answers arrived' },
      ],
      {}
    );
    renderPanel();
    const details = await screen.findAllByTestId('todo-row-detail');
    const ask = details.find((el) => (el.getAttribute('title') ?? '').includes('unanswered at the unattended window'));
    expect(ask).toBeDefined();
    expect(ask).toHaveAttribute('data-clipped', 'true');
    revealAndClose(ask as HTMLElement, 'unanswered at the unattended window');
    fireEvent.click(ask as HTMLElement);
    expect(screen.getByRole('dialog')).toHaveTextContent('Detail');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
  });
});

describe('Event log — a line’s sub-detail and the collapsed header’s last event', () => {
  it('the dispatch line’s detail is a door, and the row’s own inline expand still works beside it', async () => {
    renderPanel();
    // The default log mode is verbose: the log is open and every line wraps to a two-line clamp.
    const subs = await screen.findAllByTestId('activity-sub');
    const dispatch = subs.find((el) => el.getAttribute('title') === 'on gabee');
    expect(dispatch).toBeDefined();
    expect(dispatch).toHaveAttribute('data-clipped', 'true');
    revealAndClose(dispatch as HTMLElement, 'on gabee');
    fireEvent.click(dispatch as HTMLElement);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('dispatch');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });

    // The row still expands inline on a click that is not on the door.
    const row = (dispatch as HTMLElement).parentElement as HTMLElement;
    fireEvent.click(row);
    expect(screen.queryByRole('dialog')).toBeNull();
    const li = row.closest('li') as HTMLElement;
    expect(li.textContent).toContain('on gabee');
    expect(within(li).queryByTestId('activity-sub')).toBeNull();
  });

  it('the collapsed header’s last event reveals the line with its detail', async () => {
    renderPanel();
    const header = await screen.findByText('Event log');
    fireEvent.click(header.closest('button') as HTMLElement);
    const last = await screen.findByTestId('event-log-last');
    expect(last).toHaveAttribute('data-clipped', 'true');
    const full = last.getAttribute('title') ?? '';
    expect(full.length).toBeGreaterThan(0);
    revealAndClose(last, full);
  });
});

describe('FLEET — the task cell, the node name, the live cell’s corner door, the also-row', () => {
  const twoOnGabee = [
    ...EVENTS,
    { event: 'task_dispatched', ts, task_id: 'api', device: 'gabee-qwen3.6-27b', model: POOL[0].model_id },
  ];
  const activity = {
    ...ACTIVITY,
    api: { model: POOL[0].model_id, thinking_chars: 300, last_thinking: 'the api wraps Store.list in a handler', full_thinking: 'the api wraps Store.list in a handler' },
  };

  it('the task cell reveals the whole brief without opening the node inspector', async () => {
    mockRun(twoOnGabee, activity);
    renderPanel();
    const door = await waitFor(() => {
      const hit = [
        ...screen.queryAllByTestId('fleet-node-task'),
        ...screen.queryAllByTestId('fleet-also-title'),
      ].find((el) => el.getAttribute('title') === BRIEF);
      if (!hit) throw new Error('no fleet cell carries the brief');
      return hit;
    });
    expect(door).toHaveAttribute('data-clipped', 'true');
    fireEvent.click(door);
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByTestId('reveal-dialog')).toHaveTextContent('store');
    expect(document.querySelector('[role="dialog"][aria-label^="Node "]')).toBeNull();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(door);
  });

  it('the node name is a door when the column clips it', async () => {
    mockRun(twoOnGabee, activity);
    renderPanel();
    const names = await screen.findAllByTestId('fleet-node-name');
    const gabee = names.find((el) => el.getAttribute('title') === 'gabee');
    expect(gabee).toBeDefined();
    revealAndClose(gabee as HTMLElement, 'gabee');
  });

  it('the live generation cell has a corner door to the whole line, monospace', async () => {
    mockRun(twoOnGabee, activity);
    renderPanel();
    const glyphs = await screen.findAllByTestId('reveal-glyph');
    const inFleet = glyphs.find((g) => g.closest('[data-testid="fleet-node"]'));
    expect(inFleet).toBeDefined();
    expect((inFleet as HTMLElement).tagName).toBe('BUTTON');
    fireEvent.click(inFleet as HTMLElement);
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByTestId('reveal-dialog')).toHaveTextContent('Working on');
    expect(screen.getByTestId('reveal-body').className).toContain('font-mono');
    expect(document.querySelector('[role="dialog"][aria-label^="Node "]')).toBeNull();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(document.activeElement).toBe(inFleet);
  });

  it('the also-row’s live line is a door and its click never re-aims the inspector', async () => {
    mockRun(twoOnGabee, activity);
    renderPanel();
    const live = await screen.findByTestId('fleet-also-live');
    expect(live).toHaveAttribute('data-clipped', 'true');
    const full = live.getAttribute('title') ?? '';
    expect(full.length).toBeGreaterThan(0);
    fireEvent.click(live);
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(full);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
  });
});

describe('Lane rows, the node inspector header, and history rows', () => {
  it('a planning lane’s description is a door inside the row button, and the row keeps its own toggle', async () => {
    mockRun(
      [...EVENTS.slice(0, 4)],
      { synthesis: { model: POOL[1].model_id, last_text: 'wiring the dag from the two slice specs into one build order' } }
    );
    renderPanel();
    const desc = await screen.findByTestId('lane-row-desc');
    expect(desc).toHaveAttribute('data-clipped', 'true');
    expect(desc).toHaveAttribute('role', 'button');
    expect(desc).not.toHaveAttribute('title');
    const laneEl = desc.closest('[data-testid="turn-lane"]') as HTMLElement;
    const before = laneEl.querySelectorAll('*').length;
    const full = desc.textContent ?? '';
    revealAndClose(desc, full);
    expect(laneEl.querySelectorAll('*').length).toBe(before);
  });

  it('the inspector header’s task is a door that stacks OVER the inspector; Escape closes only the reveal', async () => {
    mockRun();
    renderPanel();
    const rows = await screen.findAllByTestId('fleet-node');
    const gabee = rows.find((r) => r.getAttribute('data-device') === 'gabee') as HTMLElement;
    fireEvent.click(gabee);
    const inspector = document.querySelector('[role="dialog"][aria-label^="Node "]') as HTMLElement;
    expect(inspector).not.toBeNull();
    const task = within(inspector).getByTestId('inspector-task');
    expect(task).toHaveAttribute('title', BRIEF);
    fireEvent.click(task);
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(BRIEF);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(screen.queryByTestId('reveal-dialog')).toBeNull();
    expect(document.querySelector('[role="dialog"][aria-label^="Node "]')).not.toBeNull();
    expect(document.activeElement).toBe(task);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(document.querySelector('[role="dialog"][aria-label^="Node "]')).toBeNull();
  });

  it('a finished call in the inspector’s history opens on the plan’s whole brief', async () => {
    mockRun([
      ...EVENTS,
      { event: 'task_completed', ts, task_id: 'store', status: 'done', device: 'gabee-qwen3.6-27b', attempts: 1, elapsed_ms: 155142, tool_calls: [] },
    ]);
    renderPanel();
    const rows = await screen.findAllByTestId('fleet-node');
    const gabee = rows.find((r) => r.getAttribute('data-device') === 'gabee') as HTMLElement;
    await waitFor(() => expect(gabee).toHaveAttribute('data-expandable', 'true'));
    fireEvent.click(gabee);
    const title = await screen.findByTestId('history-row-title');
    expect(title).toHaveAttribute('title', BRIEF);
    fireEvent.click(title);
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(BRIEF);
    expect(screen.queryByTestId('node-history-entry')?.querySelector('pre')).toBeNull();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(document.querySelector('[role="dialog"][aria-label^="Node "]')).not.toBeNull();
  });
});

describe('Run header and footer', () => {
  it('the app name is a door to the run’s whole prompt, not the tip’s first 400 chars', async () => {
    mockRun();
    renderPanel();
    const name = await screen.findByTestId('run-header-name');
    expect(name).toHaveAttribute('data-clipped', 'true');
    expect(name).not.toHaveAttribute('title');
    fireEvent.click(name);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('Prompt');
    expect(screen.getByTestId('reveal-body')).toHaveTextContent('A small operations tool.');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(document.activeElement).toBe(name);
  });

  it('the output directory line is a mono door', async () => {
    mockRun([
      ...EVENTS,
      { event: 'task_completed', ts, task_id: 'store', status: 'done', device: 'gabee-qwen3.6-27b', attempts: 1, elapsed_ms: 1000, tool_calls: [] },
      { event: 'task_dispatched', ts, task_id: 'api', device: 'mihai-qwen3.6-27b', model: POOL[1].model_id },
      { event: 'task_completed', ts, task_id: 'api', status: 'done', device: 'mihai-qwen3.6-27b', attempts: 1, elapsed_ms: 1000, tool_calls: [] },
      { event: 'task_dispatched', ts, task_id: 'integrate-verify', device: 'mihai-qwen3.6-27b', model: POOL[1].model_id },
      { event: 'task_completed', ts, task_id: 'integrate-verify', status: 'done', device: 'mihai-qwen3.6-27b', attempts: 1, elapsed_ms: 1000, tool_calls: [] },
      { event: 'run_finished', ts, done: 3, failed: 0, phases: {}, per_device: [] },
    ]);
    renderPanel();
    const dir = await screen.findByTestId('run-output-dir');
    expect(dir).toHaveAttribute('data-clipped', 'true');
    fireEvent.click(dir);
    expect(screen.getByTestId('reveal-body')).toHaveTextContent('/tmp/build');
    expect(screen.getByTestId('reveal-body').className).toContain('font-mono');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
  });
});

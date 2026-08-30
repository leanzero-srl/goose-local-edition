import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE CUMULATIVE NODE INSPECTOR (Mihai, from live use): "as soon as a phase ends the whole thing
 * clears and that's it... when it finishes a phase it folds it into a line visually and then moves
 * on and this way we can have a proper log all in a place".
 *
 * Pins, against the poller mock (staleTruth-style, never a hand-built half-state):
 *   - a finished lane appears FOLDED with DURABLE byte sizes (never thinkingChars — the r5 opener
 *     measured 38,780 stream chars vs 128,270 durable bytes, and the counter resets on restream);
 *   - expanding an entry loads the WHOLE durable file over the on-demand IPC and captions it
 *     "all N bytes" (the workCaption law: the header counts what the body shows);
 *   - the live lane stays expanded on top, unaffected;
 *   - NOTHING clears on phase end — a lane finishing under an open inspector stays on screen,
 *     labeled finished, instead of the modal jumping away;
 *   - an idle node with history is still openable (the inspector is the log now).
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const TS = '2026-08-29T09:00:00.000000+00:00';

const BASE_EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: TS },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'build' },
  {
    event: 'task_dispatched',
    task_id: 'store',
    device: POOL[0].id,
    model: POOL[0].model_id,
    ts: TS,
  },
];

const DONE_EVENTS = [
  ...BASE_EVENTS,
  { event: 'task_completed', task_id: 'store', ts: TS, elapsed_ms: 61_000 },
];

/** The r5 opener's honest-size shape: the stream counter reset, the durable file did not. */
const OPEN_DONE_DIGEST = {
  phase: 'done',
  model: POOL[0].model_id,
  thinking_chars: 38_780,
  thinking_bytes: 128_270,
  transcript_bytes: 24_102,
  full_thinking: 'the tail of the opener reasoning',
  full_transcript: 'the opener answer',
};

const STORE_LIVE_DIGEST = {
  model: POOL[0].model_id,
  thinking_chars: 2_400,
  thinking_bytes: 500_000,
  full_thinking: 'short live tail of the store reasoning',
};

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

type Payload = {
  events?: Array<Record<string, unknown>>;
  activity?: Record<string, unknown>;
};

const payload = (over: Payload = {}) => {
  const activity = over.activity ?? { open: OPEN_DONE_DIGEST, store: STORE_LIVE_DIGEST };
  return {
    runId: 'node-history',
    dir: '/tmp/build',
    events: over.events ?? BASE_EVENTS,
    activity,
    activityMtimes: Object.fromEntries(Object.keys(activity).map((k) => [k, Date.now()])),
    clarify: null,
    mtime: Date.now(),
    heartbeat: Date.now(),
    heartbeatExited: false,
    pauseRequested: false,
  };
};

const mockRun = (over: Payload = {}) => {
  electron().readSwarmRun = vi.fn(async () => payload(over));
};

const mount = () =>
  render(
    <IntlTestWrapper>
      <SwarmRunPanel workingDir="/tmp/build" />
    </IntlTestWrapper>
  );

const openInspector = async () => {
  const cell = await screen.findByLabelText(/Open the full stream from mihai/);
  fireEvent.click(cell);
  return screen.findByRole('dialog');
};

beforeEach(() => {
  const e = electron();
  e.fleetStatus = vi.fn(async () => ({}));
  e.swarmSetPaused = vi.fn(async () => true);
  e.swarmAddNote = vi.fn(async () => true);
  e.writeFile = vi.fn(async () => true);
  e.onSwarmDelta = vi.fn(() => () => {});
  e.readSwarmActivityLog = vi.fn(async () => null);
});

describe('the folded history entry', () => {
  it('lists the finished call with DURABLE byte sizes, never the resettable stream counter', async () => {
    mockRun();
    mount();
    await openInspector();
    const row = await screen.findByTestId('node-history-row');
    expect(row.textContent).toContain('thought 128,270 B');
    expect(row.textContent).toContain('said 24,102 B');
    expect(row.textContent).not.toContain('38,780');
    // The section header counts what the body shows.
    expect(screen.getByTestId('node-history').textContent).toContain('1 finished');
  });

  it('expand loads the WHOLE durable file and captions it "all N bytes"', async () => {
    mockRun();
    electron().readSwarmActivityLog = vi.fn(async (_dir: string, _key: string, channel: string) =>
      channel === 'thinking'
        ? { text: 'THE VERY BEGINNING of the opener reasoning … THE END', bytes: 128_270 }
        : null
    );
    mount();
    await openInspector();
    fireEvent.click(await screen.findByTestId('node-history-row'));
    expect(await screen.findByText(/all 128,270 bytes/)).toBeInTheDocument();
    expect(screen.getByText(/THE VERY BEGINNING of the opener reasoning/)).toBeInTheDocument();
    // The absent answer channel is a NAMED absence, never blank.
    expect(screen.getByText(/No \.log on disk/)).toBeInTheDocument();
    // The read went through the on-demand IPC at the RESOLVED run dir.
    expect(electron().readSwarmActivityLog).toHaveBeenCalledWith('/tmp/build', 'open', 'thinking');
  });

  it('keeps the live lane expanded on top, unaffected by the history below', async () => {
    mockRun();
    mount();
    const dialog = await openInspector();
    expect(dialog.getAttribute('data-task')).toBe('store');
    expect(screen.getByText('short live tail of the store reasoning')).toBeInTheDocument();
    expect(screen.getByText('Earlier calls on this node')).toBeInTheDocument();
  });
});

describe('nothing clears on phase end', () => {
  it('a lane that finishes under an open inspector stays on screen, labeled finished', async () => {
    const live = payload();
    const done = payload({
      events: DONE_EVENTS,
      activity: { open: OPEN_DONE_DIGEST, store: { ...STORE_LIVE_DIGEST, phase: 'done' } },
    });
    const read = vi.fn(async () => live);
    electron().readSwarmRun = read;
    mount();
    const dialog = await openInspector();
    expect(dialog.getAttribute('data-task')).toBe('store');
    // The next polls see the task completed — the durable files and digest persist on disk.
    read.mockImplementation(async () => done);
    expect(await screen.findByTestId('inspector-lane-ended')).toHaveTextContent('finished');
    // Still the same lane under the reader — the modal never jumped or emptied.
    expect(screen.getByRole('dialog').getAttribute('data-task')).toBe('store');
    expect(screen.getByText('short live tail of the store reasoning')).toBeInTheDocument();
  });

  it('an idle node with only finished calls still opens — the inspector is the cumulative log', async () => {
    mockRun({
      events: DONE_EVENTS,
      activity: { open: OPEN_DONE_DIGEST, store: { ...STORE_LIVE_DIGEST, phase: 'done' } },
    });
    mount();
    const cell = await screen.findByLabelText(/Open the calls mihai ran this run/);
    fireEvent.click(cell);
    await screen.findByRole('dialog');
    expect(screen.getByText('Calls this node ran')).toBeInTheDocument();
    expect(screen.getAllByTestId('node-history-row').length).toBe(2);
  });
});

describe('the WORK pane show-all control — the symmetric twin of the thinking one', () => {
  it('offers "show all" on a clipped transcript and swaps in the whole durable answer log', async () => {
    mockRun({
      activity: {
        open: OPEN_DONE_DIGEST,
        store: {
          ...STORE_LIVE_DIGEST,
          full_transcript: 'live tail of the store answer',
          transcript_bytes: 300_000,
          transcript_clipped: true,
        },
      },
    });
    electron().readSwarmActivityLog = vi.fn(async (_dir: string, _key: string, channel: string) =>
      channel === 'transcript'
        ? { text: 'THE ANSWER LOG FROM THE VERY START … THE END', bytes: 300_000 }
        : null
    );
    mount();
    await openInspector();
    const btn = await screen.findByLabelText(/Show the whole 300,000-byte answer log/);
    expect(btn.textContent).toContain('show all 293 KB');
    fireEvent.click(btn);
    // The caption is the workCaption law on a full read: ALL the bytes, counted as what is shown.
    expect(await screen.findByText(/all 300,000 bytes/)).toBeInTheDocument();
    expect(screen.getByText(/THE ANSWER LOG FROM THE VERY START/)).toBeInTheDocument();
    expect(electron().readSwarmActivityLog).toHaveBeenCalledWith('/tmp/build', 'store', 'transcript');
    // And the way back to the live structured view is a real, named control.
    const back = screen.getByLabelText(/Back to the live view of the answer channel/);
    fireEvent.click(back);
    // The narration also paints in the fleet cell and the lane header, so count, don't get-one.
    await waitFor(() =>
      expect(screen.getAllByText('live tail of the store answer').length).toBeGreaterThan(0)
    );
    expect(screen.queryByText(/THE ANSWER LOG FROM THE VERY START/)).toBeNull();
  });
});

describe('jump-to-start on full-log views — "it cuts the BEGINNING" gets a way back to it', () => {
  it('the show-all view lands following the end, offers a named start jump, and a named way back', async () => {
    mockRun();
    electron().readSwarmActivityLog = vi.fn(async () => ({
      text: 'FULL THINK LOG FROM THE VERY START',
      bytes: 500_000,
    }));
    mount();
    await openInspector();
    // The LIVE tail never offers the jump: its beginning is not on disk in this view.
    expect(screen.queryByLabelText('Jump to the start of this log')).toBeNull();
    fireEvent.click(await screen.findByLabelText(/Show the whole 500,000-byte reasoning log/));
    await screen.findByText(/all 500,000 bytes/);
    const start = await screen.findByLabelText('Jump to the start of this log');
    fireEvent.click(start);
    // Leaving the end is a state, not a scroll accident: the way back is its own named control.
    const end = await screen.findByLabelText('Back to the end of this log');
    fireEvent.click(end);
    await waitFor(() => expect(screen.queryByLabelText('Back to the end of this log')).toBeNull());
  });

  it('an expanded history entry (a whole durable file) carries the same start jump', async () => {
    mockRun();
    electron().readSwarmActivityLog = vi.fn(async (_dir: string, _key: string, channel: string) =>
      channel === 'thinking'
        ? { text: 'THE VERY BEGINNING of the opener reasoning … THE END', bytes: 128_270 }
        : null
    );
    mount();
    await openInspector();
    fireEvent.click(await screen.findByTestId('node-history-row'));
    await screen.findByText(/all 128,270 bytes/);
    expect(screen.getAllByLabelText('Jump to the start of this log').length).toBeGreaterThan(0);
  });
});

describe('the live pane show-all control', () => {
  it('offers "show all" when the durable log outsizes the tail, and swaps in the whole file', async () => {
    mockRun();
    electron().readSwarmActivityLog = vi.fn(async () => ({
      text: 'FULL THINK LOG FROM THE VERY START',
      bytes: 500_000,
    }));
    mount();
    await openInspector();
    const btn = await screen.findByLabelText(/Show the whole 500,000-byte reasoning log/);
    expect(btn.textContent).toContain('show all 488 KB');
    fireEvent.click(btn);
    expect(await screen.findByText(/all 500,000 bytes/)).toBeInTheDocument();
    expect(screen.getByText('FULL THINK LOG FROM THE VERY START')).toBeInTheDocument();
    // And the way back to the live tail is a real, named control.
    const back = screen.getByLabelText(/Back to the live tail/);
    fireEvent.click(back);
    await waitFor(() =>
      expect(screen.getByText('short live tail of the store reasoning')).toBeInTheDocument()
    );
  });
});

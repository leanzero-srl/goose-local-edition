import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  ANSWER_WINDOW_NOTE,
  SwarmRunPanel,
  narrativeWindowNote,
  taskGenWindowNote,
} from './SwarmRunPanel';
import { useSwarmRun } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * A CLIPPED PANE MUST NOT LOOK LIKE A COMPLETE ONE — rendered, not just computed.
 *
 * main.ts reads a bounded tail of each durable log and attaches the file's true size (`thinking_bytes`,
 * `transcript_bytes`) plus its own verdict (`transcript_clipped`). `thinking_bytes` and `transcript_clipped`
 * had ZERO readers anywhere in the renderer: the THINKING caption counted `thinking_chars`, an engine
 * per-stream counter that resets on a re-stream, and the OUTPUT caption re-derived clipping by comparing
 * on-disk BYTES against a UTF-16 length. This test opens the real inspector and reads the real captions,
 * because both fields were "plumbed" before and neither reached a pane.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
// The hook DROPS every digest whose mtime predates the run's own start (a previous run's leftovers), so
// the fixture's clock has to be relative — a hard-coded ts silently empties the activity join.
const ts = new Date(Date.now() - 60_000).toISOString();

const THINK_TAIL = 'the last of a very long reasoning run';
const OUT_TAIL = 'the last of a very long answer';

const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'ledgerd-core',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

const ACTIVITY = {
  'ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 2003,
    last_thinking: 'the 2,400-char rolling window',
    full_thinking: THINK_TAIL,
    thinking_bytes: 812_000,
    full_transcript: OUT_TAIL,
    transcript_bytes: 640_000,
    transcript_clipped: true,
    calls: [{ name: 'shell', summary: 'pytest -q', ok: true }],
    tool_calls: 1,
  },
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('the node inspector admits when a pane is only a tail', () => {
  beforeEach(() => {
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-clipped',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: { 'ledgerd-core': Date.now() },
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

  it('captions both panes from the sizes on disk, and shows the durable text', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );

    const cell = await screen.findByTestId('fleet-node');
    expect(cell.getAttribute('data-expandable')).toBe('true');
    fireEvent.click(cell.querySelector('[role="button"]') ?? cell);

    const dialog = await screen.findByRole('dialog');

    // THINKING: `thinking_chars` (2,003) is not the denominator — the think.log is 812,000 bytes on disk.
    expect(dialog.textContent).toContain('tail of 793KB');
    expect(dialog.textContent).not.toContain('of 2,003 chars');
    expect(dialog.textContent).toContain(THINK_TAIL);

    // OUTPUT: main.ts already answered the clipping question; the pane must report its answer.
    expect(dialog.textContent).toContain('tail of 625KB');
    expect(dialog.textContent).toContain(OUT_TAIL);
  });

  it('says nothing about tails when both logs arrived whole', async () => {
    const whole = {
      'ledgerd-core': {
        ...ACTIVITY['ledgerd-core'],
        thinking_bytes: new TextEncoder().encode(THINK_TAIL).length,
        transcript_bytes: new TextEncoder().encode(OUT_TAIL).length,
        transcript_clipped: false,
      },
    };
    (electron().readSwarmRun as ReturnType<typeof vi.fn>).mockImplementation(async () => ({
      runId: 'swarm-whole',
      dir: '/tmp/build',
      events: EVENTS,
      activity: whole,
      activityMtimes: { 'ledgerd-core': Date.now() },
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));

    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );

    const cell = await screen.findByTestId('fleet-node');
    fireEvent.click(cell.querySelector('[role="button"]') ?? cell);

    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).not.toContain('tail of');
  });

  /**
   * AGENDA ITEM V, UI HALF: a pre-transcript ARCHIVE's digest carries `full_reasoning` — the engine's
   * 24,000-char answer WINDOW — and no durable log. The window read survives for exactly that case
   * (39 such archives measured under ~/goose-builds), captioned as the window it is, and it lives in
   * the ANSWER pane by channel: the Thinking pane used to borrow it, so an archived (or non-thinking)
   * lane rendered its answer under "Thinking" with "archived digest; full log unavailable" beside it.
   */
  describe('the answer-window fallback is captioned, and sits in the answer pane', () => {
    it('predicates: the note fires only when the durable log the chain prefers is absent', () => {
      expect(narrativeWindowNote({ answerWindow: 'the window' })).toBe(ANSWER_WINDOW_NOTE);
      expect(
        narrativeWindowNote({ answerWindow: 'the window', fullTranscript: 'durable answer log' })
      ).toBeNull();
      expect(narrativeWindowNote({})).toBeNull();
      // Lane-shaped (VA-025): the card reads the joined lane; the wire-key read lives in the join alone.
      expect(taskGenWindowNote({ answerWindow: 'the window' })).toBe(ANSWER_WINDOW_NOTE);
      expect(
        taskGenWindowNote({ answerWindow: 'the window', fullThinking: 'durable think.log' })
      ).toBeNull();
      expect(
        taskGenWindowNote({ answerWindow: 'the window', fullTranscript: 'durable task.log' })
      ).toBeNull();
      expect(taskGenWindowNote({})).toBeNull();
    });

    it('the inspector shows an archived window ONCE, under Work, captioned — and Thinking says why it is empty', async () => {
      (electron().readSwarmRun as ReturnType<typeof vi.fn>).mockImplementation(async () => ({
        runId: 'swarm-archived-clip',
        dir: '/tmp/build',
        events: EVENTS,
        activity: {
          'ledgerd-core': {
            model: 'mihai-qwen3.6-27b',
            full_reasoning: 'the last 24k characters of a much longer narration',
            tool_calls: 3,
          },
        },
        activityMtimes: { 'ledgerd-core': Date.now() },
        clarify: null,
        mtime: Date.now(),
        heartbeat: Date.now(),
        heartbeatExited: false,
        pauseRequested: false,
      }));

      const { result } = renderHook(() => useSwarmRun('/tmp/build'));
      await waitFor(() => expect(result.current.present).toBe(true));
      render(
        <IntlTestWrapper>
          <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
        </IntlTestWrapper>
      );

      const cell = await screen.findByTestId('fleet-node');
      fireEvent.click(cell.querySelector('[role="button"]') ?? cell);
      const dialog = await screen.findByRole('dialog');
      const text = dialog.textContent ?? '';
      const WINDOW = 'the last 24k characters of a much longer narration';
      // The body appears exactly once — in Work. Before, it rendered under Thinking as well.
      expect(text.split(WINDOW).length - 1).toBe(1);
      expect(text).toContain(ANSWER_WINDOW_NOTE);
      expect(text).not.toContain('archived digest');
      // The Thinking pane is empty and says the true reason: this call has no reasoning channel — not
      // "has not produced a token", which the window beside it disproves.
      expect(text).toContain('No reasoning channel on this call');
      expect(text).not.toContain('has not produced a token');
    });
  });
});

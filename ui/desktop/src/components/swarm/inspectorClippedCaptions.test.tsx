import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  REASONING_CLIP_NOTE,
  SwarmRunPanel,
  narrativeClipNote,
  taskGenClipNote,
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
   * ITEM 2's residue (agenda item V): an ARCHIVED digest whose durable logs are gone still carries
   * `full_reasoning` — the engine's 24,000-char TAIL CLIP — and every surface that falls back to it
   * used to present the clip as the whole record. The fallback stays (archived-run compat); the
   * honest caption is what closes the item.
   */
  describe('the full_reasoning fallback is captioned as the 24k clip it is', () => {
    it('predicates: the note fires only when the durable log the chain prefers is absent', () => {
      expect(narrativeClipNote({ fullReasoning: 'the 24k clip' })).toBe(REASONING_CLIP_NOTE);
      expect(
        narrativeClipNote({ fullReasoning: 'the 24k clip', fullTranscript: 'durable answer log' })
      ).toBeNull();
      expect(narrativeClipNote({})).toBeNull();
      expect(taskGenClipNote({ full_reasoning: 'the 24k clip' })).toBe(REASONING_CLIP_NOTE);
      expect(
        taskGenClipNote({ full_reasoning: 'the 24k clip', full_thinking: 'durable think.log' })
      ).toBeNull();
      expect(taskGenClipNote({})).toBeNull();
    });

    it('the inspector THINKING caption says so when its body fell back to the clip', async () => {
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
      expect(dialog.textContent).toContain(REASONING_CLIP_NOTE);
    });
  });
});

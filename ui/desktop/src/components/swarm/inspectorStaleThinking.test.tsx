import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { resetFoldCache, resetLiveChannelMemory, useSwarmRun } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * VA-026 — A STALE ROLLING WINDOW MUST NOT FILL THE THINKING PANE, RENDERED.
 *
 * `inspectorThinkingText` fell back to `lastThinking` with no `thinkingChars` gate, while
 * `laneThinkingRun` (the strip and board) had one: two copies of one rule, one of them wrong. The
 * measured shape: a lane key reused for a new call — REVIEW's next round, a judge re-stream — whose
 * fresh digest stamps `thinking_chars: 0`, has no think.log yet, and still carries the previous
 * call's 2,400-char window (the join keeps the prior poll's `lastThinking` when a digest omits the
 * key; a seed digest may also carry it outright). The pane rendered the dead call's reasoning under
 * the new call's title, and the header beside it counted zero.
 *
 * This opens the real inspector on that shape. What a person must see is the honest empty from
 * a8dd8974e — "No reasoning channel on this call…" — because the call HAS work (an answer and a tool
 * call sit right beside it), and never the stale words.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
// The hook DROPS digests whose mtime predates the run's first event — the fixture clock is relative.
const ts = new Date(Date.now() - 60_000).toISOString();

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

const STALE_WINDOW = 'me analyze my slice from the PREVIOUS call on this lane key';

const ACTIVITY = {
  'ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    // The new call: counter reset, no durable think.log for it yet, a leftover window in the digest.
    thinking_chars: 0,
    last_thinking: STALE_WINDOW,
    // …and real work beside it, so the pane's honest empty is the "no reasoning channel" one.
    full_transcript: 'Wiring the ledger store to the api.',
    last_text: 'Wiring the ledger store to the api.',
    calls: [{ name: 'shell', summary: 'pytest -q', ok: true }],
    tool_calls: 1,
  },
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('the inspector THINKING pane refuses a stale window when the counter is zero', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-stale-window',
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

  it('shows the "no reasoning channel" empty, never the previous call’s words', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    // The lane carries the stale window and a zero counter — the exact input the pane used to render.
    const lane = result.current.lanes.find((l) => l.taskId === 'ledgerd-core');
    expect(lane?.lastThinking).toBe(STALE_WINDOW);
    expect(lane?.thinkingChars).toBe(0);

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );

    const cell = await screen.findByTestId('fleet-node');
    fireEvent.click(cell.querySelector('[role="button"]') ?? cell);
    const dialog = await screen.findByRole('dialog');
    const text = dialog.textContent ?? '';
    expect(text).toContain('No reasoning channel on this call');
    expect(text).not.toContain(STALE_WINDOW);
    // The answer channel is still there under Work — the empty is about THIS pane, not the call.
    expect(text).toContain('Wiring the ledger store to the api.');
  });
});

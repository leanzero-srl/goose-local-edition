import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { resetFoldCache, resetLiveChannelMemory, useSwarmRun } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * VA-025 — THE TASK CARD IS FED BY THE ONE JOIN, RENDERED.
 *
 * `TaskGenDetail` (the WORK board's "Live generation" card) used to receive the RAW activity digest and
 * map its keys by hand (`streamLaneOfDigest`): a second copy of `digestStreamFields`, on a render path
 * where the drift guards over the fold's lane groups could not see it. And it had already drifted —
 * `malformed` was read there and carried by no lane. The card now takes a lane the hook joined once
 * (deriveNodeHistory for a finished digest no event lane claims; deriveFleet for a live one).
 *
 * The one real case where a WORK row has a digest and no fold lane is a JUDGE-SPLIT PARENT: the fold
 * drops its lane on purpose (the children carry the work), while its call's digest stays on disk. This
 * opens that row in the real panel and reads the card's numbers off the joined lane — including the
 * field the join never carried before, and the "(live)" label the raw digest could never disprove.
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
  {
    event: 'judge_verdict',
    ts,
    task_id: 'ledgerd-core',
    verdict: 'looping',
    hint: 'split the ledger into store and api',
    action: 'split',
  },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'ledgerd-store',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

const PARENT_REASONING = 'reading every module twice before the judge split the task';

const ACTIVITY = {
  // The split parent's call, over (phase done): 5 calls, 4 of them reads — the over-reading shape the
  // card exists to show at a glance — plus the two counters the engine stamps beside them.
  'ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    phase: 'done',
    tool_calls: 5,
    calls: [
      { name: 'read', summary: 'app/ledger.py', ok: true },
      { name: 'read', summary: 'app/store.py', ok: true },
      { name: 'read', summary: 'app/api.py', ok: true },
      { name: 'read', summary: 'app/ledger.py', ok: true },
      { name: 'shell', summary: 'pytest -q', ok: false },
    ],
    thinking_chars: 3000,
    full_thinking: PARENT_REASONING,
    errors: 2,
    malformed: 1,
  },
  'ledgerd-store': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 12,
    full_thinking: 'starting on the store',
  },
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('the WORK board card for a judge-split parent reads the JOINED lane', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-split-parent',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: { 'ledgerd-core': Date.now(), 'ledgerd-store': Date.now() },
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

  it('shows the breakdown, the thinking count, BOTH error counters and a non-live label', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    // The fold dropped the parent's lane on the split — this is the digest-without-a-lane case.
    expect(result.current.lanes.some((l) => l.taskId === 'ledgerd-core')).toBe(false);
    expect(result.current.lanes.some((l) => l.taskId === 'ledgerd-store')).toBe(true);

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );

    const rows = await screen.findAllByTestId('board-row');
    const parent = rows.find((r) => (r.textContent ?? '').includes('split into sub-tasks'));
    expect(parent, 'the split parent must still be a board row').toBeTruthy();
    fireEvent.click(parent!.querySelector('[role="button"]') ?? parent!);

    const text = parent!.textContent ?? '';
    expect(text).toContain('Live generation');
    expect(text).toContain('tool calls 5');
    // The per-tool breakdown comes from the lane's `calls` — 4 reads and a shell.
    expect(text).toContain('4 read · 1 shell');
    expect(text).toContain('3,000 ch');
    // `malformed` reached this card ONLY through the join now — before, it was the one field a render
    // path read that no lane carried.
    expect(text).toContain('2 app-error · 1 malformed');
    expect(text).toContain(PARENT_REASONING);
    // The call is over: the raw digest could not say so and the card called it live.
    expect(text).toContain('Model reasoning');
    expect(text).not.toContain('Model reasoning (live)');
  });
});

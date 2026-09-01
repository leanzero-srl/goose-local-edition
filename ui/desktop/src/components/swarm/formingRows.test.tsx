import { cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SwarmRunPanel, formingLiveLine, laneLiveLine } from './SwarmRunPanel';
import { digestStreamFields, resetFoldCache, useSwarmRun } from './useSwarmRun';
import type { FormingCall, TurnLane } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * II-11b/c UI half: the forming state flows through digestStreamFields — the ONE shared join every
 * lane path uses — and is never carried from a previous poll. The sidecar's absence IS the engine
 * saying nothing is forming (the file is removed on completion and at scope exit), so a carried
 * row would outlive its own call and render a dead amber "forming…" forever.
 *
 * Since II-11c the sidecar carries `args_bytes`/`args_preview`, and the product moment is watching
 * the generation transform into the tool call: r5's OPEN sat visually frozen for 5 minutes while
 * 28 KB of arguments streamed. The tests below pin the three honest states — preview, zero-byte,
 * and absent — on the lane live line, the lane cell, and the inspector WORK pane.
 */
const FORMING: FormingCall[] = [
  {
    id: 'call-7',
    name: 'developer__write_file',
    since_ms: 1_000,
    args_bytes: 12_288,
    args_preview: '"content": "def apply(payment):\n    ledger.append(payment)',
  },
];

const ZERO_BYTE: FormingCall[] = [{ id: 'call-8', name: 'developer__shell', since_ms: 1_000 }];

describe('forming flows through the shared digest join and never survives its sidecar', () => {
  it('passes forming through when the digest carries it — byte progress and preview included', () => {
    const out = digestStreamFields('lane-a', { forming: FORMING });
    expect(out.forming).toEqual(FORMING);
    expect(out.forming?.[0].args_bytes).toBe(12_288);
    expect(out.forming?.[0].args_preview).toContain('ledger.append(payment)');
  });

  it('does NOT carry a previous poll\'s forming when the sidecar is gone', () => {
    const prev: Partial<TurnLane> = { forming: FORMING, toolCalls: 3 };
    const out = digestStreamFields('lane-a', { tool_calls: 4 }, prev);
    expect(out.forming).toBeUndefined();
    // the carry itself still works for fields that ARE carried
    expect(out.toolCalls).toBe(4);
  });

  it('an absent digest yields no forming row', () => {
    expect(digestStreamFields('lane-a', undefined, { forming: FORMING }).forming).toBeUndefined();
  });
});

describe('the forming live line — one honest LINE for the cell', () => {
  it('names the call, counts the bytes, and hands the row the freshest LINE of the preview', () => {
    expect(formingLiveLine(FORMING)).toBe(
      'forming write_file · 12,288 bytes · ledger.append(payment)'
    );
  });

  it('outranks the running call and both channels in laneLiveLine — these bytes are NOW', () => {
    expect(
      laneLiveLine({
        forming: FORMING,
        inflight: [{ id: 'call-1', tool: 'shell', args: 'shell: pytest -q', since: '2026-08-30T10:00:00Z' }],
        liveChannel: 'thinking',
        fullThinking: 'I should write the ledger core now',
      })
    ).toBe('forming write_file · 12,288 bytes · ledger.append(payment)');
  });

  it('a zero-byte forming call yields no line — the caller falls through as today', () => {
    expect(formingLiveLine(ZERO_BYTE)).toBe('');
    expect(
      laneLiveLine({ forming: ZERO_BYTE, fullTranscript: 'wrote the cli entry point' })
    ).toBe('wrote the cli entry point');
  });

  it('no forming rows means no forming line', () => {
    expect(formingLiveLine(undefined)).toBe('');
    expect(formingLiveLine([])).toBe('');
  });
});

// ---------------------------------------------------------------------------------------------------
// THE RENDERED SURFACES: the fleet cell's live line and the inspector WORK pane.
// ---------------------------------------------------------------------------------------------------

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const ts = new Date(Date.now() - 60_000).toISOString();
const EVENTS = [
  { event: 'run_started', prompt: '# Build `app`', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'ledgerd-core',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

/** r5's OPEN shape: a lane with ONLY thinking — no calls, no answer text — while a call forms. */
const digest = (forming?: FormingCall[]) => ({
  'ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 28_000,
    full_thinking: 'Planning the ledger core write now.',
    ...(forming ? { forming } : {}),
  },
});

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

const mockRun = (activity: Record<string, unknown>) => {
  const e = electron();
  e.readSwarmRun = vi.fn(async () => ({
    runId: 'swarm-forming',
    dir: '/tmp/build',
    events: EVENTS,
    activity,
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
  e.onSwarmDelta = vi.fn(() => () => {});
  e.readSwarmActivityLog = vi.fn(async () => null);
};

const openInspector = async () => {
  const { result } = renderHook(() => useSwarmRun('/tmp/build'));
  await waitFor(() => expect(result.current.present).toBe(true));
  render(
    <IntlTestWrapper>
      <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
    </IntlTestWrapper>
  );
  const cell = await screen.findByTestId('fleet-node');
  fireEvent.click(cell.querySelector('[role="button"]') ?? cell);
  return { cell, dialog: await screen.findByRole('dialog') };
};

describe('the WORK pane while a call forms', () => {
  beforeEach(() => resetFoldCache());
  afterEach(() => cleanup());

  it('shows the forming call with byte count and captioned preview instead of "Still thinking"', async () => {
    mockRun(digest(FORMING));
    const { cell, dialog } = await openInspector();
    // The placeholder must yield to the forming block — this is the r5 frozen-OPEN moment.
    expect(dialog.textContent).not.toContain('Still thinking');
    const row = dialog.querySelector('[data-testid="forming-row"]');
    expect(row).not.toBeNull();
    expect(row!.textContent).toContain('12,288 bytes of arguments');
    expect(row!.textContent).toMatch(/forming/i);
    // The preview is captioned as exactly what it is: a bounded tail of the arguments so far.
    const preview = dialog.querySelector('[data-testid="forming-preview"]');
    expect(preview).not.toBeNull();
    expect(preview!.textContent).toContain(
      `forming — last ${FORMING[0].args_preview!.length} chars of the arguments so far`
    );
    expect(preview!.textContent).toContain('ledger.append(payment)');
    // The header counts what the body shows.
    expect(dialog.textContent).toContain('1 forming');
    // The fleet cell's live line is the forming call, not the stale thought. The cell types its
    // text out (nextRevealedText), so wait for the reveal to reach the name.
    await waitFor(() => expect(cell.textContent).toContain('forming write_file'), {
      timeout: 4000,
    });
  });

  it('a zero-byte forming call renders name, spinner and clock — no pretended progress', async () => {
    mockRun(digest(ZERO_BYTE));
    const { dialog } = await openInspector();
    const row = dialog.querySelector('[data-testid="forming-row"]');
    expect(row).not.toBeNull();
    expect(row!.textContent).toContain("the model is still generating this call's arguments");
    expect(row!.textContent).not.toContain('bytes of arguments');
    expect(dialog.querySelector('[data-testid="forming-preview"]')).toBeNull();
  });

  it('no sidecar, no forming row — and no Work pane at all: output earns its column', async () => {
    mockRun(digest(undefined));
    const { dialog } = await openInspector();
    expect(dialog.querySelector('[data-testid="forming-row"]')).toBeNull();
    // The empty pane used to render stacked with the "Still thinking" placeholder. It renders
    // NOTHING now — the Thinking pane gets the whole modal until the first call/narration byte.
    expect(dialog.textContent).not.toContain('Still thinking');
    const paneTitles = Array.from(
      dialog.querySelectorAll('[data-testid="pane-title"]')
    ).map((el) => el.textContent);
    expect(paneTitles).toContain('Thinking');
    expect(paneTitles).not.toContain('Work');
  });
});

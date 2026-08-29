import { cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SwarmRunPanel, laneLiveLine } from './SwarmRunPanel';
import {
  elapsedSince,
  resetFoldCache,
  useSwarmRun,
  workCaption,
  workRows,
  type InflightCall,
  type SwarmCall,
} from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE WORK PANE SHOWS A TOOL CALL WHILE IT RUNS, NOT ONLY AFTER IT LANDS.
 *
 * Mihai, 22:29, on lane service-boot: "2 tool calls · 2 ok" listed a call only once its result arrived,
 * while THINKING streamed — a long write was invisible for its whole duration. The engine now writes an
 * `inflight` row at the REQUEST moment (swarm.rs build_worker_digest, one row per request without a result,
 * removed when the result lands) and the same pending set as provisional `ok: null` rows in `calls`. One
 * running row per call, above the finished ones; when the result lands the finished record is already in
 * `calls`, so the running row drops and the finished row takes its place — never both.
 */
const WRITE: InflightCall = {
  id: 'call_3',
  tool: 'write',
  args: 'write app/cli.py (83 lines, 2100 bytes)',
  since: new Date(Date.now() - 12_000).toISOString(),
};
const DONE: SwarmCall[] = [
  { name: 'shell', summary: 'ls -la', ok: true, result: 'total 8' },
  { name: 'read', summary: 'app/models.py', ok: true, result: 'class Payment' },
];
const PROVISIONAL: SwarmCall = { name: 'write', summary: 'app/cli.py', ok: null, result: '' };
const LANDED: SwarmCall = {
  name: 'write',
  summary: 'app/cli.py',
  ok: true,
  result: '',
  id: 'call_3',
};

describe('workRows — running rows and finished rows, never the same call twice', () => {
  it('lists the inflight row as running, drops its provisional ok:null twin, and counts it', () => {
    const r = workRows([...DONE, PROVISIONAL], [WRITE]);
    expect(r.running).toEqual([WRITE]);
    expect(r.completed).toEqual(DONE);
    expect(r.tallies).toMatchObject({ ok: 2, pending: 1 });
    expect(workCaption(r.completed.length + r.running.length, 2, r.tallies)).toBe(
      '3 tool calls · 2 ok · 1 running'
    );
  });

  it('when the result lands the finished row takes the running row’s place — one row for the id', () => {
    const r = workRows([...DONE, LANDED], []);
    expect(r.running).toEqual([]);
    expect(r.completed).toHaveLength(3);
    expect(r.completed.filter((c) => c.summary === 'app/cli.py')).toHaveLength(1);
    expect(workCaption(3, 3, r.tallies)).toBe('3 tool calls · 3 ok');
  });

  it('drops a finished row that still names an in-flight id', () => {
    const r = workRows([...DONE, LANDED], [WRITE]);
    expect(r.completed.map((c) => c.summary)).not.toContain('app/cli.py');
    expect(r.running).toHaveLength(1);
  });

  it('keeps the provisional shape for a digest from an engine without the key', () => {
    const r = workRows([...DONE, PROVISIONAL], undefined);
    expect(r.completed).toHaveLength(3);
    expect(r.running).toEqual([]);
    expect(r.tallies.pending).toBe(1);
  });
});

describe('elapsedSince', () => {
  const t0 = Date.parse('2026-08-29T22:40:01+00:00');
  it('reads seconds under a minute and m/s above it', () => {
    expect(elapsedSince('2026-08-29T22:40:01+00:00', t0 + 12_000)).toBe('12s');
    expect(elapsedSince('2026-08-29T22:40:01+00:00', t0 + 65_000)).toBe('1m 05s');
  });
  it('is empty for a stamp that does not parse, never NaN', () => {
    expect(elapsedSince('soon', t0)).toBe('');
  });
});

describe('the fleet cell’s live line names the running call', () => {
  it('prefers the running call over the freshest thought', () => {
    expect(
      laneLiveLine({
        inflight: [WRITE],
        liveChannel: 'thinking',
        fullThinking: 'I should write the CLI entry point now',
        lastThinking: 'I should write the CLI entry point now',
      })
    ).toBe('running: write app/cli.py (83 lines, 2100 bytes)');
  });

  it('counts the others when more than one is in flight', () => {
    const shell = { ...WRITE, id: 'call_4', tool: 'shell', args: 'shell: pytest -q' };
    expect(laneLiveLine({ inflight: [WRITE, shell] })).toBe('running: shell: pytest -q +1');
  });

  it('falls back to the channels when nothing is in flight', () => {
    expect(laneLiveLine({ inflight: [], fullTranscript: 'wrote the cli entry point' })).toBe(
      'wrote the cli entry point'
    );
  });
});

// ---------------------------------------------------------------------------------------------------
// THE RENDERED INSPECTOR, on the lane shape from the screenshot.
// ---------------------------------------------------------------------------------------------------

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const ts = new Date(Date.now() - 60_000).toISOString();
const EVENTS = [
  { event: 'run_started', prompt: '# Build `app`', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'service-boot',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

const digest = (calls: SwarmCall[], inflight: InflightCall[], toolCalls: number) => ({
  'service-boot': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 40,
    full_thinking: 'Now write the CLI entry point.',
    calls,
    inflight,
    tool_calls: toolCalls,
    recent: ['shell ok', 'read ok'],
    last_text: '.',
    full_transcript: 'Writing the CLI.',
  },
});

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

const mockRun = (activity: Record<string, unknown>) => {
  const e = electron();
  e.readSwarmRun = vi.fn(async () => ({
    runId: 'swarm-inflight',
    dir: '/tmp/build',
    events: EVENTS,
    activity,
    activityMtimes: { 'service-boot': Date.now() },
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

describe('the inspector’s WORK pane while a write is in flight', () => {
  beforeEach(() => resetFoldCache());
  afterEach(() => cleanup());

  it('renders the running write ABOVE the finished calls, with its args, an elapsed time and the count', async () => {
    mockRun(digest([...DONE, PROVISIONAL], [WRITE], 2));
    const { cell, dialog } = await openInspector();
    const rows = dialog.querySelectorAll('[data-testid="inflight-row"]');
    expect(rows).toHaveLength(1);
    const row = rows[0];
    expect(row.textContent).toContain('write app/cli.py (83 lines, 2100 bytes)');
    expect(row.textContent).toContain('Writing');
    expect(row.textContent).toMatch(/running/i);
    expect(row.querySelector('.tabular-nums')?.textContent).toMatch(/^\d+s$/);
    // Above the finished rows: it is the first row of the pane.
    expect(row.parentElement?.firstElementChild).toBe(row);
    expect(dialog.textContent).toContain('3 tool calls · 2 ok · 1 running');
    // Not also drawn as a finished write — the provisional twin is gone.
    expect(dialog.textContent).not.toContain('Wrote app/cli.py');
    // The fleet cell says what the node is doing, not what it last thought. The cell types its live line
    // out (nextRevealedText), so wait for the reveal to reach the end of the preview.
    await waitFor(
      () => expect(cell.textContent).toContain('running: write app/cli.py (83 lines, 2100 bytes)'),
      { timeout: 4000 }
    );
  });

  it('drops the running row when the result lands — the finished row is there, once', async () => {
    mockRun(digest([...DONE, LANDED], [], 3));
    const { cell, dialog } = await openInspector();
    expect(dialog.querySelector('[data-testid="inflight-row"]')).toBeNull();
    expect(dialog.textContent?.match(/Wrote app\/cli\.py/g) ?? []).toHaveLength(1);
    expect(dialog.textContent).toContain('3 tool calls · 3 ok');
    expect(dialog.textContent).not.toContain('1 running');
    expect(cell.textContent).not.toContain('running:');
  });
});

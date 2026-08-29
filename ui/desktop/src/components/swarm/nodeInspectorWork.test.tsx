import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel, squeezeBlankRuns, squeezeNote } from './SwarmRunPanel';
import {
  callRowMeta,
  callTallies,
  firstCallNeedingAttention,
  useSwarmRun,
  workCaption,
  type SwarmCall,
} from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE NODE INSPECTOR SHOWS THE WORK, NOT SIX WORDS ABOUT IT.
 *
 * MEASURED on lane `apptest-advertised-surface`: the header said "42 tool calls" and the body rendered
 * `[...recent, said]` — six literal "shell ok" strings joined onto a `<task>.log` that was 123 blank lines
 * out of 143. Every record needed to render the real work (`summary` = the command, `result` = its output)
 * sat unread in the same digest object. Four archived runs confirm the shape independently: digests with
 * 9-17 calls carry a `last_text` of ONE character.
 *
 * Every test below fails on the pre-change code, and each says which defect it catches.
 */

const okCall = (summary: string, result: string): SwarmCall => ({
  name: 'shell',
  summary,
  ok: true,
  result,
});

describe('callTallies — five DISJOINT buckets', () => {
  // CATCHES: the old tally folded 'ran-nothing' into appError, so the LYING-GREEN call — exit 0 while the
  // output proves nothing ran — had no number of its own anywhere in the UI. It also had no bucket at all
  // for pending, so the buckets could not sum to what was on screen.
  const calls: SwarmCall[] = [
    okCall('ls -la', 'total 8'),
    okCall('pytest -q | head', 'no tests ran'),
    { name: 'shell', summary: 'cargo test', ok: false, result: '3 tests failed' },
    { name: 'write', summary: 'x.rs', ok: false, result: 'missing field `path`' },
    { name: 'shell', summary: 'cargo build', ok: null, result: '' },
  ];

  it('gives the lying-green call its own number instead of hiding it in app-errors', () => {
    const t = callTallies(calls);
    expect(t.ranNothing).toBe(1);
    expect(t.appError).toBe(1);
    expect(t.ok).toBe(1);
    expect(t.malformed).toBe(1);
    expect(t.pending).toBe(1);
  });

  it('sums to exactly the number of rows on screen', () => {
    const t = callTallies(calls);
    expect(t.ok + t.appError + t.ranNothing + t.malformed + t.pending).toBe(calls.length);
  });
});

describe('firstCallNeedingAttention — one rule for three call sites', () => {
  // CATCHES: the lane row used `c.ok === false` and the board row used the classifier. The first opens on
  // every productive app-error (the worker testing), which buries the lane; two rules for one question is
  // how they drifted.
  it('walks past an app-error and stops on the malformed call', () => {
    expect(
      firstCallNeedingAttention([
        okCall('ls', 'a'),
        { name: 'shell', summary: 'cargo test', ok: false, result: '3 tests failed' },
        { name: 'write', summary: 'x.rs', ok: false, result: 'missing field `path`' },
      ])
    ).toBe(2);
  });

  it('stops on the lying-green call, which reports ok:true', () => {
    expect(
      firstCallNeedingAttention([okCall('ls', 'a'), okCall('pytest | head', 'no tests ran')])
    ).toBe(1);
  });

  it('is -1 when nothing needs attention', () => {
    expect(firstCallNeedingAttention([okCall('ls', 'a')])).toBe(-1);
  });
});

describe('callRowMeta — position in the WHOLE history, not the array index', () => {
  // CATCHES: the engine sends the LAST 60 resolved records plus every in-flight one, so `calls[0]` is call
  // #10 of 69 and rendering it as row 1 is a lie a reader cannot check. Keys were the array index, which
  // shifts by one every time the 60-window slides — remounting every row and losing every open output.
  const sixty = Array.from({ length: 60 }, (_, i) => okCall(`cmd ${i}`, 'out'));

  it('numbers the window against the engine total', () => {
    const meta = callRowMeta(sixty, 69);
    expect(meta[0].ordinal).toBe(10);
    expect(meta[59].ordinal).toBe(69);
    expect(meta[0].key).toBe('#10');
  });

  it('gives a pending call NO ordinal — tool_calls counts only resolved records', () => {
    const meta = callRowMeta([...sixty, { name: 'shell', summary: 'cargo build', ok: null }], 69);
    expect(meta[60].ordinal).toBeNull();
    expect(meta[59].ordinal).toBe(69);
  });

  it('keys duplicate pendings apart without depending on the engine HashMap order', () => {
    const meta = callRowMeta(
      [
        { name: 'shell', summary: 'cargo build', ok: null },
        { name: 'shell', summary: 'cargo build', ok: null },
      ],
      0
    );
    expect(meta[0].key).not.toBe(meta[1].key);
    expect(new Set(meta.map((m) => m.key)).size).toBe(2);
  });

  it('never reports fewer calls than it was handed, even with a stale engine total', () => {
    const meta = callRowMeta([okCall('a', 'x'), okCall('b', 'x')], 1);
    expect(meta.map((m) => m.ordinal)).toEqual([1, 2]);
  });
});

describe('workCaption — the sentence that stops the header counting one thing while the body shows another', () => {
  // CATCHES: the header printed `calls.length` while two other sites printed `lane.toolCalls ?? calls.length`,
  // so the modal said "42 tool calls" over a 51-record body. It also never admitted the 60-record window.
  it('labels the window instead of pretending it is the whole history', () => {
    const t = callTallies(Array.from({ length: 60 }, () => okCall('ls', 'out')));
    expect(workCaption(60, 69, t)).toBe('last 60 of 69 tool calls · 60 ok');
  });

  it('does not report "last 4 of 3" when a pending call overflows the resolved total', () => {
    const calls = [
      okCall('a', 'x'),
      okCall('b', 'x'),
      okCall('c', 'x'),
      { name: 'shell', summary: 'd', ok: null },
    ];
    expect(workCaption(calls.length, 3, callTallies(calls as SwarmCall[]))).toBe(
      '4 tool calls · 3 ok · 1 running'
    );
  });

  it('prints only the non-zero buckets, ran-nothing among them', () => {
    const calls: SwarmCall[] = [
      okCall('ls', 'total 8'),
      okCall('pytest | head', 'no tests ran'),
      { name: 'write', summary: 'x', ok: false, result: 'missing field `path`' },
    ];
    expect(workCaption(3, 3, callTallies(calls))).toBe(
      '3 tool calls · 1 ok · 1 ran nothing · 1 retried'
    );
  });

  it('says so plainly when there is nothing to count', () => {
    expect(workCaption(0, 0, callTallies([]))).toBe('no tool calls yet');
  });
});

describe('squeezeBlankRuns — the blank runs are a chunking artifact, not spacing', () => {
  // CATCHES: `<task>.log` is `texts[already..].join("")` over raw stream deltas, and `whitespace-pre-wrap`
  // rendered all 123 blank lines of a 143-line log — the screenfuls of nothing in the screenshot.
  it('collapses a 13-line gap to one', () => {
    expect(squeezeBlankRuns(`a${'\n'.repeat(14)}b`)).toBe('a\n\nb');
  });

  it('drops the TRAILING blank run, which is what follow was landing on', () => {
    expect(squeezeBlankRuns('a\nb\n\n\n\n')).toBe('a\nb');
    expect(squeezeBlankRuns('\n\n\na\n')).toBe('a');
  });

  it('leaves a text with no blank runs byte-identical', () => {
    const prose = 'I will read the manifest.\nThen I will run the tests.\n\nHere is what I found.';
    expect(squeezeBlankRuns(prose)).toBe(prose);
  });

  it('turns the measured 86%-blank shape into something readable', () => {
    const measured = Array.from({ length: 20 }, (_, i) => `line ${i}${'\n'.repeat(7)}`).join('');
    const out = squeezeBlankRuns(measured);
    const blanks = (t: string) => t.split('\n').filter((l) => !l.trim()).length;
    expect(blanks(measured) / measured.split('\n').length).toBeGreaterThan(0.8);
    expect(blanks(out)).toBe(19);
    expect(out.split('\n').filter((l) => l.trim()).length).toBe(20);
  });

  it('is safe on empty input', () => {
    expect(squeezeBlankRuns('')).toBe('');
  });
});

describe('squeezeNote', () => {
  it('says how much the screen is holding back, and stays silent otherwise', () => {
    expect(squeezeNote('a\n\n\n\nb', 'a\n\nb')).toBe(' · 2 blank lines collapsed');
    expect(squeezeNote('a\nb', 'a\nb')).toBe('');
  });
});

// ---------------------------------------------------------------------------------------------------
// THE RENDERED WINDOW. The unit tests above cannot see the actual defect Mihai reported six times: a
// header that counts work over a body that shows none of it. This opens the real inspector on a real
// tool-lane shape (many calls, one character of prose) and reads what is on screen.
// ---------------------------------------------------------------------------------------------------

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const ts = new Date(Date.now() - 60_000).toISOString();

const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'apptest-advertised-surface',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

// The measured lane: acts a lot, narrates almost nothing, and its log is mostly blank lines.
const CALLS = [
  { name: 'shell', summary: 'ls -la', ok: true, result: 'total 8\ndrwxr-xr-x  src' },
  { name: 'shell', summary: 'cargo test --quiet', ok: false, result: '3 tests failed' },
  { name: 'shell', summary: 'pytest -q | head', ok: true, result: 'no tests ran' },
  { name: 'shell', summary: 'cargo build', ok: null, result: '' },
];

const ACTIVITY = {
  'apptest-advertised-surface': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 40,
    full_thinking: 'I should check the advertised surface first.',
    calls: CALLS,
    tool_calls: 51,
    recent: ['shell ok', 'shell ERR', 'shell ok', 'shell ok', 'shell ok', 'shell ok'],
    last_text: '.',
    full_transcript: `Checking the surface.${'\n'.repeat(13)}Done.${'\n'.repeat(9)}`,
  },
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

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
  return screen.findByRole('dialog');
};

describe('the node inspector on a lane that ACTS instead of narrating', () => {
  beforeEach(() => {
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-work',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: { 'apptest-advertised-surface': Date.now() },
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

  it('renders the COMMANDS the lane ran, which the pane never showed', async () => {
    // CATCHES the defect itself: the body used to be `[...recent, said]`, so a lane with 51 records showed
    // "shell ok" six times and one character of text. `summary` is the command and it was never rendered.
    const dialog = await openInspector();
    expect(dialog.textContent).toContain('ls -la');
    expect(dialog.textContent).toContain('cargo test --quiet');
    expect(dialog.textContent).toContain('cargo build');
  });

  it('stops rendering the information-free "shell ok" summaries', async () => {
    const dialog = await openInspector();
    expect(dialog.textContent).not.toContain('shell ok');
    expect(dialog.textContent).not.toContain('shell ERR');
  });

  it('auto-opens the LYING-GREEN call and shows its output, not the app-error above it', async () => {
    // CATCHES: exit 0 while the output proves nothing ran is the one row a reader must not have to hunt
    // for; the app-error is the worker testing and must not steal the auto-open.
    const dialog = await openInspector();
    expect(dialog.textContent).toContain('no tests ran');
    expect(dialog.textContent).not.toContain('3 tests failed');
  });

  it('captions the pane with a count that matches the rows, and labels the window', async () => {
    // CATCHES: "42 tool calls" over a 51-record body. The header now names both numbers explicitly.
    const dialog = await openInspector();
    expect(dialog.textContent).toContain('last 4 of 51 tool calls');
    expect(dialog.textContent).toContain('1 ok');
    expect(dialog.textContent).toContain('1 ran nothing');
    expect(dialog.textContent).toContain('1 running');
  });

  it('numbers each row against the whole history', async () => {
    const dialog = await openInspector();
    expect(dialog.textContent).toContain('#49');
    expect(dialog.textContent).toContain('#51');
  });

  it('keeps the work column even though this lane wrote one character of prose', async () => {
    // CATCHES the trap in the change: with `recent` gone, the old `outText` predicate makes narration ''
    // and collapses the column — taking all 51 calls with it.
    const dialog = await openInspector();
    const grid = dialog.querySelector('.grid');
    expect(grid?.className).toContain('lg:grid-cols-2');
  });

  it('squeezes the log’s blank runs instead of scrolling follow onto nothing', async () => {
    const dialog = await openInspector();
    expect(dialog.textContent).toContain('Checking the surface.');
    expect(dialog.textContent).toContain('blank lines collapsed');
  });
});

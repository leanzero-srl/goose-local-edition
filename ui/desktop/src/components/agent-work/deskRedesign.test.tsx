import { act, fireEvent, render as renderBase, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import { assertStudioClean } from '../lz/assertStudioClean';
import {
  foldDesk,
  sourceUrls,
  tickPhases,
  tickReport,
  type AgentEvent,
  type AgentWorkRead,
  type DeskState,
  type TickRecord,
} from './agentWorkModel';
import { DeskHero, type DeskControls } from './DeskHero';
import { TickAnatomy } from './TickAnatomy';
import { AgentResults } from './AgentResults';
import { NeedsYou } from './NeedsYou';
import AgentWorkView from './AgentWorkView';

function render(ui: React.ReactNode) {
  return renderBase(<IntlProvider locale="en">{ui}</IntlProvider>);
}

const NOW = Date.parse('2026-09-20T09:00:00Z');
const URL_MDN = 'https://developer.mozilla.org/en-US/docs/Web/JavaScript';

/** The events of the real "Public web research" tick 1 (run.jsonl, 2026-09-20), trimmed. */
const TICK1_EVENTS: AgentEvent[] = [
  { event: 'tick_started', tick: 1, ts: '2026-09-20T08:32:04.431Z' },
  { event: 'tick_phase', tick: 1, phase: 'guard', ts: '2026-09-20T08:32:04.431Z' },
  { event: 'tick_phase', tick: 1, phase: 'poll', ts: '2026-09-20T08:32:04.431Z' },
  { event: 'poll_absent', tick: 1, ts: '2026-09-20T08:32:04.431Z' },
  { event: 'tick_phase', tick: 1, phase: 'orient', ts: '2026-09-20T08:32:04.432Z' },
  { event: 'orient_done', tick: 1, ts: '2026-09-20T08:32:56.039Z', summary: 'one research lane' },
  { event: 'tick_phase', tick: 1, phase: 'lanes', ts: '2026-09-20T08:32:56.039Z' },
  {
    event: 'lane_dispatched',
    tick: 1,
    key: 't1-charter-mdn-js-read',
    surgeon: 'researcher',
    item: URL_MDN,
    model: 'goose-agent-demo',
    ts: '2026-09-20T08:32:56.039Z',
  },
  {
    event: 'lane_done',
    tick: 1,
    key: 't1-charter-mdn-js-read',
    secs: 130.5,
    confidence: 3,
    ts: '2026-09-20T08:35:06.541Z',
  },
  { event: 'tick_phase', tick: 1, phase: 'handoff', ts: '2026-09-20T08:35:06.541Z' },
  { event: 'synthesis_not_needed', tick: 1, ts: '2026-09-20T08:35:06.541Z' },
  { event: 'tick_phase', tick: 1, phase: 'post', ts: '2026-09-20T08:35:06.542Z' },
  { event: 'tick_phase', tick: 1, phase: 'close', ts: '2026-09-20T08:35:06.542Z' },
  { event: 'tick_done', tick: 1, outcome: 'done', ts: '2026-09-20T08:35:06.543Z' },
];

const TICK1: TickRecord = {
  tick: 1,
  outcome: 'done',
  summary: 'Delivered the read-only report from lane charter-mdn-js-read without rewriting it.',
  lane_secs: 182.1,
  wall_secs: 182.1,
  orient: {
    summary: 'one research lane',
    lanes: [
      {
        id: 'charter-mdn-js-read',
        surgeon: 'researcher',
        item: URL_MDN,
        objective: 'Report the exact title, purpose and source URL.',
        kind: 'research',
      },
    ],
    asks: [],
    drop: [],
  },
  lanes: [
    {
      key: 't1-charter-mdn-js-read',
      surgeon: 'researcher',
      item: URL_MDN,
      model: 'goose-agent-demo',
      secs: 130.5,
      finding: `The page's exact title is "JavaScript | MDN". Source URL: ${URL_MDN}`,
      homework: 'Fetched the page with the page extraction tool, then confirmed with curl.',
      evidence: [
        `${URL_MDN} (get-single-web-page-content, 543 words extracted)`,
        "curl -sL ... | grep -oE '<title>[^<]*</title>' -> <title>JavaScript | MDN</title>",
      ],
      next_step: 'No further action needed unless a deeper section is requested.',
      confidence: 3,
    },
  ],
  synthesis: {
    source: { mode: 'lane_report', lane: 'charter-mdn-js-read', key: 't1-charter-mdn-js-read' },
    staged: [],
    asks: [],
    facts: [],
    log_line: 'Delivered.',
    handoff: 'LANE charter-mdn-js-read — the raw handoff',
    pending: [],
  },
  posted: [],
};

const TICK2: TickRecord = {
  tick: 2,
  outcome: 'done',
  summary: 'Second run.',
  synthesis: {
    source: { mode: 'model' },
    staged: [],
    asks: [],
    facts: [],
    log_line: 'Second run.',
    handoff: 'The second tick found the Guide index.',
    pending: [],
  },
};

const state: DeskState = {
  status: 'stopped',
  agent: 'web-research',
  title: 'Public web research',
  pid: null,
  run_id: 'r',
  tick: 2,
  phase: 'idle',
  phase_started_at: null,
  next_tick_at: null,
  next_tick_reason: 'ran once',
  next_tick_local: 'Sun 08:32 UTC',
  cadence: '30m',
  timezone: 'UTC',
  window_open: true,
  last_tick: null,
  lanes_planned: 1,
  lanes_done: 1,
  hold_reason: null,
  decisions_applied: 0,
  planner_model: 'goose-agent-demo',
  devices: [{ id: 'demo', model_id: 'goose-agent-demo', weight: 1, supervision: false }],
  updated_at: '',
};

const read: AgentWorkRead = {
  dir: '/Users/someone/goose-agents/public-web-handoff-source-deterministic',
  manifest: { name: 'web-research', title: 'Public web research', cadence: '30m' },
  state,
  pid: null,
  heartbeatMs: null,
  events: TICK1_EVENTS,
  ticks: [TICK2, TICK1],
  lanes: {
    't1-charter-mdn-js-read': { model: 'goose-agent-demo', last_text: 'raw tail', tool_calls: 7 },
  },
  laneMtimes: {},
  prepared: [],
  asks: [],
  ledger: { kinds: { tick: [{ tick: 1, lanes: 1, lane_secs: 182 }, { tick: 2 }] } },
  scratchpad: '',
  pending: '',
  dailyLog: '',
  engineLog: '',
  now: NOW,
};

function controls(): DeskControls {
  return {
    busy: false,
    onStart: vi.fn(),
    onRunOnce: vi.fn(),
    onStop: vi.fn(),
    onTickNow: vi.fn(),
    onPause: vi.fn(),
  };
}

describe('tick anatomy — phases from the engine events, never guessed', () => {
  it('times every entered phase, names handoff, marks review skipped and carries the poll note', () => {
    const spans = tickPhases(TICK1_EVENTS, 1, {
      live: false,
      now: NOW,
      phase: 'idle',
      phaseElapsedMs: null,
    })!;
    const by = Object.fromEntries(spans.map((s) => [s.key, s]));
    expect(spans.map((s) => s.key)).toEqual([
      'guard',
      'poll',
      'orient',
      'lanes',
      'review',
      'handoff',
      'post',
      'close',
    ]);
    expect(by.orient.ms).toBe(51_607);
    expect(by.lanes.ms).toBe(130_502);
    expect(by.lanes.state).toBe('done');
    expect(by.review.state).toBe('skipped');
    expect(by.handoff.label).toBe('Handoff');
    expect(by.poll.note).toBe('no poll command');
    expect(spans.some((s) => s.key === 'synthesis')).toBe(false);
  });

  it('a finished tick with no phase events returns null — the view states the absence', () => {
    expect(
      tickPhases([], 3, { live: false, now: NOW, phase: 'idle', phaseElapsedMs: null })
    ).toBeNull();
    const model = foldDesk({ ...read, events: [] }, NOW, 1)!;
    render(<TickAnatomy model={model} onOpenTick={vi.fn()} />);
    expect(screen.getByTestId('phase-ribbon-absent').textContent).toContain('no phase timings');
    expect(screen.queryByTestId('phase-ribbon')).toBeNull();
  });

  it('an interrupted tick ends on an interrupted phase, not a running one', () => {
    const cut = TICK1_EVENTS.filter((e) => e.event !== 'tick_done' && e.phase !== 'handoff');
    const spans = tickPhases(cut.slice(0, 8), 1, {
      live: false,
      now: NOW,
      phase: 'idle',
      phaseElapsedMs: null,
    })!;
    expect(spans.find((s) => s.key === 'lanes')?.state).toBe('interrupted');
    expect(spans.some((s) => s.state === 'live' || s.state === 'next')).toBe(false);
  });

  it('draws the viewed tick with its durations and navigates between ticks', () => {
    const onOpenTick = vi.fn();
    const model = foldDesk(read, NOW, 1)!;
    const { container } = render(<TickAnatomy model={model} onOpenTick={onOpenTick} />);
    assertStudioClean(container);
    expect(container.querySelector('[data-phase="lanes"]')?.textContent).toContain('2m 11s');
    expect(screen.getByTestId('phase-skipped').textContent).toBe('Skipped: Review');
    expect(screen.getByTestId('tick-outcome').textContent).toBe('done');
    fireEvent.click(screen.getByText('Newer'));
    expect(onOpenTick).toHaveBeenCalledWith(null);
  });
});

describe('the viewed tick (?tick=N)', () => {
  it('folds the viewed tick lanes while the queue and nodes stay the current tick', () => {
    const model = foldDesk(read, NOW, 1)!;
    expect(model.viewTick).toBe(1);
    expect(model.tick).toBe(2);
    expect(model.lanes.map((l) => l.key)).toEqual(['t1-charter-mdn-js-read']);
    expect(model.lanes[0].status).toBe('done');
    expect(model.lanes[0].liveLine).toContain('JavaScript | MDN');
    expect(foldDesk(read, NOW)!.lanes).toEqual([]);
  });

  it('shows THAT tick result, with its sources as links and the evidence behind a disclosure', () => {
    const open = vi.fn(async () => {});
    Object.assign(window, { electron: { ...(window.electron ?? {}), openExternal: open } });
    const { container } = render(<AgentResults model={foldDesk(read, NOW, 1)!} />);
    assertStudioClean(container);
    expect(screen.getByTestId('agent-result-lead').textContent).toContain('JavaScript | MDN');
    expect(screen.queryByText('The second tick found the Guide index.')).toBeNull();
    const link = screen.getByTestId('agent-result-sources').querySelector('a')!;
    expect(link.getAttribute('href')).toBe(URL_MDN);
    fireEvent.click(link);
    expect(open).toHaveBeenCalledWith(URL_MDN);
    expect(screen.queryByText(/543 words extracted/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'How it was checked' }));
    expect(screen.getByText(/543 words extracted/)).toBeTruthy();
  });

  it('the latest tick is the default result', () => {
    render(<AgentResults model={foldDesk(read, NOW)!} />);
    expect(screen.getByTestId('agent-result-lead').textContent).toContain(
      'The second tick found the Guide index.'
    );
  });

  it('extracts distinct sources and trims trailing punctuation', () => {
    expect(
      sourceUrls([`see ${URL_MDN}.`, `${URL_MDN} (tool)`, 'http://a.test/x, and more'])
    ).toEqual([URL_MDN, 'http://a.test/x']);
    const r = tickReport(TICK1);
    expect(r.mode).toBe('lane_report');
    expect(r.sources).toEqual([URL_MDN]);
    expect(r.confidence).toBe(3);
  });
});

describe('the desk hero', () => {
  afterEach(() => {
    window.location.hash = '';
  });

  it('a stopped desk: solid stopped status, Start schedule is the one primary, nothing waits', () => {
    const model = foldDesk(read, NOW)!;
    const { container } = render(
      <DeskHero
        model={model}
        title="Public web research"
        schedule="every 30m"
        dir={read.dir}
        plannerModel="goose-agent-demo"
        controls={controls()}
        onRemove={vi.fn()}
        onReviewNeeds={vi.fn()}
      />
    );
    assertStudioClean(container);
    expect(screen.getByTestId('desk-status').getAttribute('data-tone')).toBe('stopped');
    const primaries = container.querySelectorAll(
      '[data-testid="desk-actions"] [data-variant="primary"]'
    );
    expect([...primaries].map((b) => b.textContent)).toEqual(['Start schedule']);
    expect(screen.getByTestId('needs-you-quiet').textContent).toBe('Nothing waits on you');
    expect(screen.queryByTestId('needs-you-banner')).toBeNull();
    expect(screen.getByTestId('desk-path').textContent).toBe(
      'goose-agents/public-web-handoff-source-deterministic'
    );
  });

  it('a waiting desk with an open ask: Tick now is primary and the banner counts what waits', () => {
    const onReviewNeeds = vi.fn();
    const model = foldDesk(
      {
        ...read,
        pid: 9,
        heartbeatMs: NOW,
        state: { ...state, status: 'waiting', next_tick_at: new Date(NOW + 60_000).toISOString() },
        asks: [
          {
            id: 'a1',
            tick: 1,
            question: 'Guide or Reference?',
            why: '',
            status: 'open',
            raised_at: '',
          },
        ],
      },
      NOW
    )!;
    const { container } = render(
      <DeskHero
        model={model}
        title="Public web research"
        schedule="every 30m"
        dir={read.dir}
        plannerModel=""
        controls={controls()}
        onRemove={vi.fn()}
        onReviewNeeds={onReviewNeeds}
      />
    );
    const primaries = container.querySelectorAll(
      '[data-testid="desk-actions"] [data-variant="primary"]'
    );
    expect([...primaries].map((b) => b.textContent)).toEqual(['Tick now']);
    expect(screen.getByTestId('needs-you-banner').textContent).toContain('1 question to answer');
    expect(screen.queryByTestId('needs-you-quiet')).toBeNull();
    fireEvent.click(screen.getByTestId('needs-you-banner'));
    expect(onReviewNeeds).toHaveBeenCalled();
  });

  it('needs-you renders nothing when nothing waits', () => {
    const { container } = render(
      <NeedsYou model={foldDesk(read, NOW)!} onDecide={vi.fn()} requiresApproval hasPostCommand />
    );
    expect(container.textContent).toBe('');
  });
});

describe('the view wires ?tick from the URL', () => {
  it('opens the tick the sidebar named, and the scroller is a block (panels never shrink)', async () => {
    window.location.hash = `#/agent-work?desk=${encodeURIComponent(read.dir)}&tick=1`;
    Object.assign(window, {
      electron: {
        ...(window.electron ?? {}),
        agentWorkList: vi.fn(async () => [
          {
            dir: read.dir,
            addedAt: '',
            manifest: read.manifest,
            state,
            pid: null,
            heartbeatMs: null,
            exists: true,
          },
        ]),
        agentWorkRead: vi.fn(async () => read),
      },
    });
    render(<AgentWorkView />);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(screen.getByTestId('agent-result-lead').textContent).toContain('JavaScript | MDN');
    expect(screen.getByLabelText('Tick 1')).toBeTruthy();
    expect(screen.getByTestId('agent-work-scroll').className.split(' ')).not.toContain('flex');
    window.location.hash = '';
  });
});

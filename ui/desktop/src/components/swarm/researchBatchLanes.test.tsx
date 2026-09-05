import { render, renderHook, waitFor, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import {
  buildActivity,
  buildPhaseTodo,
  foldEvents,
  researchLaneLabel,
  resetFoldCache,
  resetLiveChannelMemory,
  useSwarmRun,
} from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * VA-029 — RESEARCH LANES UNDER THE FAN CUT (C3, 8d6a4eb7c): ONE LANE PER BATCH, ITS QUESTIONS LISTED.
 *
 * The engine now dispatches a slice's questions as ONE lane (`activity_key: research-<slice>`, one per
 * slice with anything left to ask, +1 for the open decisions) and still emits the per-question events:
 * `research_dispatched` once per question with `batch` (how many ride the lane) and `activity_key` (the
 * lane), `research_answered` / `research_unanswered` naming slice + q_index. `research_planned` carries
 * `facts` — questions the opener settled as cited spec facts, answered with NO lane — and `lanes`.
 * Shapes below are verbatim from research.rs `emit_research_planned` / `emit_research_outcome` and the
 * swarm.rs dispatch site (read 2026-09-01); the OLD shape is the r6d archive's own line.
 *
 * Synthetic new-shape stream: 6 lanes, 22 questions, facts 13.
 */

const ts = '2026-09-01T10:00:00Z';
const POOL = [
  { id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 },
  { id: 'local-gabee-qwen3.6-27b', model_id: 'gabee-qwen3.6-27b', weight: 2 },
];
const START = { event: 'run_started', ts, pool: POOL };

const PLANNED = {
  event: 'research_planned',
  questions: 22,
  dispatching: 22,
  resumed: 0,
  facts: 13,
  lanes: 6,
  per_slice: {
    __open_decisions__: 3,
    'drafts-workflow': 4,
    'ledger-api': 4,
    'ledger-core': 5,
    notifierd: 3,
    'web-page': 3,
  },
};

const BATCHES: Array<{ slice: string; n: number; model: string }> = [
  { slice: 'ledger-core', n: 5, model: 'mihai-qwen3.6-27b' },
  { slice: 'ledger-api', n: 4, model: 'gabee-qwen3.6-27b' },
  { slice: 'drafts-workflow', n: 4, model: 'mihai-qwen3.6-27b' },
  { slice: 'notifierd', n: 3, model: 'gabee-qwen3.6-27b' },
  { slice: 'web-page', n: 3, model: 'mihai-qwen3.6-27b' },
  { slice: '__open_decisions__', n: 3, model: 'gabee-qwen3.6-27b' },
];

const dispatched = (slice: string, q: number, model: string, batch: number) => ({
  event: 'research_dispatched',
  slice,
  q_index: q,
  question: `[${slice}] question ${q}: what does the request fix for this slice?`,
  model,
  activity_key: `research-${slice}`,
  batch,
});
const answered = (slice: string, q: number, model: string, batch: number, chars: number) => ({
  event: 'research_answered',
  slice,
  q_index: q,
  chars,
  raised: q === 0 ? 1 : 0,
  secs: 412,
  batch,
  model,
});
const unanswered = (slice: string, q: number, model: string) => ({
  event: 'research_unanswered',
  slice,
  q_index: q,
  reason: 'lane_panicked',
  detail: 'task panicked at join',
  secs: 0,
  model,
});

const DISPATCHES = BATCHES.flatMap((b) =>
  Array.from({ length: b.n }, (_, q) => dispatched(b.slice, q, b.model, b.n))
);
// ledger-core: every question answered. ledger-api: 2 of 4 settled. notifierd: every question lost.
const OUTCOMES = [
  ...Array.from({ length: 5 }, (_, q) => answered('ledger-core', q, 'mihai-qwen3.6-27b', 5, 1800 + q)),
  answered('ledger-api', 0, 'gabee-qwen3.6-27b', 4, 2100),
  answered('ledger-api', 1, 'gabee-qwen3.6-27b', 4, 950),
  ...Array.from({ length: 3 }, (_, q) => unanswered('notifierd', q, 'gabee-qwen3.6-27b')),
];
const EVENTS = [START, PLANNED, ...DISPATCHES, ...OUTCOMES];

const ACTIVITY = {
  'research-ledger-core': {
    model: 'mihai-qwen3.6-27b',
    phase: 'done',
    thinking_chars: 4000,
    full_transcript: '{"answers":[{"question_index":0,"answer":"…"}]}',
  },
  'research-ledger-api': {
    model: 'gabee-qwen3.6-27b',
    thinking_chars: 900,
    full_thinking: 'reading request.md:218 for the health shape',
  },
};

describe('the fold: one research lane per batch key, questions from the per-question events', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
  });

  it('6 lanes for 6 batches, 22 questions between them — digest or not, the dispatch is the fact', () => {
    const folded = foldEvents(EVENTS as never, ACTIVITY as never, 'vb-029-fold');
    expect(folded.researchLanes.map((l) => l.taskId)).toEqual(
      [
        'research-__open_decisions__',
        'research-drafts-workflow',
        'research-ledger-api',
        'research-ledger-core',
        'research-notifierd',
        'research-web-page',
      ]
    );
    const total = folded.researchLanes.reduce((n, l) => n + (l.researchQuestions?.length ?? 0), 0);
    expect(total).toBe(22);
    // No per-question lane exists any more — the r6d shape would have minted 22 of them.
    expect(folded.researchLanes.some((l) => /-q\d+$/.test(l.taskId))).toBe(false);
    expect(folded.planningLanes.some((l) => l.taskId.startsWith('research-'))).toBe(false);
  });

  it('a lane lists its questions numbered by q_index, with each outcome, and its caption counts them', () => {
    const folded = foldEvents(EVENTS as never, ACTIVITY as never, 'vb-029-rows');
    const core = folded.researchLanes.find((l) => l.taskId === 'research-ledger-core')!;
    expect(core.description).toBe('Research ledger-core · 5 questions in one read-only session');
    expect(core.researchQuestions?.map((q) => q.qIndex)).toEqual([0, 1, 2, 3, 4]);
    expect(core.researchQuestions?.[2]).toMatchObject({
      slice: 'ledger-core',
      qIndex: 2,
      question: '[ledger-core] question 2: what does the request fix for this slice?',
      status: 'answered',
      chars: 1802,
      secs: 412,
    });
    expect(core.researchQuestions?.[0].raised).toBe(1);
    // The decisions pseudo-slice reads as words a person knows.
    const decisions = folded.researchLanes.find((l) => l.taskId === 'research-__open_decisions__')!;
    expect(decisions.description).toBe('Research open decisions · 3 questions in one read-only session');
    expect(decisions.device).toBe('gabee');
  });

  it('status is the truth layer: settled → done, partial → running, every question lost → error', () => {
    const folded = foldEvents(EVENTS as never, ACTIVITY as never, 'vb-029-status');
    const by = (k: string) => folded.researchLanes.find((l) => l.taskId === k)!;
    expect(by('research-ledger-core').status).toBe('done');
    expect(by('research-ledger-api').status).toBe('running');
    expect(by('research-ledger-api').researchQuestions?.map((q) => q.status)).toEqual([
      'answered',
      'answered',
      'dispatched',
      'dispatched',
    ]);
    // The absence twin: a lane whose every question came back unanswered is not a clean pass.
    const dead = by('research-notifierd');
    expect(dead.status).toBe('error');
    expect(dead.researchQuestions?.every((q) => q.status === 'unanswered')).toBe(true);
    expect(dead.researchQuestions?.[0]).toMatchObject({
      reason: 'lane_panicked',
      detail: 'task panicked at join',
    });
    // Dispatched, no outcome yet, no digest yet: running, on the node the dispatch named.
    expect(by('research-web-page').status).toBe('running');
    expect(by('research-web-page').device).toBe('mihai');
  });

  // The OLD shape — the r6d archive's own research_dispatched line (one lane per question, activity_key
  // `research-<slice>-q<n>`, no `batch`) — still renders: one lane per key, one question each, the
  // caption the earlier tests pinned.
  it('the r6d per-question shape renders one lane per question with the pinned caption', () => {
    const old = [
      START,
      {
        event: 'research_dispatched',
        slice: 'ledger-core',
        q_index: 4,
        question:
          "How is 'vendor down' surfaced to /api/health and the UI degraded state, and how long does registration retry before giving up (it must keep retrying)?",
        model: 'mihai-qwen3.6-27b',
        activity_key: 'research-ledger-core-q4',
      },
      {
        event: 'research_dispatched',
        slice: 'ledger-core',
        q_index: 2,
        question: 'Choose the exact 404 envelope code literal',
        model: 'gabee-qwen3.6-27b',
        activity_key: 'research-ledger-core-q2',
      },
      {
        event: 'research_answered',
        slice: 'ledger-core',
        q_index: 4,
        chars: 3206,
        raised: 0,
        secs: 1501,
        model: 'mihai-qwen3.6-27b',
      },
    ];
    const folded = foldEvents(old as never, {} as never, 'vb-029-old');
    expect(folded.researchLanes.map((l) => l.taskId)).toEqual([
      'research-ledger-core-q2',
      'research-ledger-core-q4',
    ]);
    const q4 = folded.researchLanes[1];
    expect(q4.description).toBe('Research ledger-core q4 · one opener question, answered read-only');
    expect(q4.researchQuestions).toHaveLength(1);
    expect(q4.researchQuestions?.[0]).toMatchObject({ qIndex: 4, status: 'answered', chars: 3206 });
    expect(q4.status).toBe('done');
    expect(folded.researchLanes[0].status).toBe('running');
  });

  it('researchLaneLabel: both key shapes, the decisions slice, and no invented count', () => {
    expect(researchLaneLabel('research-ledger-core-q4')).toBe(
      'Research ledger-core q4 · one opener question, answered read-only'
    );
    expect(researchLaneLabel('research-ledger-core', 5)).toBe(
      'Research ledger-core · 5 questions in one read-only session'
    );
    expect(researchLaneLabel('research-__open_decisions__', 1)).toBe(
      'Research open decisions · 1 question in one read-only session'
    );
    // Known only from its digest: no events, no count claimed.
    expect(researchLaneLabel('research-web-page')).toBe('Research · web-page');
  });
});

describe('the feed: one dispatch line per lane, updated as the batch arrives; the plan line names the facts', () => {
  it('a 5-question batch is ONE line naming its questions, not five', () => {
    const { activity, verbose } = buildActivity(EVENTS);
    const lines = activity.filter((r) => r.text.startsWith('Researching ledger-core'));
    expect(lines).toHaveLength(1);
    expect(lines[0].text).toBe('Researching ledger-core · 5 questions (q0, q1, q2, q3, q4)');
    expect(lines[0].sub).toBe('on mihai');
    expect(activity.some((r) => r.text === 'Researching ledger-core q1')).toBe(false);
    // Verbose carries the numbered questions themselves, one per line.
    const v = verbose.find((r) => r.text.startsWith('Researching ledger-core'))!;
    expect(v.sub?.split('\n')[0]).toBe('on mihai');
    expect(v.sub).toContain('[q0] [ledger-core] question 0');
    expect(v.sub).toContain('[q4] [ledger-core] question 4');
    // Six lanes, six lines.
    expect(activity.filter((r) => r.text.startsWith('Researching ')).length).toBe(6);
    expect(
      activity.some((r) => r.text === 'Researching open decisions · 3 questions (q0, q1, q2)')
    ).toBe(true);
  });

  it('research_planned is a line: questions across lanes, and the facts settled with no lane', () => {
    const { activity } = buildActivity([START, PLANNED]);
    const row = activity.find((r) => r.text.startsWith('Research planned'))!;
    expect(row.text).toBe('Research planned — 22 questions across 6 lanes');
    expect(row.sub).toContain('13 answered from the spec (no lane)');
    expect(row.sub).toContain('open decisions 3');
    expect(row.sub).toContain('ledger-core 5');
  });

  it("the r6d shape's plan line claims no facts and no lanes it did not carry", () => {
    const { activity } = buildActivity([
      START,
      {
        event: 'research_planned',
        questions: 38,
        dispatching: 38,
        resumed: 0,
        per_slice: { __open_decisions__: 3, 'ledger-core': 6 },
      },
    ]);
    const row = activity.find((r) => r.text.startsWith('Research planned'))!;
    expect(row.text).toBe('Research planned — 38 questions');
    expect(row.sub).toBe('open decisions 3 · ledger-core 6');
  });
});

describe('the Research checklist: the header carries the facts; a fact-only queue is not "scoped — 0"', () => {
  const research = (events: Array<Record<string, unknown>>) =>
    buildPhaseTodo(events, {}, { clarifyPending: false }).find((p) => p.key === 'research')!;

  it('the phase note reads the facts count in the words a person needs', () => {
    const p = research(EVENTS);
    expect(p.note).toBe('13 answered from the spec (no lane)');
    // The rows still count the lanes' questions — the facts are outside them.
    expect(p.items.map((i) => i.label)).toContain('Researching — 10 of 22 questions settled');
    expect(p.items.find((i) => i.label.includes('unanswered'))?.detail).toBe('lane panicked ×3');
  });

  it('a queue settled entirely from facts/ledger is a done row, never the legacy zero', () => {
    const p = research([
      START,
      { event: 'research_planned', questions: 13, dispatching: 0, resumed: 0, facts: 13, lanes: 0 },
    ]);
    expect(p.items.map((i) => i.label)).toEqual([
      'Research — nothing to dispatch: 13 answered from the spec',
    ]);
    expect(p.state).toBe('done');
    expect(p.note).toBe('13 answered from the spec (no lane)');
    expect(p.items.some((i) => i.label.includes('scoped — 0'))).toBe(false);
  });

  it('between the queue event and the first dispatch the row is running with the queue denominator', () => {
    const p = research([START, PLANNED]);
    expect(p.items.map((i) => i.label)).toEqual([
      'Research planned — 22 questions queued across 6 lanes',
    ]);
    expect(p.state).toBe('running');
  });

  it("the v1 scout-era shape (a questions ARRAY) still takes the legacy row, and r6d's has no facts", () => {
    const v1 = research([START, { event: 'research_planned', questions: ['a', 'b'] }]);
    expect(v1.items.map((i) => i.label)).toContain('Research questions scoped — 2');
    const r6d = research([
      START,
      { event: 'research_planned', questions: 38, dispatching: 38, resumed: 0, per_slice: {} },
    ]);
    expect(r6d.note).toBeUndefined();
    expect(r6d.items.map((i) => i.label)).toEqual(['Research planned — 38 questions queued']);
  });
});

// ── Rendered: the Research header, the fan group header, and one lane's numbered questions. ──

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('the planning zone renders one lane per batch with its questions, and the facts in the header', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const now = new Date(Date.now() - 60_000).toISOString();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-fan-cut',
      dir: '/tmp/build',
      events: EVENTS.map((ev) => ({ ...ev, ts: now })),
      activity: ACTIVITY,
      activityMtimes: { 'research-ledger-core': Date.now(), 'research-ledger-api': Date.now() },
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

  it('6 lanes · 22 questions under Research, "13 answered from the spec (no lane)" beside the counts', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(result.current.researchLanes).toHaveLength(6);

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );

    const phase = await screen.findByTestId('planning-phase-research');
    const text = phase.textContent ?? '';
    expect(screen.getByTestId('planning-phase-research-note').textContent).toContain(
      '13 answered from the spec (no lane)'
    );
    expect(text).toContain('Research answers · 6 lanes · 22 questions');
    // One row per lane — six, not twenty-two.
    expect(phase.querySelectorAll('[data-testid="turn-lane"]')).toHaveLength(6);
    // Running lanes open by default: ledger-api's four questions are on screen, numbered, each with
    // its own outcome; the dead notifierd lane's three read unanswered, never as a pass.
    const questionRows = phase.querySelectorAll('[data-testid="research-question"]');
    expect(questionRows.length).toBeGreaterThanOrEqual(4);
    const api = [...phase.querySelectorAll('[data-testid="turn-lane"]')].find((el) =>
      (el.textContent ?? '').includes('Research ledger-api')
    )!;
    expect(api.textContent).toContain('Questions · 4 · 2 answered · 2 open');
    expect(api.querySelectorAll('[data-status="answered"]')).toHaveLength(2);
    expect(api.querySelectorAll('[data-status="dispatched"]')).toHaveLength(2);
    expect(api.textContent).toContain('q3');
    expect(api.textContent).toContain('[ledger-api] question 3');
    expect(api.textContent).toContain('2,100 chars · 1 raised');
  });
});

import { cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';
import {
  buildPhaseTodo,
  foldEvents,
  resetFoldCache,
  resetLiveChannelMemory,
  useSwarmRun,
} from './useSwarmRun';

/**
 * VA-143 — VA-138's two residues, against r6j's OWN research stream (benchmark/runs/build/swarm-3node-r0
 * run.jsonl, read 2026-09-02: 6 lanes, 37 landings, 6 `remainder_empty` closers, 49 builder_decides).
 *
 * (1) A research lane's closer — `research_unanswered{reason: remainder_empty, detail: "N question(s)
 *     landed through research_answer; the final reply added none and listed M builder_decides"}` — was
 *     folded as a question row, so r6j's web-viz lane (landed 4) read "q4 · unanswered · remainder
 *     empty" and "1 unanswered" for the whole run. It is the lane's CLOSE, never a miss.
 * (2) `research_answer_landed{task, slice, q_index, kind, status, chars, raised, via}` — the mid-lane
 *     landing the engine emits per tool call — reached no feed line and no lane count.
 *
 * Every number below is the run's: dispatch order, kinds, chars, raised, secs, timestamps.
 */
type Ev = Record<string, unknown>;

const POOL = [
  { id: 'mac-gabee-qwen3.8-27b', model_id: 'gabee-qwen3.8-27b', weight: 1, speed_weight: 1 },
  { id: 'local-mihai-qwen3.8-27b', model_id: 'mihai-qwen3.8-27b', weight: 1, speed_weight: 2 },
  {
    id: 'worksmacstudio-workhorse-qwen3.8-27b',
    model_id: 'workhorse-qwen3.8-27b',
    weight: 1,
    speed_weight: 3,
  },
];
const HOST: Record<string, string> = {
  'web-viz': 'workhorse-qwen3.8-27b',
  'web-page': 'workhorse-qwen3.8-27b',
  'ledgerd-webhooks-drafts': 'workhorse-qwen3.8-27b',
  'ledgerd-core': 'gabee-qwen3.8-27b',
  notifierd: 'gabee-qwen3.8-27b',
  'ledgerd-api': 'mihai-qwen3.8-27b',
};
const at = (ts: string, e: Ev): Ev => ({ ...e, ts });

// [ts, slice, rank, sections]
const ORDER: Array<[string, string, number, number]> = [
  ['2026-09-02T16:28:14.135570+00:00', 'ledgerd-core', 2, 7],
  ['2026-09-02T16:28:14.135685+00:00', 'web-viz', 0, 10],
  ['2026-09-02T16:28:14.135791+00:00', 'ledgerd-api', 1, 9],
  ['2026-09-02T17:02:23.220768+00:00', 'web-page', 3, 4],
  ['2026-09-02T17:24:46.898779+00:00', 'ledgerd-webhooks-drafts', 4, 3],
  ['2026-09-02T17:36:35.638859+00:00', 'notifierd', 5, 1],
];
// [ts, slice, q_index, kind, chars, raised, secs] — the 37 landings, in the order they landed.
const LANDINGS: Array<[string, string, number, string, number, number, number]> = [
  ['2026-09-02T16:49:25.155494+00:00', 'web-viz', 0, 'design', 2007, 1, 1270],
  ['2026-09-02T16:50:08.544444+00:00', 'web-viz', 1, 'design', 569, 1, 1314],
  ['2026-09-02T16:51:15.639053+00:00', 'web-viz', 2, 'design', 890, 1, 1381],
  ['2026-09-02T16:59:21.531999+00:00', 'web-viz', 3, 'design', 1181, 1, 1867],
  ['2026-09-02T17:21:55.379061+00:00', 'web-page', 0, 'design', 2860, 1, 1172],
  ['2026-09-02T17:25:49.456304+00:00', 'ledgerd-core', 0, 'external', 636, 0, 3455],
  ['2026-09-02T17:25:49.457212+00:00', 'ledgerd-core', 1, 'external', 1349, 0, 3455],
  ['2026-09-02T17:28:59.742512+00:00', 'ledgerd-core', 2, 'external', 674, 0, 3645],
  ['2026-09-02T17:28:59.744106+00:00', 'ledgerd-core', 3, 'external', 458, 0, 3645],
  ['2026-09-02T17:28:59.746188+00:00', 'ledgerd-core', 4, 'external', 632, 0, 3645],
  ['2026-09-02T17:30:18.589610+00:00', 'ledgerd-core', 5, 'external', 956, 1, 3724],
  ['2026-09-02T17:30:18.590534+00:00', 'ledgerd-core', 6, 'external', 1150, 1, 3724],
  ['2026-09-02T17:33:21.718032+00:00', 'ledgerd-core', 7, 'design', 3187, 1, 3907],
  ['2026-09-02T17:33:21.723511+00:00', 'ledgerd-core', 8, 'design', 1399, 0, 3907],
  ['2026-09-02T17:33:21.725233+00:00', 'ledgerd-core', 9, 'design', 1101, 1, 3907],
  ['2026-09-02T17:35:32.180003+00:00', 'ledgerd-core', 10, 'design', 1770, 2, 4038],
  ['2026-09-02T17:35:32.181909+00:00', 'ledgerd-core', 11, 'design', 1114, 2, 4038],
  ['2026-09-02T18:03:42.050827+00:00', 'notifierd', 0, 'design', 1172, 1, 1626],
  ['2026-09-02T18:04:31.289672+00:00', 'notifierd', 1, 'design', 566, 1, 1675],
  ['2026-09-02T18:10:33.301724+00:00', 'ledgerd-webhooks-drafts', 0, 'external', 908, 0, 2746],
  ['2026-09-02T18:10:33.303206+00:00', 'ledgerd-webhooks-drafts', 1, 'external', 719, 0, 2746],
  ['2026-09-02T18:11:54.395071+00:00', 'ledgerd-webhooks-drafts', 2, 'external', 821, 1, 2827],
  ['2026-09-02T18:11:54.396298+00:00', 'ledgerd-webhooks-drafts', 3, 'external', 1045, 0, 2827],
  ['2026-09-02T18:13:08.327733+00:00', 'ledgerd-webhooks-drafts', 4, 'design', 1151, 0, 2901],
  ['2026-09-02T18:13:08.328990+00:00', 'ledgerd-webhooks-drafts', 5, 'design', 1037, 0, 2901],
  ['2026-09-02T18:13:43.030743+00:00', 'ledgerd-webhooks-drafts', 6, 'design', 657, 0, 2936],
  ['2026-09-02T18:14:11.979091+00:00', 'ledgerd-webhooks-drafts', 7, 'design', 470, 0, 2965],
  ['2026-09-02T18:39:31.183877+00:00', 'ledgerd-api', 0, 'design', 1723, 0, 7876],
  ['2026-09-02T18:43:17.642065+00:00', 'ledgerd-api', 1, 'design', 1005, 0, 8103],
  ['2026-09-02T18:44:12.649534+00:00', 'ledgerd-api', 2, 'design', 1268, 0, 8158],
  ['2026-09-02T18:45:14.876214+00:00', 'ledgerd-api', 3, 'design', 1165, 0, 8220],
  ['2026-09-02T18:46:31.068827+00:00', 'ledgerd-api', 4, 'design', 2381, 0, 8296],
  ['2026-09-02T18:47:39.991583+00:00', 'ledgerd-api', 5, 'design', 1826, 0, 8365],
  ['2026-09-02T18:48:26.469990+00:00', 'ledgerd-api', 6, 'design', 662, 0, 8412],
  ['2026-09-02T18:49:12.885447+00:00', 'ledgerd-api', 7, 'design', 869, 0, 8458],
  ['2026-09-02T18:50:10.906698+00:00', 'ledgerd-api', 8, 'design', 847, 1, 8516],
  ['2026-09-02T18:51:06.411015+00:00', 'ledgerd-api', 9, 'design', 957, 0, 8572],
];
// [ts, slice, next q_index, secs, builder_decides] — the closers, each followed by its decides.
const CLOSERS: Array<[string, string, number, number, number]> = [
  ['2026-09-02T17:02:23.220532+00:00', 'web-viz', 4, 2049, 11],
  ['2026-09-02T17:24:46.898600+00:00', 'web-page', 1, 1343, 9],
  ['2026-09-02T17:36:35.638749+00:00', 'ledgerd-core', 12, 4101, 4],
  ['2026-09-02T18:05:28.588032+00:00', 'notifierd', 2, 1732, 10],
  ['2026-09-02T18:14:50.362083+00:00', 'ledgerd-webhooks-drafts', 8, 3003, 7],
  ['2026-09-02T18:52:32.529087+00:00', 'ledgerd-api', 10, 8658, 8],
];
const LANDED: Record<string, number> = {
  'web-viz': 4,
  'web-page': 1,
  'ledgerd-core': 12,
  notifierd: 2,
  'ledgerd-webhooks-drafts': 8,
  'ledgerd-api': 10,
};

const dispatch = ([ts, slice]: (typeof ORDER)[number]): Ev[] => [
  at(ts, {
    event: 'research_dispatched',
    slice,
    derives: true,
    q_indexes: [],
    model: HOST[slice],
    activity_key: `research-${slice}`,
  }),
];
/** The two events one landing emits, in the engine's order: kind → funnel. */
const landing = ([ts, slice, q, kind, chars, raised, secs]: (typeof LANDINGS)[number]): Ev[] => {
  const kindEv = at(ts, {
    event: 'research_question_kind',
    slice,
    q_index: q,
    kind,
    source: 'model',
    model_kind: null,
    cite: 'request.md',
    question: `[${slice}] q${q}`,
  });
  const funnel = at(ts, {
    event: 'research_answered',
    slice,
    q_index: q,
    chars,
    raised,
    secs,
    batch: 0,
    model: HOST[slice],
  });
  return [kindEv, funnel];
};
const closer = ([ts, slice, next, secs, decides]: (typeof CLOSERS)[number]): Ev[] => [
  at(ts, {
    event: 'research_unanswered',
    slice,
    q_index: next,
    reason: 'remainder_empty',
    detail: `${next} question(s) landed through research_answer; the final reply added none and listed ${decides} builder_decides`,
    secs,
    model: HOST[slice],
  }),
];

/** Microsecond order — `Date.parse` drops the run's microseconds, and the web-viz closer (.220532)
 *  and the web-page dispatch (.220768) share a millisecond. */
const usOf = (e: Ev): number => {
  const ts = e['ts'] as string;
  return Date.parse(ts.replace(/\.\d+/, '')) * 1000 + Number(ts.match(/\.(\d{6})/)![1]);
};
/** r6j's research stream, every event at its own timestamp. */
const r6jResearch = (): Ev[] =>
  [...ORDER.flatMap(dispatch), ...LANDINGS.flatMap(landing), ...CLOSERS.flatMap(closer)].sort(
    (a, b) => usOf(a) - usOf(b)
  );

const OPENING: Ev[] = [
  at('2026-09-02T15:36:03.606000+00:00', {
    event: 'run_started',
    prompt: '# Build `app` — Meridian Payments Console',
    pool: POOL,
  }),
  at('2026-09-02T15:36:03.640000+00:00', { event: 'pool_resolved', devices: POOL }),
  at('2026-09-02T15:36:04.206822+00:00', { event: 'phase', phase: 'open' }),
  at('2026-09-02T16:28:14.134042+00:00', {
    event: 'research_planned',
    lanes: 6,
    per_slice_sections: { 'ledgerd-api': 9, 'ledgerd-core': 7, 'ledgerd-webhooks-drafts': 3, notifierd: 1, 'web-page': 4, 'web-viz': 10 },
    resumed_slices: [],
    decisions: 0,
  }),
  at('2026-09-02T16:28:14.134086+00:00', { event: 'phase', phase: 'research' }),
];
const R6J: Ev[] = [...OPENING, ...r6jResearch()];

const lanesOf = (events: Ev[], scope: string) =>
  Object.fromEntries(foldEvents(events, {}, scope).researchLanes.map((l) => [l.taskId, l]));

describe('VA-143 (1) — a remainder_empty closer is the lane’s close, never a miss', () => {
  it('no lane on r6j shows an unanswered row; every lane closes with what it landed', () => {
    const lanes = lanesOf(R6J, 'va143-fold');
    expect(Object.keys(lanes).sort()).toEqual(
      Object.keys(LANDED)
        .map((s) => `research-${s}`)
        .sort()
    );
    let rows = 0;
    for (const [slice, n] of Object.entries(LANDED)) {
      const lane = lanes[`research-${slice}`];
      const qs = lane.researchQuestions ?? [];
      rows += qs.length;
      expect(qs.some((q) => q.status === 'unanswered'), `${slice} shows a miss`).toBe(false);
      expect(qs.filter((q) => q.status === 'answered')).toHaveLength(n);
      expect(lane.status).toBe('done');
      expect(lane.researchClose).toMatchObject({ reason: 'remainder_empty', landed: n });
    }
    expect(rows).toBe(37);
    // The closer's own words survive on the close (title).
    expect(lanes['research-web-viz'].researchClose).toEqual({
      reason: 'remainder_empty',
      landed: 4,
      secs: 2049,
      detail:
        '4 question(s) landed through research_answer; the final reply added none and listed 11 builder_decides',
    });
    expect(lanes['research-web-viz'].elapsedMs).toBe(2049 * 1000);
    expect(lanes['research-ledgerd-core'].researchClose).toMatchObject({ landed: 12 });
    expect(lanes['research-ledgerd-api'].researchClose).toMatchObject({ landed: 10 });
  });

  it('a genuine reason stays a loud miss, and a lane that derived nothing is not a clean pass', () => {
    const events: Ev[] = [
      ...OPENING,
      ...dispatch(ORDER[1]),
      ...dispatch(ORDER[5]),
      ...landing(LANDINGS[0]),
      at('2026-09-02T16:52:00.000000+00:00', {
        event: 'research_unanswered',
        slice: 'web-viz',
        q_index: 1,
        reason: 'judge_ended',
        detail: 'judge_out_of_moves',
        secs: 1400,
        model: HOST['web-viz'],
      }),
      at('2026-09-02T17:40:00.000000+00:00', {
        event: 'research_unanswered',
        slice: 'notifierd',
        q_index: 0,
        reason: 'no_questions',
        detail: 'the lane read its sections and derived no design or external question',
        secs: 200,
        model: HOST['notifierd'],
      }),
    ];
    const lanes = lanesOf(events, 'va143-miss');
    const viz = lanes['research-web-viz'];
    expect(viz.researchQuestions?.map((q) => `${q.qIndex}:${q.status}`)).toEqual(['0:answered', '1:unanswered']);
    expect(viz.researchQuestions?.[1]).toMatchObject({ reason: 'judge_ended', detail: 'judge_out_of_moves' });
    expect(viz.researchClose).toBeUndefined();
    const dead = lanes['research-notifierd'];
    expect(dead.status).toBe('error');
    expect(dead.researchQuestions?.[0]).toMatchObject({ status: 'unanswered', reason: 'no_questions' });
  });
});

describe('VA-138 — the Research step’s lane rows count what each lane landed', () => {
  it('reads `landed N · <kinds> · closed` on a closed lane and `running` before its closer', () => {
    const research = buildPhaseTodo(R6J, {}, { clarifyPending: false }).find((p) => p.key === 'research')!;
    const detail = (slice: string) => research.items.find((i) => i.id === `r2-lane-${slice}`)!.detail;
    expect(detail('ledgerd-core')).toBe(
      'landed 12 · 5 design, 7 external · closed — the final reply added nothing behind the landed answers'
    );
    expect(detail('web-viz')).toBe(
      'landed 4 · 4 design · closed — the final reply added nothing behind the landed answers'
    );
    const cut = R6J.findIndex((e) => e['event'] === 'research_unanswered' && e['slice'] === 'web-viz');
    const early = buildPhaseTodo(R6J.slice(0, cut), {}, { clarifyPending: false }).find((p) => p.key === 'research')!;
    expect(early.items.find((i) => i.id === 'r2-lane-web-viz')!.detail).toBe('landed 4 · 4 design · running');
  });
});

// ── Rendered: r6j's web-viz lane row. ──

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('VA-143 rendered — r6j’s web-viz lane row', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-3node-r0',
      dir: '/tmp/build',
      events: R6J,
      activity: {
        'research-web-viz': {
          tool_calls: 4,
          last_text: 'four answers landed',
          model: 'workhorse-qwen3.8-27b',
          attempt: 0,
          phase: 'done',
        },
      },
      activityMtimes: {},
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
  afterEach(() => cleanup());

  it('reads "Questions · 4 · 4 answered" and "landed 4 · done 34m" — no unanswered', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(result.current.researchLanes).toHaveLength(6);
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
    const phase = await screen.findByTestId('planning-phase-research');
    // A header counts what the body shows: 37 question rows across the six lanes.
    expect(phase.textContent).toContain('Research answers · 6 lanes · 37 questions');
    const viz = [...phase.querySelectorAll('[data-testid="turn-lane"]')].find((el) =>
      (el.textContent ?? '').includes('Research web-viz')
    ) as HTMLElement;
    expect(viz).toBeTruthy();
    // A done lane is collapsed by default; open it.
    fireEvent.click(viz.querySelector('button')!);
    await waitFor(() => expect(viz.querySelector('[data-testid="research-questions"]')).toBeTruthy());
    expect(viz.textContent).toContain('Questions · 4 · 4 answered');
    expect(viz.textContent).not.toContain('unanswered');
    expect(viz.querySelectorAll('[data-testid="research-question"]')).toHaveLength(4);
    expect(viz.querySelectorAll('[data-status="unanswered"]')).toHaveLength(0);
    const close = viz.querySelector('[data-testid="research-lane-close"]') as HTMLElement;
    expect(close.dataset.reason).toBe('remainder_empty');
    expect(close.textContent).toBe('closedlanded 4 · done 34m');
    expect(close.title).toBe(
      '4 question(s) landed through research_answer; the final reply added none and listed 11 builder_decides'
    );
    // The lane's clock is the closer's own secs.
    expect(viz.textContent).toContain('2049s');
  });

  it('VA-145 (1): ledgerd-core’s 12 landings are rows under its lane with their kinds, and the rows fold', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
    const phase = await screen.findByTestId('planning-phase-research');
    const core = [...phase.querySelectorAll('[data-testid="turn-lane"]')].find((el) =>
      (el.textContent ?? '').includes('Research ledgerd-core')
    ) as HTMLElement;
    expect(core).toBeTruthy();
    fireEvent.click(core.querySelector('button')!);
    await waitFor(() => expect(core.querySelector('[data-testid="research-questions"]')).toBeTruthy());
    expect(core.textContent).toContain('Questions · 12 · 12 answered');
    const rows = () => core.querySelectorAll('[data-testid="research-question"]');
    expect(rows()).toHaveLength(12);
    // Every landing carries the kind the lane named as a chip: q0–q6 external, q7–q11 design.
    const kinds = [...core.querySelectorAll('[data-testid="research-kind"]')].map(
      (el) => (el as HTMLElement).dataset.kind
    );
    expect(kinds).toEqual([...Array(7).fill('external'), ...Array(5).fill('design')]);
    // The row is the landing line: chars, raised, and the engine's lane clock at the landing.
    const q5 = [...rows()].find((el) => el.textContent?.startsWith('q5external'))!;
    expect(q5.textContent).toBe('q5external[ledgerd-core] q5956 chars · 1 raised · 3724s');
    const close = core.querySelector('[data-testid="research-lane-close"]') as HTMLElement;
    expect(close.textContent).toBe('closedlanded 12 · done 1h 8m');
    // The header folds the rows and keeps the close line; a second click brings all twelve back.
    const toggle = core.querySelector('[data-testid="research-questions-toggle"]') as HTMLElement;
    fireEvent.click(toggle);
    expect(rows()).toHaveLength(0);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(core.querySelector('[data-testid="research-lane-close"]')).toBeTruthy();
    fireEvent.click(toggle);
    expect(rows()).toHaveLength(12);
  });
});

// ── VA-031: a covered question, a reclassified kind and a raised-folded line are ROWS on the lane. ──
//
// The live r6j run.jsonl (benchmark/runs/build/swarm-3node-r0, read 2026-09-02 15:36 → 20:48) carries
// 37 `research_question_kind` — every one `source: "model"` — 0 `research_question_covered` (its
// emitters died with VA-089, e33a2f77f; the r6c–r6h archives carry it and none is on this machine) and
// 0 `research_raised_folded` (17 `research_raised_for` instead). The three are therefore built from the
// emit fields VERBATIM: research.rs:988 (kind, `source`/`model_kind`), research.rs:1046 (folded,
// `raised_by`/`question`), and at e33a2f77f^ swarm.rs:21380 (`by: mini`, `by_mini`, `rule`) and
// research_plan.rs:184 (`by: decision`, `decision`, `rule`; rules cite | decision_id | stem). Timestamps
// and the landing's numbers are web-page's own (17:02 dispatch, q0 2,860 chars / raised 1 / 1172s,
// 17:24 closer with 9 builder_decides).
const WEB_PAGE_Q0_CITE =
  'request.md:602-640 (Records page); open because: the request freezes the row layout but not the sort key; alternatives: created_at desc';
const WEB_PAGE_Q0 = 'Which sort key orders the Records page by default?';
const WEB_PAGE_Q1 = 'What does window.page.showRecord(id) return when the record is filtered out?';
const WEB_PAGE_Q2 = 'Is the SSE event named, or the default message channel?';
const WEB_PAGE_RAISED = 'Does the sort survive a brush from viz.js, or reset to created_at desc?';
const LANDING_TS = '2026-09-02T17:21:55.379061+00:00';
const COVERED_TS = '2026-09-02T17:02:23.220900+00:00';
const va031Landing: Ev[] = [
  // The engine's order for one landing: kind → funnel → the raised lines.
  at(LANDING_TS, {
    event: 'research_question_kind',
    slice: 'web-page',
    q_index: 0,
    kind: 'spec_restated',
    source: 'classifier',
    model_kind: 'design',
    cite: WEB_PAGE_Q0_CITE,
    question: WEB_PAGE_Q0,
  }),
  at(LANDING_TS, {
    event: 'research_answered',
    slice: 'web-page',
    q_index: 0,
    chars: 2860,
    raised: 1,
    secs: 1172,
    batch: 0,
    model: HOST['web-page'],
  }),
  at(LANDING_TS, {
    event: 'research_raised_folded',
    slice: 'web-page',
    q_index: 0,
    raised_by: 'research-web-page-q0.json',
    question: WEB_PAGE_RAISED,
  }),
];
const va031CoveredByMini: Ev = at(COVERED_TS, {
  event: 'research_question_covered',
  slice: 'web-page',
  q_index: 1,
  question: WEB_PAGE_Q1,
  by: 'mini',
  by_mini: 'research-web-viz-q0.json',
  rule: 'cite',
});
const va031CoveredByDecision: Ev = at(COVERED_TS, {
  event: 'research_question_covered',
  slice: 'web-page',
  q_index: 2,
  question: WEB_PAGE_Q2,
  by: 'decision',
  decision: 0,
  rule: 'decision_id',
});
const VA031: Ev[] = [
  ...OPENING,
  ...dispatch(ORDER[3]),
  va031CoveredByMini,
  va031CoveredByDecision,
  ...va031Landing,
  ...closer(CLOSERS[1]),
];

describe('VA-031 — covered, reclassified and raised-folded rows on the lane', () => {
  it('a covered question stays a row with its provenance, and the lane’s count still includes it', () => {
    const lane = lanesOf(VA031, 'va031-fold')['research-web-page'];
    expect(lane.researchQuestions?.map((q) => `${q.qIndex}:${q.status}`)).toEqual([
      '0:answered',
      '1:covered',
      '2:covered',
    ]);
    expect(lane.researchQuestions?.[1]).toMatchObject({
      question: WEB_PAGE_Q1,
      covered: { by: 'mini', mini: 'research-web-viz-q0.json', rule: 'cite' },
    });
    expect(lane.researchQuestions?.[2]).toMatchObject({
      question: WEB_PAGE_Q2,
      covered: { by: 'decision', decision: 0, rule: 'decision_id' },
    });
    expect(lane.researchQuestions?.[2].covered?.mini).toBeUndefined();
    // The derived count is the lane's whole batch — the two covered questions included.
    expect(lane.description).toBe(
      'Research web-page · 3 questions derived in one read-only session (1 spec_restated)'
    );
    // Covered rows are terminal: the lane settles, and they are neither landed nor missed.
    expect(lane.status).toBe('done');
    expect(lane.researchClose).toMatchObject({ reason: 'remainder_empty', landed: 1 });
  });

  it('the classifier’s override keeps what the lane said; the raised line is on the lane that raised it', () => {
    const lane = lanesOf(VA031, 'va031-kind')['research-web-page'];
    expect(lane.researchQuestions?.[0]).toMatchObject({
      status: 'answered',
      kind: 'spec_restated',
      kindSource: 'classifier',
      modelKind: 'design',
      cite: WEB_PAGE_Q0_CITE,
      chars: 2860,
      raised: 1,
      secs: 1172,
    });
    expect(lane.researchRaisedFolded).toEqual([
      { qIndex: 0, raisedBy: 'research-web-page-q0.json', question: WEB_PAGE_RAISED },
    ]);
    // r6j's own shape — `source: model`, `model_kind: null` — names no override and no fold.
    const viz = lanesOf(R6J, 'va031-r6j')['research-web-viz'];
    expect(viz.researchQuestions?.[0]).toMatchObject({ kind: 'design', kindSource: 'model' });
    expect(viz.researchQuestions?.[0].modelKind).toBeUndefined();
    expect(viz.researchRaisedFolded).toBeUndefined();
  });

  it('a cover that lands before the lane’s dispatch (the archive order) still joins that lane', () => {
    const lanes = lanesOf(
      [...OPENING, va031CoveredByMini, ...dispatch(ORDER[3]), ...va031Landing],
      'va031-early'
    );
    expect(Object.keys(lanes)).toEqual(['research-web-page']);
    const lane = lanes['research-web-page'];
    expect(lane.researchQuestions?.map((q) => `${q.qIndex}:${q.status}`)).toEqual(['0:answered', '1:covered']);
    expect(lane.model).toBe(HOST['web-page']);
    expect(lane.researchRaisedFolded).toHaveLength(1);
  });
});

describe('VA-031 rendered — the web-page lane row', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-3node-r0',
      dir: '/tmp/build',
      events: VA031,
      activity: {
        'research-web-page': {
          tool_calls: 1,
          last_text: 'one answer landed',
          model: 'workhorse-qwen3.8-27b',
          attempt: 0,
          phase: 'done',
        },
      },
      activityMtimes: {},
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
  afterEach(() => cleanup());

  it('reads "Questions · 3 · 1 answered · 2 covered · 1 raised", a solid covered-by chip per covered row, "was design" on the override, and the raised line', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
    const phase = await screen.findByTestId('planning-phase-research');
    // The header counts what the body shows: three question rows, covered ones included.
    expect(phase.textContent).toContain('Research answers · 1 lane · 3 questions');
    const lane = [...phase.querySelectorAll('[data-testid="turn-lane"]')].find((el) =>
      (el.textContent ?? '').includes('Research web-page')
    ) as HTMLElement;
    expect(lane).toBeTruthy();
    fireEvent.click(lane.querySelector('button')!);
    await waitFor(() => expect(lane.querySelector('[data-testid="research-questions"]')).toBeTruthy());
    expect(lane.textContent).toContain('Questions · 3 · 1 answered · 2 covered · 1 raised');
    expect(lane.textContent).not.toContain('unanswered');
    const rows = [...lane.querySelectorAll('[data-testid="research-question"]')] as HTMLElement[];
    expect(rows).toHaveLength(3);
    expect(rows.map((r) => r.dataset.status)).toEqual(['answered', 'covered', 'covered']);
    // The covered rows: q chip, the provenance chip (the mini's slice and index / the decision's
    // index, as the engine named them), the question, the rule verbatim.
    const chips = [...lane.querySelectorAll('[data-testid="research-covered-by"]')] as HTMLElement[];
    expect(chips.map((c) => c.textContent)).toEqual(['covered by web-viz q0', 'covered by decision 0']);
    expect(chips.map((c) => c.dataset.by)).toEqual(['mini', 'decision']);
    expect(chips[0].title).toBe('research-web-viz-q0.json');
    // A solid token fill, never a hand-written colour (DESIGN.md ban 5).
    expect(chips[0].className).toContain('bg-lz-secondary');
    expect(chips[0].getAttribute('style')).toBeNull();
    expect(rows[1].textContent).toBe(`q1covered by web-viz q0${WEB_PAGE_Q1}covered · rule cite`);
    expect(rows[2].textContent).toBe(`q2covered by decision 0${WEB_PAGE_Q2}covered · rule decision_id`);
    // The override: the final kind chip, then what the lane had said, with the classifier's evidence.
    const kind = rows[0].querySelector('[data-testid="research-kind"]') as HTMLElement;
    expect(kind.dataset.kind).toBe('spec_restated');
    const was = rows[0].querySelector('[data-testid="research-kind-reclassified"]') as HTMLElement;
    expect(was.textContent).toBe('was design');
    expect(was.dataset.modelKind).toBe('design');
    expect(was.title).toBe(
      `the lane said design; reclassified spec_restated by the classifier — ${WEB_PAGE_Q0_CITE}`
    );
    expect(rows[1].querySelector('[data-testid="research-kind-reclassified"]')).toBeNull();
    // The raised line, under the list, on the lane that raised it.
    const folded = [...lane.querySelectorAll('[data-testid="research-raised-folded"]')] as HTMLElement[];
    expect(folded).toHaveLength(1);
    expect(folded[0].textContent).toBe(`raised${WEB_PAGE_RAISED}by q0 · folded into this slice’s brief`);
    expect(folded[0].title).toBe('research-web-page-q0.json');
    // Folding the header hides the rows and the raised line together; the close line stays.
    const toggle = lane.querySelector('[data-testid="research-questions-toggle"]') as HTMLElement;
    fireEvent.click(toggle);
    expect(lane.querySelectorAll('[data-testid="research-question"]')).toHaveLength(0);
    expect(lane.querySelectorAll('[data-testid="research-raised-folded"]')).toHaveLength(0);
    expect(lane.querySelector('[data-testid="research-lane-close"]')?.textContent).toBe(
      'closedlanded 1 · done 22m'
    );
  });
});

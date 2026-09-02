import { beforeEach, describe, expect, it } from 'vitest';
import {
  buildActivity,
  buildPhaseTodo,
  foldEvents,
  foldRunPhase,
  researchLaneLabel,
  resetFoldCache,
  tallyKinds,
} from './useSwarmRun';
import { planningLanesFor } from './phaseList';
import type { TurnLane } from './useSwarmRun';

/**
 * RESEARCH FAN v2 since VA-089 — ONE read-only lane per slice DERIVES its own questions in-session
 * and answers them, plus the decisions lane; the opener emits slices only. Every payload shape below
 * is verbatim from the emit sites read 2026-09-02: research.rs `emit_research_planned` /
 * `emit_research_outcome`, swarm.rs's `research_dispatched` / `research_slice_resumed`, opener.rs's
 * `research_question_ignored`, answer_routing.rs (VA-104), dep_sources (VA-103), vendor_probe.rs
 * (VA-105) and shards.rs `shard_provides_empty` (VA-102). The fan emits NO `phase` event (its banner
 * is stderr-only), so phase visibility is derived from these events — pinned here too.
 *
 * The truth-layer rule under test: every line is built from the event's own fields; a field the log
 * lacks is left unsaid — never "undefined", never an invented 0.
 */

const ts = '2026-09-02T10:00:00Z';
const START = {
  event: 'run_started',
  ts,
  pool: [
    { id: 'mac-mihai-x', model_id: 'mihai-qwen' },
    { id: 'mac-gabee-x', model_id: 'gabee-qwen' },
  ],
};

const IGNORED = { event: 'research_question_ignored', slice: 'ledgerd-api', count: 3 };
const PLANNED = {
  event: 'research_planned',
  lanes: 3,
  per_slice_sections: { 'ledgerd-api': 2, 'ledgerd-core': 3, 'web-console': 0 },
  resumed_slices: ['ledgerd-api'],
  decisions: 1,
};
const RESUMED = { event: 'research_slice_resumed', slice: 'ledgerd-api', rows: 2 };
const DISPATCH_CORE = {
  event: 'research_dispatched',
  slice: 'ledgerd-core',
  derives: true,
  q_indexes: [] as number[],
  model: 'mihai-qwen',
  activity_key: 'research-ledgerd-core',
};
const DISPATCH_WEB = {
  ...DISPATCH_CORE,
  slice: 'web-console',
  model: 'gabee-qwen',
  activity_key: 'research-web-console',
};
const DISPATCH_DECISIONS = {
  event: 'research_dispatched',
  slice: '__open_decisions__',
  derives: false,
  q_indexes: [0],
  model: 'mihai-qwen',
  activity_key: 'research-__open_decisions__',
};
const KIND_CORE_0 = {
  event: 'research_question_kind',
  slice: 'ledgerd-core',
  q_index: 0,
  kind: 'design',
  cite: null,
  question: 'Which storage format does the request mandate for the payments ledger?',
};
const ANSWERED_CORE_0 = {
  event: 'research_answered',
  slice: 'ledgerd-core',
  q_index: 0,
  chars: 1843,
  raised: 1,
  secs: 74,
  batch: 2,
  model: 'mihai-qwen',
};
const KIND_CORE_1 = {
  event: 'research_question_kind',
  slice: 'ledgerd-core',
  q_index: 1,
  kind: 'external',
  cite: 'request.md:218',
  question: 'What page shape does the vendor /v3/payments endpoint return?',
};
const ANSWERED_CORE_1 = { ...ANSWERED_CORE_0, q_index: 1, chars: 900, raised: 0, secs: 40 };
const KIND_WEB_0 = {
  event: 'research_question_kind',
  slice: 'web-console',
  q_index: 0,
  kind: 'unkinded',
  cite: null,
  question: 'Which degraded state does the console show while the vendor is down?',
};
const UNANSWERED_WEB_0 = {
  event: 'research_unanswered',
  slice: 'web-console',
  q_index: 0,
  reason: 'judge_ended',
  detail: 'judge_out_of_moves after 4 looks',
  secs: 210,
  model: 'gabee-qwen',
};
const KIND_DECISION_0 = {
  event: 'research_question_kind',
  slice: '__open_decisions__',
  q_index: 0,
  kind: 'design',
  cite: null,
  question: 'D1: retry registration forever or give up after the vendor answers 5xx?',
};
const ANSWERED_DECISION_0 = {
  event: 'research_answered',
  slice: '__open_decisions__',
  q_index: 0,
  chars: 400,
  raised: 0,
  secs: 30,
  batch: 1,
  model: 'mihai-qwen',
};
const ROUTED = {
  event: 'research_answer_routed',
  from_slice: 'webhooks-workflow',
  to_task: 'ledgerd-core',
  q_index: 5,
  matched: 'file',
  value: 'app/webhooks.py',
  owner: 'webhooks-workflow',
  arm: 'implemented_by_asker',
};
const UNOWNED = {
  event: 'research_answer_unowned',
  from_slice: 'ledgerd-core',
  q_index: 1,
  names: ['lib/vendor_client.py'],
};
const ROUTING_SKIPPED = {
  event: 'research_answer_routing_skipped',
  error: 'expected value at line 1 column 1',
};
const DEP_CUT = {
  event: 'dep_source_truncated',
  task_id: 'ledgerd-core',
  file: 'app/db.py',
  bytes: 9000,
  kept: 5128,
  reason: 'over the per-file budget',
};
const VENDOR_CUT = {
  event: 'vendor_probe_truncated',
  kind: 'body',
  url: 'http://127.0.0.1:8850/v3/payments',
  chars: 40000,
  kept: 12000,
  at_object_boundary: true,
};
const VENDOR_FAILED = {
  event: 'vendor_probe_fetch_failed',
  url: 'http://127.0.0.1:8850/v3/docs',
  error: 'connection refused',
};
const SHARD_EMPTY = {
  event: 'shard_provides_empty',
  module: 'web-viz',
  shard: 'web-viz-shard-2',
  sections: ['Rendering'],
  reason: 'synthesis declared no exports for this shard',
};
const SYNTHESIS = { event: 'phase', phase: 'synthesis' };

const FAN = [
  START,
  IGNORED,
  PLANNED,
  RESUMED,
  DISPATCH_CORE,
  DISPATCH_WEB,
  DISPATCH_DECISIONS,
  KIND_CORE_0,
  ANSWERED_CORE_0,
  KIND_CORE_1,
  ANSWERED_CORE_1,
  KIND_WEB_0,
  UNANSWERED_WEB_0,
  KIND_DECISION_0,
  ANSWERED_DECISION_0,
];
const WHOLE = [
  ...FAN,
  ROUTED,
  UNOWNED,
  ROUTING_SKIPPED,
  DEP_CUT,
  VENDOR_CUT,
  VENDOR_FAILED,
  SHARD_EMPTY,
  SYNTHESIS,
];

describe("the fan reaches the feed — one line per lane, from the events' own fields", () => {
  it('research_planned is the Research line: lanes, one per slice, the decisions lane, resumed count', () => {
    const { activity, verbose } = buildActivity([START, PLANNED]);
    for (const feed of [activity, verbose]) {
      const row = feed.find((r) => r.text.startsWith('Research — '))!;
      expect(row.text).toBe(
        'Research — 3 lanes (one per slice: ledgerd-core, web-console; the open decisions), 1 resumed from the ledger'
      );
      expect(row.sub).toBe(
        'ledgerd-api 2 sections (resumed) · ledgerd-core 3 sections · web-console 0 sections · 1 open decision'
      );
    }
  });

  it('a plan with no lane to run says so — never "0 questions"', () => {
    const { activity } = buildActivity([
      START,
      {
        event: 'research_planned',
        lanes: 0,
        per_slice_sections: { boot: 1 },
        resumed_slices: ['boot'],
        decisions: 0,
      },
    ]);
    expect(activity.map((r) => r.text)).toContain(
      'Research — no lane to run, 1 resumed from the ledger'
    );
    expect(activity.some((r) => r.text.includes('0 question'))).toBe(false);
  });

  it('a slice lane is ONE line: "deriving" at dispatch, then the derived count and kinds as they land', () => {
    const early = buildActivity([START, PLANNED, DISPATCH_CORE]);
    const first = early.activity.find((r) => r.text.startsWith('Researching ledgerd-core'))!;
    expect(first.text).toBe('Researching ledgerd-core · deriving its questions from the spec');
    expect(first.sub).toBe('on mihai');

    const { activity, verbose } = buildActivity(FAN);
    const lines = activity.filter((r) => r.text.startsWith('Researching ledgerd-core'));
    expect(lines).toHaveLength(1);
    expect(lines[0].text).toBe(
      'Researching ledgerd-core · 2 questions derived (q0, q1) — 1 design, 1 external'
    );
    const v = verbose.find((r) => r.text.startsWith('Researching ledgerd-core'))!;
    expect(v.sub?.split('\n')).toEqual([
      'on mihai',
      '[q0] (design) Which storage format does the request mandate for the payments ledger?',
      '[q1] (external) What page shape does the vendor /v3/payments endpoint return?',
    ]);
    // The decisions lane names its indexes at dispatch and its question when it lands.
    expect(activity.some((r) => r.text === 'Researching open decisions q0')).toBe(true);
    expect(verbose.find((r) => r.text === 'Researching open decisions q0')?.sub).toBe(
      'on mihai — D1: retry registration forever or give up after the vendor answers 5xx?'
    );
    // Three lanes, three lines — the resumed slice runs none.
    expect(activity.filter((r) => r.text.startsWith('Researching ')).length).toBe(3);
  });

  it('an answer is a verbose done-line carrying the kind the lane named, chars, follow-ups and the node', () => {
    const { verbose } = buildActivity(FAN);
    const row = verbose.find((r) => r.text.startsWith('Research answered — ledgerd-core q0'))!;
    expect(row.text).toBe('Research answered — ledgerd-core q0 (1,843 chars, 1 follow-up raised)');
    expect(row.tone).toBe('good');
    expect(row.sub).toBe('design · by mihai · 74s');
  });

  it('an unanswered question is LOUD in both feeds, reason verbatim, detail as sub', () => {
    const { activity, verbose } = buildActivity(FAN);
    for (const feed of [activity, verbose]) {
      const row = feed.find((r) => r.text === 'Research unanswered — web-console q0 (judge ended)');
      expect(row?.tone).toBe('warn');
      expect(row?.sub).toContain('judge_out_of_moves');
    }
  });

  it('a legacy opener emit with per-slice questions is a warning naming the slice and count', () => {
    const { activity } = buildActivity([START, IGNORED]);
    const row = activity.find((r) => r.text.startsWith('Opener wrote'))!;
    expect(row.text).toBe(
      'Opener wrote 3 questions for ledgerd-api — ignored; its research lane derives its own'
    );
    expect(row.tone).toBe('warn');
  });

  it('a resumed slice says its rows came from the ledger and no lane ran', () => {
    const { activity } = buildActivity([START, PLANNED, RESUMED]);
    expect(activity.map((r) => r.text)).toContain(
      'Research ledgerd-api resumed from the ledger — 2 rows reused, no lane'
    );
  });

  it('a fan-wide panic is a bad line carrying the error', () => {
    const { activity } = buildActivity([
      START,
      { event: 'research_fan_panicked', error: 'task panicked at join' },
    ]);
    const row = activity.find(
      (r) => r.text === 'The research fan crashed — planning proceeds with zero answers'
    );
    expect(row?.tone).toBe('bad');
    expect(row?.sub).toBe('task panicked at join');
  });

  it('the VA-102/103/104/105 events each render a line from their own fields', () => {
    const { activity, verbose } = buildActivity(WHOLE);
    const texts = (feed: typeof activity) =>
      feed.map((r) => `${r.text}${r.sub ? ` | ${r.sub}` : ''}`);
    expect(texts(verbose)).toContain(
      'Research answer q5 of webhooks-workflow routed to ledgerd-core (owner webhooks-workflow, file app/webhooks.py) | implemented by asker'
    );
    expect(texts(activity)).toContain(
      'Research answer q1 of ledgerd-core names 1 path no plan task owns | lib/vendor_client.py'
    );
    expect(texts(activity)).toContain(
      'Research answers were not routed into the briefs — the plan did not parse | expected value at line 1 column 1'
    );
    expect(texts(activity)).toContain(
      "Dependency source app/db.py cut in ledgerd-core's brief — 5,128 of 9,000 bytes kept | over the per-file budget"
    );
    expect(texts(activity)).toContain(
      'Vendor body http://127.0.0.1:8850/v3/payments cut — 12,000 of 40,000 chars kept | cut after a whole JSON object'
    );
    expect(texts(activity)).toContain(
      'Vendor fetch failed — http://127.0.0.1:8850/v3/docs: connection refused'
    );
    expect(texts(activity)).toContain(
      'Shard web-viz-shard-2 of web-viz declares no exports — its brief states the absence where PROVIDES would be | sections: Rendering'
    );
  });

  it('no line on either feed says "undefined" or invents a count for a field the log lacks', () => {
    const { activity, verbose } = buildActivity(WHOLE);
    for (const r of [...activity, ...verbose]) {
      expect(`${r.text} ${r.sub ?? ''}`).not.toContain('undefined');
      expect(`${r.text} ${r.sub ?? ''}`).not.toContain('NaN');
      expect(r.text).not.toMatch(/\bq\?/);
    }
    expect(activity.some((r) => r.text.includes('answered from the spec'))).toBe(false);
  });
});

describe('phase visibility — derived from the fan events, since no phase event fires', () => {
  it('research_planned with lanes enters Research; a plan with no lane does not', () => {
    const askFirst = [START, { event: 'phase', phase: 'ask' }];
    expect(foldRunPhase([...askFirst, PLANNED]).phase).toBe('research');
    expect(foldRunPhase([...askFirst, PLANNED]).observed.research).toBe(true);
    const none = foldRunPhase([
      ...askFirst,
      {
        event: 'research_planned',
        lanes: 0,
        per_slice_sections: { boot: 1 },
        resumed_slices: ['boot'],
        decisions: 0,
      },
    ]);
    expect(none.phase).toBe('ask');
    expect(none.observed.research).toBeUndefined();
  });

  it('dispatch, question_kind and the outcomes count as research evidence; phase:synthesis moves on', () => {
    for (const ev of [DISPATCH_CORE, KIND_CORE_0, ANSWERED_CORE_0, UNANSWERED_WEB_0])
      expect(foldRunPhase([START, ev]).phase).toBe('research');
    expect(foldRunPhase([...FAN, SYNTHESIS]).phase).toBe('synthesize');
  });
});

describe('the research lanes: one per dispatched key, questions named as the lane derives them', () => {
  beforeEach(() => resetFoldCache());

  it('a dispatched slice lane exists before any question or digest — running, "deriving"', () => {
    const folded = foldEvents([START, PLANNED, DISPATCH_CORE] as never, {} as never, 'va101-early');
    const lane = folded.researchLanes.find((l) => l.taskId === 'research-ledgerd-core');
    expect(lane, 'the dispatch is the fact').toBeTruthy();
    expect(lane?.status).toBe('running');
    expect(lane?.description).toBe(
      'Research ledgerd-core · deriving its questions in one read-only session'
    );
    expect(lane?.device).toBe('mihai');
    expect(lane?.researchQuestions ?? []).toHaveLength(0);
  });

  it('the rows carry kind + cite from research_question_kind and settle on the outcome; kinds in the caption', () => {
    const folded = foldEvents(FAN as never, {} as never, 'va101-rows');
    expect(folded.researchLanes.map((l) => l.taskId)).toEqual([
      'research-__open_decisions__',
      'research-ledgerd-core',
      'research-web-console',
    ]);
    const core = folded.researchLanes.find((l) => l.taskId === 'research-ledgerd-core')!;
    expect(core.description).toBe(
      'Research ledgerd-core · 2 questions derived in one read-only session (1 design, 1 external)'
    );
    expect(core.researchQuestions).toEqual([
      expect.objectContaining({
        qIndex: 0,
        kind: 'design',
        status: 'answered',
        chars: 1843,
        raised: 1,
        secs: 74,
      }),
      expect.objectContaining({
        qIndex: 1,
        kind: 'external',
        cite: 'request.md:218',
        status: 'answered',
        chars: 900,
      }),
    ]);
    expect(core.researchQuestions?.[0].cite).toBeUndefined();
    expect(core.status).toBe('done');
    // The absence twin: a lane whose every derived question came back unanswered is not a clean pass.
    const web = folded.researchLanes.find((l) => l.taskId === 'research-web-console')!;
    expect(web.status).toBe('error');
    expect(web.researchQuestions?.[0]).toMatchObject({
      kind: 'unkinded',
      status: 'unanswered',
      reason: 'judge_ended',
    });
    // The decisions lane: seeded from q_indexes, named by its kind event.
    const decisions = folded.researchLanes.find((l) => l.taskId === 'research-__open_decisions__')!;
    expect(decisions.description).toBe(
      'Research open decisions · 1 question in one read-only session (1 design)'
    );
    expect(decisions.researchQuestions?.[0].question).toContain('D1:');
    expect(decisions.status).toBe('done');
    expect(folded.planningLanes.some((l) => l.taskId.startsWith('research-'))).toBe(false);
  });

  it('a lane-level outcome with no question (lane_panicked) still lands on the deriving lane — the absence renders', () => {
    const folded = foldEvents(
      [
        START,
        DISPATCH_WEB,
        { ...UNANSWERED_WEB_0, reason: 'lane_panicked', detail: 'task panicked at join' },
      ] as never,
      {} as never,
      'va101-panic'
    );
    const web = folded.researchLanes.find((l) => l.taskId === 'research-web-console')!;
    expect(web.researchQuestions).toEqual([
      expect.objectContaining({
        qIndex: 0,
        question: '',
        status: 'unanswered',
        reason: 'lane_panicked',
      }),
    ]);
    expect(web.status).toBe('error');
  });

  it('researchLaneLabel and tallyKinds: nothing claimed the events did not carry', () => {
    expect(researchLaneLabel('research-web-console')).toBe('Research · web-console');
    expect(researchLaneLabel('research-web-console', 0, { derived: true, kinds: '' })).toBe(
      'Research web-console · deriving its questions in one read-only session'
    );
    expect(
      researchLaneLabel('research-ledgerd-core', 3, {
        derived: true,
        kinds: '2 design, 1 unkinded',
      })
    ).toBe(
      'Research ledgerd-core · 3 questions derived in one read-only session (2 design, 1 unkinded)'
    );
    expect(tallyKinds([])).toBe('');
    expect(tallyKinds([undefined, 'external', 'design', 'design', 'odd'])).toBe(
      '2 design, 1 external, 1 odd'
    );
  });

  it('planningLanesFor puts the research fan under the Research chip, v1 slices as the fallback', () => {
    const laneOf = (taskId: string): TurnLane => ({
      taskId,
      device: 'mihai',
      status: 'done',
      seq: 0,
    });
    const v2 = planningLanesFor('research', {
      sliceLanes: [],
      contractLanes: [],
      researchLanes: [laneOf('research-boot')],
    });
    expect(v2?.label).toBe('Research answers');
    const v1 = planningLanesFor('research', {
      sliceLanes: [laneOf('slice-boot')],
      contractLanes: [],
      researchLanes: [],
    });
    expect(v1?.label).toBe('Slice specs');
  });
});

describe('the Research checklist rows count what the fan measured', () => {
  const research = (events: Array<Record<string, unknown>>) =>
    buildPhaseTodo(events, {}, { clarifyPending: false }).find((p) => p.key === 'research')!;

  it('plan built, nothing dispatched yet: the plan line, running, with the per-slice split', () => {
    const p = research([START, PLANNED]);
    expect(p.items.map((i) => i.label)).toEqual([
      'Research — 3 lanes (one per slice: ledgerd-core, web-console; the open decisions), 1 resumed from the ledger',
    ]);
    expect(p.items[0].detail).toBe(
      'ledgerd-api 2 sections (resumed) · ledgerd-core 3 sections · web-console 0 sections · 1 open decision'
    );
    expect(p.state).toBe('running');
    expect(p.note).toBeUndefined();
  });

  it('a plan with no lane to run is a done row, never the legacy "scoped — 0"', () => {
    const p = research([
      START,
      {
        event: 'research_planned',
        lanes: 0,
        per_slice_sections: { boot: 1 },
        resumed_slices: ['boot'],
        decisions: 0,
      },
    ]);
    expect(p.items.map((i) => i.label)).toEqual([
      'Research — no lane to run, 1 resumed from the ledger',
    ]);
    expect(p.state).toBe('done');
  });

  it('mid-fan: lanes dispatched of planned, derived questions settled, kinds as detail', () => {
    const p = research(FAN.slice(0, FAN.indexOf(KIND_DECISION_0)));
    const run = p.items.find((i) => i.id === 'r2-run')!;
    expect(run.label).toBe(
      'Researching — 3 of 3 lanes dispatched · 3 of 3 derived questions settled, 1 resumed from the ledger'
    );
    expect(run.detail).toBe('1 design, 1 external, 1 unkinded');
    expect(p.state).toBe('running');
  });

  it('over (phase:synthesis): answered of derived across lanes; misses and the ignored emit on their own rows', () => {
    const p = research(WHOLE);
    const labels = p.items.map((i) => i.label);
    expect(labels).toEqual([
      'Research — 3 of 4 derived questions answered across 3 lanes, 1 resumed from the ledger',
      '1 question unanswered — kept as raw questions in the briefs',
      'Opener wrote 3 questions for ledgerd-api — ignored; its research lane derives its own',
    ]);
    expect(p.items[0].state).toBe('done');
    expect(p.items[0].detail).toBe(
      '2 design, 1 external, 1 unkinded · answered questions become settled facts in their slice brief, beside the sources'
    );
    expect(p.items[1].detail).toBe('judge ended ×1');
    expect(p.items[2].state).toBe('advisory');
    expect(p.note).toBeUndefined();
    for (const i of p.items) expect(`${i.label} ${i.detail ?? ''}`).not.toContain('undefined');
  });

  it('a fan panic is an unverified row naming the lanes dispatched before the crash', () => {
    const p = research([
      START,
      PLANNED,
      DISPATCH_CORE,
      { event: 'research_fan_panicked', error: 'join panicked' },
    ]);
    const row = p.items.find((i) => i.label.startsWith('Research fan crashed'))!;
    expect(row.state).toBe('unverified');
    expect(row.detail).toBe('1 lane dispatched before the crash');
  });

  it('an archived pre-v2 run without fan events builds no v2 rows (legacy branches untouched)', () => {
    const p = research([
      START,
      { event: 'research_completed', slices: 2, brief_chars: [4200, 3100], secs: 260 },
    ]);
    expect(p.items.some((i) => i.id.startsWith('r2-'))).toBe(false);
    expect(p.items.some((i) => i.id === 'r-specs')).toBe(true);
  });
});

import { beforeEach, describe, expect, it } from 'vitest';
import { deriveFleet, digestsFromThisRun, foldEvents, resetFoldCache } from './useSwarmRun';
import type { TurnLane } from './useSwarmRun';

/**
 * ONE DIGEST-TO-LANE JOIN, PROVEN ACROSS EVERY PATH THAT BUILDS A LANE.
 *
 * The join was hand-copied five times and diverged twice. `fullThinking` reached one path while
 * `thinkingChars` reached four, so the inspector's header counted a transcript its body was not showing.
 * Extracting only the STREAM fields left the rest still copied, and the sixth omission was already in that
 * remainder: the repair-twin path set `errors` and not `phase` — and `phase` is what makes a node
 * prompt-processing a fix read WORKING instead of idle, and what drops a finished twin out of running.
 *
 * This test is what stops "which paths carry which field" being counted by hand again.
 */
const DIGEST = {
  last_text: 'answer text',
  recent: ['ran a command'],
  reasoning: 'short digest reasoning',
  full_reasoning: 'the 24k clip',
  calls: [],
  tool_calls: 7,
  thinking_chars: 4096,
  last_thinking: 'the rolling window',
  full_thinking: 'the durable think.log',
  thinking_bytes: 900000,
  full_transcript: 'the durable task.log',
  transcript_bytes: 200000,
  transcript_clipped: true,
  judging: true,
  phase: 'processing',
  inflight: [
    {
      id: 'call_9',
      tool: 'write',
      args: 'write app/cli.py (83 lines, 2100 bytes)',
      since: '2026-08-29T22:40:01+00:00',
    },
  ],
  errors: 2,
};

const EXPECTED = {
  lastText: 'answer text',
  recent: ['ran a command'],
  reasoning: 'short digest reasoning',
  fullReasoning: 'the 24k clip',
  calls: [],
  toolCalls: 7,
  thinkingChars: 4096,
  lastThinking: 'the rolling window',
  fullThinking: 'the durable think.log',
  thinkingBytes: 900000,
  fullTranscript: 'the durable task.log',
  transcriptBytes: 200000,
  transcriptClipped: true,
  judging: true,
  phase: 'processing',
  inflight: [
    {
      id: 'call_9',
      tool: 'write',
      args: 'write app/cli.py (83 lines, 2100 bytes)',
      since: '2026-08-29T22:40:01+00:00',
    },
  ],
  // Derived by the join, not copied: first sight of a lane with an answer reads the answer channel.
  liveChannel: 'transcript',
  errors: 2,
};

const joined = (lane: TurnLane | undefined) => {
  expect(lane, 'the lane must exist').toBeTruthy();
  return Object.fromEntries(Object.keys(EXPECTED).map((k) => [k, (lane as never)[k]]));
};

const RUN_STARTED = { event: 'run_started', ts: '2026-08-29T10:00:00Z', pool: [] };

describe('every lane-building path carries the whole digest', () => {
  beforeEach(() => resetFoldCache());

  it('joins it onto a BUILD worker lane', () => {
    const folded = foldEvents(
      [
        RUN_STARTED,
        { event: 'task_dispatched', task_id: 'ledgerd-core', model: 'mihai-qwen' },
      ] as never,
      { 'ledgerd-core': DIGEST } as never
    );
    expect(joined(folded.lanes.find((l) => l.taskId === 'ledgerd-core'))).toStrictEqual(EXPECTED);
  });

  // THE SIXTH OMISSION. This path set every other field and not `phase`, so a node chewing the fix prompt
  // read idle in the fleet strip and a twin whose digest stamped phase 'done' never left running.
  it('joins it onto a REPAIR twin lane, `phase` included', () => {
    const folded = foldEvents(
      [
        RUN_STARTED,
        {
          event: 'complete_fix_dispatched',
          task_id: 'complete-fix::twin0',
          round: 1,
          model: 'mihai-qwen',
        },
      ] as never,
      { 'complete-fix::twin0': DIGEST } as never
    );
    expect(joined(folded.fixLanes.find((l) => l.taskId === 'complete-fix::twin0'))).toStrictEqual(
      EXPECTED
    );
  });

  it('joins it onto a PLAN DRAFT lane', () => {
    const folded = foldEvents([RUN_STARTED] as never, { 'plandraft-1': DIGEST } as never);
    expect(joined(folded.planLanes[0])).toStrictEqual(EXPECTED);
  });

  it('joins it onto a fanned SLICE lane', () => {
    const folded = foldEvents([RUN_STARTED] as never, { 'slice-core': DIGEST } as never);
    expect(joined(folded.sliceLanes[0])).toStrictEqual(EXPECTED);
  });

  it('joins it onto the fleet strip’s laneless row', () => {
    const { workingByDevice } = deriveFleet({
      pool: ['mihai'],
      laneSources: [],
      digests: { 'verify::api': { ...DIGEST, model: 'mihai-qwen' } },
      digestMtimes: { 'verify::api': 1000 },
      now: 1000,
    });
    expect(joined(workingByDevice.get('mihai'))).toStrictEqual(EXPECTED);
  });
});

/**
 * `.swarm/activity/` survives a run: the engine truncates only `.swarm/prereview`, and main globs the whole
 * directory. A second run in the same working directory therefore inherits the previous run's digests, which
 * name no run and whose `phase` never reached 'done' on a killed one — so they mint lanes, claim nodes and
 * stamp checklist rows for work that is over. The mtime is the only signal that separates them without an
 * engine change.
 */
describe('digestsFromThisRun — a previous run’s leftovers do not become lanes', () => {
  const started = 5_000;

  it('drops a digest written before the run started', () => {
    expect(
      digestsFromThisRun({ old: 1, live: 2 }, { old: started - 1, live: started + 1 }, started)
    ).toStrictEqual({ live: 2 });
  });

  it('keeps a digest written at the run’s first event', () => {
    expect(digestsFromThisRun({ live: 2 }, { live: started }, started)).toStrictEqual({ live: 2 });
  });

  it('keeps a digest whose mtime is unknown — an older main supplies none', () => {
    expect(digestsFromThisRun({ live: 2 }, {}, started)).toStrictEqual({ live: 2 });
  });

  it('gates nothing when the stream carries no parseable start', () => {
    expect(digestsFromThisRun({ old: 1 }, { old: 1 }, null)).toStrictEqual({ old: 1 });
  });

  it('leaves no lane behind once the stale digests are gated out', () => {
    const stale = { 'slice-core': DIGEST };
    expect(foldEvents([RUN_STARTED] as never, stale as never).sliceLanes).toHaveLength(1);
    const gated = digestsFromThisRun(stale, { 'slice-core': 1 }, Date.parse(RUN_STARTED.ts));
    expect(foldEvents([RUN_STARTED] as never, gated as never).sliceLanes).toHaveLength(0);
  });
});

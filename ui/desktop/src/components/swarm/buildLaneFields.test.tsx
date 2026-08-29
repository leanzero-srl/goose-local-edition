import { describe, expect, it } from 'vitest';
import type { TurnLane } from './useSwarmRun';
import { deriveFleet, foldEvents } from './useSwarmRun';

// WHAT A DIGEST OWES A LANE, ON EVERY PATH THAT BUILDS ONE.
//
// The join from activity digest to lane was hand-copied into five places and diverged twice. The BUILD
// worker lane -- first in `laneSources`, so it is what BOTH the fleet strip and the inspector receive for
// a build node -- carried none of the durable fields for the whole of BUILD, which is where a run spends
// its hours. The repair-twin lane then set `errors` and not `phase`, so a node prompt-processing a fix
// read idle (`phase === 'processing'` is the only thing that says WORKING before the first token).
//
// Both were invisible because the test that was meant to catch them looked at ONE path (`folded.lanes`)
// `transcriptBytes`, three of the fields its own fixture set and its own comment named, could be dropped
// with the test still green.
//
// This file is now the gate for the whole contract: every field, on every path, with the path table
// checked against the fold's own output so a NEW lane group cannot be added uncovered.

const DIGEST = {
  last_text: 'the rolling view of the answer',
  recent: ['ran a command'],
  reasoning: 'short digest reasoning',
  full_reasoning: 'the 24k clip',
  calls: [{ name: 'shell', summary: 'pytest -q', ok: true }],
  tool_calls: 2,
  thinking_chars: 4096,
  last_thinking: 'the rolling window',
  full_thinking: 'the durable think.log, much longer than the window',
  thinking_bytes: 6014,
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
  errors: 1,
  model: 'mihai-qwen',
  // SAID provenance — stamped together by the engine's shared builder; a path dropping any of the
  // five re-creates the r0 case (a dead attempt's error shown as current, unlabeled).
  attempt: 1,
  dispatched_at: '2026-08-29T23:31:02+00:00',
  said_at: '2026-08-29T23:55:10+00:00',
  said_kind: 'said',
  superseded: [
    {
      attempt: 0,
      last_text:
        'Network error: Stream decode error: error decoding response body\n\nPlease resend your message to try again.',
      said_kind: 'error',
      said_at: '2026-08-29T23:30:40+00:00',
      model: 'mihai-qwen',
    },
  ],
};

// Written out by hand, deliberately. Deriving this from the join the paths share would make the test
// agree with the implementation about a field they had both lost. What keeps the two in step is the
// key-set assertion at the bottom, which fails when a lane gains or loses a digest field named here.
const EXPECTED: Record<string, unknown> = {
  lastText: 'the rolling view of the answer',
  recent: ['ran a command'],
  reasoning: 'short digest reasoning',
  fullReasoning: 'the 24k clip',
  calls: [{ name: 'shell', summary: 'pytest -q', ok: true }],
  toolCalls: 2,
  thinkingChars: 4096,
  lastThinking: 'the rolling window',
  fullThinking: 'the durable think.log, much longer than the window',
  thinkingBytes: 6014,
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
  errors: 1,
  attempt: 1,
  dispatchedAt: '2026-08-29T23:31:02+00:00',
  saidAt: '2026-08-29T23:55:10+00:00',
  saidKind: 'said',
  superseded: [
    {
      attempt: 0,
      last_text:
        'Network error: Stream decode error: error decoding response body\n\nPlease resend your message to try again.',
      said_kind: 'error',
      said_at: '2026-08-29T23:30:40+00:00',
      model: 'mihai-qwen',
    },
  ],
};

const digestFieldsOf = (lane: TurnLane | undefined): Record<string, unknown> => {
  const seen = lane as unknown as Record<string, unknown>;
  return Object.fromEntries(Object.keys(EXPECTED).map((k) => [k, seen[k]]));
};

const ts = '2026-08-29T10:00:00Z';

// One digest per lane-building path, all identical, so any field a path drops shows up as that path's
// field alone being undefined rather than as a fixture difference.
const ACTIVITY: Record<string, unknown> = {
  'ledgerd-core': DIGEST,
  'complete-fix::twin0': DIGEST,
  'plandraft-1': DIGEST,
  'scout-storage': DIGEST,
  'contract-ledger': DIGEST,
  'detail-ledger': DIGEST,
  'slice-ledger': DIGEST,
  synthesis: DIGEST,
};

const EVENTS: Array<Record<string, unknown>> = [
  { event: 'run_started', ts, pool: ['mihai-qwen'] },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'ledgerd-core',
    device: 'mihai-qwen',
    model: 'mihai-qwen',
  },
  // The repair wave dispatches outside the task lifecycle, which is why its lane is built by its own path.
  {
    event: 'complete_fix_dispatched',
    ts,
    task_id: 'complete-fix::twin0',
    round: 1,
    twin: 0,
    model: 'mihai-qwen',
  },
];

// group name in the fold's output -> the task id whose digest that group is built from.
const LANE_GROUPS: Record<string, string> = {
  lanes: 'ledgerd-core',
  fixLanes: 'complete-fix::twin0',
  planLanes: 'plandraft-1',
  scoutLanes: 'scout-storage',
  contractLanes: 'contract-ledger',
  detailLanes: 'detail-ledger',
  sliceLanes: 'slice-ledger',
  planningLanes: 'synthesis',
};

describe('every lane the fold builds carries every field its digest supplies', () => {
  const folded = foldEvents(EVENTS, ACTIVITY);
  const groupsOf = (run: typeof folded): Record<string, TurnLane[]> =>
    Object.fromEntries(
      Object.entries(run).filter(([, v]) => Array.isArray(v))
    ) as unknown as Record<string, TurnLane[]>;

  it.each(Object.entries(LANE_GROUPS))('%s (%s)', (group, taskId) => {
    const lanes = groupsOf(folded)[group];
    const lane = lanes?.find((l) => l.taskId === taskId);
    expect(lane, `${group} must contain a lane for ${taskId}`).toBeTruthy();
    expect(digestFieldsOf(lane)).toEqual(EXPECTED);
  });

  // THE GATE THAT MAKES THE TABLE ABOVE A CONTRACT RATHER THAN A SAMPLE. The last two omissions were both
  // "a path nobody counted": a lane group added here without an entry in LANE_GROUPS fails immediately
  // instead of shipping unjoined for a release.
  it('leaves no lane group of the fold untested', () => {
    expect(Object.keys(groupsOf(folded)).sort()).toEqual(Object.keys(LANE_GROUPS).sort());
  });

  // The three the previous version of this test set in its fixture, named in its own comment, and never
  // asserted -- so all three could be dropped with it still green. Kept as their own case because the
  // failure message "judging was lost" is worth more than one line of a 17-field diff.
  it('keeps the supervision fields a JSON.stringify assertion could not see', () => {
    const lane = folded.lanes.find((l) => l.taskId === 'ledgerd-core');
    expect(lane?.judging).toBe(true);
    expect(lane?.transcriptBytes).toBe(200000);
  });

  // The repair-twin path's own omission: it spread the stream fields and then set `errors` by hand while
  // `phase` was not set at all. The engine seeds `complete-fix::twinN` digests with phase="processing" at
  // dispatch and mirrors them out of the speculative shadow specifically so this lane can read them.
  it('gives a repair twin the phase that says it is prompt-processing', () => {
    const twin = folded.fixLanes.find((l) => l.taskId === 'complete-fix::twin0');
    expect(twin?.phase).toBe('processing');
    expect(twin?.status).toBe('running');
  });
});

// THE SIXTH PATH, and the one no `foldEvents` test can reach: the fleet strip builds a lane for a digest
// that has NO lane at all (a planning call the event stream never dispatched). It is the row the inspector
// opens for that node, so it owes the same fields.
describe('the fleet strip lane built from a digest alone', () => {
  const now = Date.now();
  const fleet = deriveFleet({
    pool: ['mihai'],
    laneSources: [],
    digests: { 'open-coverage-1': DIGEST },
    digestMtimes: { 'open-coverage-1': now },
    now,
  });

  it('carries every field the digest supplies', () => {
    const lane = fleet.workingByDevice.get('mihai');
    expect(lane, 'the digest must put its node in WORKING').toBeTruthy();
    expect(digestFieldsOf(lane)).toEqual(EXPECTED);
  });
});

describe('the join and this test cannot drift apart', () => {
  // The fields a lane has that are NOT the digest's: its identity, and the status the event stream or the
  // phase stamp decides. Everything else on a lane built purely FROM a digest came from the digest.
  const IDENTITY = ['taskId', 'description', 'device', 'model', 'status', 'seq'];

  // Asserted against the LANE, not against whichever helper the paths currently share: that join has
  // already been extracted once, and a test bound to its name stops proving anything the day it moves.
  // What must never change is what comes out the other end. A digest field that reaches a lane without
  // being named in EXPECTED fails here; one that stops reaching it fails the per-path cases above.
  it('leaves no digest-derived lane field unasserted', () => {
    const draft = foldEvents(EVENTS, ACTIVITY).planLanes.find((l) => l.taskId === 'plandraft-1');
    const carried = Object.keys(draft ?? {}).filter((k) => !IDENTITY.includes(k));
    expect(carried.sort()).toEqual(Object.keys(EXPECTED).sort());
  });
});

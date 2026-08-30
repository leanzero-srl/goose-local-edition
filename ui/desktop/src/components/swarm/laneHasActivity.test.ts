import { beforeEach, describe, expect, it } from 'vitest';
import { foldEvents, resetFoldCache } from './useSwarmRun';

/**
 * THE ONE KEEP/HIDE RULE for digest-built lanes (finishFold's hasActivity) — panel #5 item 3.
 *
 * A digest whose ONLY content is the durable `full_transcript` (answer channel written straight to
 * `<task>.log`, every rolling field still empty) satisfied none of the old clauses — text windows,
 * tool calls, thinking chars, prompt-processing — so its lane was silently HIDDEN while the node was
 * demonstrably producing output. Latent rather than measured, but the same class as every other
 * lane-hider this strip has shipped. The rule also used to exist twice (planLanes carried a near-copy
 * that had already dropped the tool-calls clause); it is one function now, and both paths are pinned.
 */

const START = { event: 'run_started', ts: '2026-08-30T10:00:00Z', pool: [{ id: 'local-mihai-x', model_id: 'mihai-qwen' }] };

describe('a fullTranscript-only digest still earns its lane', () => {
  beforeEach(() => resetFoldCache());

  it('on the laneFromDigest paths (slice-*)', () => {
    const folded = foldEvents(
      [START],
      { 'slice-store': { model: 'mihai-qwen', full_transcript: 'wrote the module spec to disk' } },
      'has-activity-slice'
    );
    expect(folded.sliceLanes.map((l) => l.taskId)).toEqual(['slice-store']);
  });

  it('on the planLanes path, which had its own near-copy of the rule', () => {
    const folded = foldEvents(
      [START],
      { 'plandraft-1': { model: 'mihai-qwen', full_transcript: 'the draft DAG' } },
      'has-activity-draft'
    );
    expect(folded.planLanes.map((l) => l.taskId)).toEqual(['plandraft-1']);
  });

  it('while a digest with NOTHING in it stays hidden — the filter still filters', () => {
    const folded = foldEvents(
      [START],
      { 'slice-empty': { model: 'mihai-qwen' } },
      'has-activity-empty'
    );
    expect(folded.sliceLanes).toHaveLength(0);
  });
});

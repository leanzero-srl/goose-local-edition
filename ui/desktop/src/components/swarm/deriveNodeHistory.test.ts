import { describe, expect, it } from 'vitest';
import type { TurnLane } from './useSwarmRun';
import { deriveNodeHistory } from './useSwarmRun';

/**
 * The cumulative per-node history behind the inspector's folded log — the durable answer to "as soon
 * as a phase ends the whole thing clears". Same derivation style as deriveFleet (pure, testable), but
 * about EVERYTHING THAT FINISHED rather than about now.
 */

const lane = (
  over: Partial<TurnLane> & Pick<TurnLane, 'taskId' | 'device' | 'status'>
): TurnLane => ({
  seq: 0,
  ...over,
});

const DONE_DIGEST = {
  phase: 'done',
  model: 'mihai-qwen3.6-27b',
  thinking_chars: 38_780,
  thinking_bytes: 128_270,
  transcript_bytes: 24_102,
  full_thinking: 'the durable reasoning tail',
};

describe('deriveNodeHistory', () => {
  it('lists a closed laneSource under its device and keeps a running one out', () => {
    const history = deriveNodeHistory({
      laneSources: [
        lane({ taskId: 'store', device: 'mihai', status: 'done', seq: 1 }),
        lane({ taskId: 'web', device: 'mihai', status: 'error', seq: 2 }),
        lane({ taskId: 'live-now', device: 'mihai', status: 'running', seq: 3 }),
      ],
      digests: {},
      digestMtimes: {},
    });
    const mihai = history.get('mihai') ?? [];
    expect(mihai.map((h) => h.lane.taskId)).toEqual(['store', 'web']);
  });

  it('discovers a digest-only finished call (verify::) via its model, joined through digestStreamFields', () => {
    const history = deriveNodeHistory({
      laneSources: [],
      digests: { 'verify::store': DONE_DIGEST },
      digestMtimes: { 'verify::store': 1_000 },
    });
    const entry = (history.get('mihai') ?? [])[0];
    expect(entry?.lane.taskId).toBe('verify::store');
    // The ONE join supplied the durable sizes — the fields were not hand-copied.
    expect(entry?.lane.thinkingBytes).toBe(128_270);
    expect(entry?.lane.transcriptBytes).toBe(24_102);
    expect(entry?.lastWriteMs).toBe(1_000);
  });

  it('does NOT invent an end for a digest-only key that never stamped done', () => {
    const history = deriveNodeHistory({
      laneSources: [],
      digests: { 'verify::store': { ...DONE_DIGEST, phase: undefined } },
      digestMtimes: { 'verify::store': 1_000 },
    });
    expect(history.size).toBe(0);
  });

  it('never double-lists a key that laneSources already claim (running or closed)', () => {
    const history = deriveNodeHistory({
      laneSources: [lane({ taskId: 'verify::store', device: 'mihai', status: 'running' })],
      digests: { 'verify::store': DONE_DIGEST },
      digestMtimes: { 'verify::store': 1_000 },
    });
    expect(history.size).toBe(0);
  });

  it('orders entries chronologically by their last durable write', () => {
    const history = deriveNodeHistory({
      laneSources: [
        lane({ taskId: 'later', device: 'mihai', status: 'done', seq: 1 }),
        lane({ taskId: 'earlier', device: 'mihai', status: 'done', seq: 2 }),
      ],
      digests: {},
      digestMtimes: { later: 2_000, earlier: 1_000 },
    });
    expect((history.get('mihai') ?? []).map((h) => h.lane.taskId)).toEqual(['earlier', 'later']);
  });
});

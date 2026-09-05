import { describe, expect, it } from 'vitest';
import type { TurnLane } from './useSwarmRun';
import { DIGEST_FRESH_MS, DIGEST_OPEN_CALL_FRESH_MS, deriveNodeHistory, poolNodeMap } from './useSwarmRun';

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

const NOW = 10_000_000;

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
      now: NOW,
    });
    const mihai = history.get('mihai') ?? [];
    expect(mihai.map((h) => h.lane.taskId)).toEqual(['store', 'web']);
  });

  it('discovers a digest-only finished call (verify::) via its model, joined through digestStreamFields', () => {
    const history = deriveNodeHistory({
      laneSources: [],
      digests: { 'verify::store': DONE_DIGEST },
      digestMtimes: { 'verify::store': 1_000 },
      now: NOW,
    });
    const entry = (history.get('mihai') ?? [])[0];
    expect(entry?.lane.taskId).toBe('verify::store');
    // The ONE join supplied the durable sizes — the fields were not hand-copied.
    expect(entry?.lane.thinkingBytes).toBe(128_270);
    expect(entry?.lane.transcriptBytes).toBe(24_102);
    expect(entry?.lastWriteMs).toBe(1_000);
  });

  // PANEL #2's FIND, closed by panel #5: a digest-only laneless call that died without phase:'done'
  // used to VANISH the moment its file went stale — live it was a fleet row, demoted it was nothing.
  // The staleness (same windows deriveFleet demotes by) is the LIVENESS fact that the call is over,
  // so it becomes an honest INTERRUPTED row rather than either a fake 'finished' or an absence.
  describe('a digest-only key that never stamped done', () => {
    const open = { ...DONE_DIGEST, phase: undefined };

    it('is NOT listed while fresh — the fleet strip is showing it live', () => {
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'verify::store': open },
        digestMtimes: { 'verify::store': NOW - DIGEST_FRESH_MS + 5_000 },
        now: NOW,
      });
      expect(history.size).toBe(0);
    });

    it('is listed INTERRUPTED once stale — captioned by the flag, never called finished', () => {
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'verify::store': open },
        digestMtimes: { 'verify::store': NOW - DIGEST_FRESH_MS - 5_000 },
        now: NOW,
      });
      const entry = (history.get('mihai') ?? [])[0];
      expect(entry?.lane.taskId).toBe('verify::store');
      expect(entry?.lane.interrupted).toBe(true);
      expect(entry?.lane.status).toBe('error');
      // Still the ONE join: the durable sizes ride along like on every other path.
      expect(entry?.lane.thinkingBytes).toBe(128_270);
    });

    it('honors the digest\'s own open-call record — a long silent tool call is not an interruption', () => {
      const withOpenCall = { ...open, calls: [{ name: 'shell', summary: 'cargo build', ok: null }] };
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'verify::store': withOpenCall },
        digestMtimes: { 'verify::store': NOW - DIGEST_FRESH_MS - 5_000 },
        now: NOW,
      });
      expect(history.size).toBe(0);
      const stale = deriveNodeHistory({
        laneSources: [],
        digests: { 'verify::store': withOpenCall },
        digestMtimes: { 'verify::store': NOW - DIGEST_OPEN_CALL_FRESH_MS - 5_000 },
        now: NOW,
      });
      expect((stale.get('mihai') ?? [])[0]?.lane.interrupted).toBe(true);
    });

    it('an UNKNOWN mtime corroborates nothing — no interruption is invented from it', () => {
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'verify::store': open },
        digestMtimes: {},
        now: NOW,
      });
      expect(history.size).toBe(0);
    });
  });

  it('never double-lists a key that laneSources already claim (running or closed)', () => {
    const history = deriveNodeHistory({
      laneSources: [lane({ taskId: 'verify::store', device: 'mihai', status: 'running' })],
      digests: { 'verify::store': DONE_DIGEST },
      digestMtimes: { 'verify::store': 1_000 },
      now: NOW,
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
      now: NOW,
    });
    expect((history.get('mihai') ?? []).map((h) => h.lane.taskId)).toEqual(['earlier', 'later']);
  });

  describe('the sidecar model id files under the sidecar node (the pool join, same as deriveFleet)', () => {
    const MIXED_POOL = [
      { id: 'workhorse-27b', model_id: 'workhorse-qwen3.8-27b', engine: 'lmstudio', node: 'workhorse' },
      {
        id: 'workhorse-mlx',
        model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
        engine: 'mlx-sidecar',
        node: 'workhorse-mlx',
      },
    ];
    const MLX_DONE = { ...DONE_DIGEST, model: 'workhorse-qwen3.5-9b-4bit-mlx' };

    it('MLX-only pool: a finished `judge-<task>` call is the MLX node\'s history, not a phantom `workhorse`', () => {
      const poolNodes = poolNodeMap([{ event: 'pool_resolved', devices: [MIXED_POOL[1]] }]);
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'judge-ledgerd-core': MLX_DONE },
        digestMtimes: { 'judge-ledgerd-core': 1_000 },
        now: NOW,
        poolNodes,
      });
      expect(Array.from(history.keys())).toEqual(['workhorse-mlx']);
      expect(history.get('workhorse-mlx')?.[0]?.lane.thinkingBytes).toBe(128_270);
    });

    it('mixed pool: the sidecar call files under `workhorse-mlx`; `workhorse` has no entry', () => {
      const poolNodes = poolNodeMap([{ event: 'pool_resolved', devices: MIXED_POOL }]);
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'judge-ledgerd-core': MLX_DONE },
        digestMtimes: { 'judge-ledgerd-core': 1_000 },
        now: NOW,
        poolNodes,
      });
      expect(history.get('workhorse-mlx')?.[0]?.lane.taskId).toBe('judge-ledgerd-core');
      expect(history.has('workhorse')).toBe(false);
    });

    it('without the map the prefix rule files it under `workhorse` — the pre-fix misattribution, pinned', () => {
      const history = deriveNodeHistory({
        laneSources: [],
        digests: { 'judge-ledgerd-core': MLX_DONE },
        digestMtimes: { 'judge-ledgerd-core': 1_000 },
        now: NOW,
      });
      expect(Array.from(history.keys())).toEqual(['workhorse']);
    });
  });
});

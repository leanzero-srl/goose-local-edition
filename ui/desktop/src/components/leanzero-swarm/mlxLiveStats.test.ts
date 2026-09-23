import { describe, expect, it } from 'vitest';
import {
  NO_RATES,
  SPARK_WINDOW,
  advanceLastRates,
  advanceMountWatch,
  compactTokens,
  formatElapsed,
  formatRate,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  mountCost,
  mountFill,
  parseMlxLiveStatus,
  pushSample,
  sparklinePoints,
  type MlxLiveStats,
} from './mlxLiveStats';
import {
  CACHED_GENERATING_STATUS,
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from './mlxLiveStatus.fixtures';

const GIB = 1024 * 1024 * 1024;

function statsOf(body: unknown): MlxLiveStats {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return read.stats;
}

describe('parseMlxLiveStatus — the real engine body', () => {
  it('reads the verbatim idle body from the running engine', () => {
    const s = statsOf(IDLE_STATUS);
    expect(s.engineStatus).toBe('idle');
    expect(s.uptimeS).toBe(874.3);
    expect(s.generationTps).toBe(19.91);
    expect(s.numRunning).toBe(0);
    expect(s.numWaiting).toBe(0);
    expect(s.activeMemoryGb).toBe(54.28);
    expect(s.cacheHitRate).toBe(0.2);
    expect(s.requests).toEqual([]);
  });

  it("reads the engine's lifetime counters and the tokens the prefix cache saved", () => {
    const s = statsOf(IDLE_STATUS);
    expect(s.totalRequests).toBe(5);
    expect(s.totalPromptTokens).toBe(91743);
    expect(s.totalCompletionTokens).toBe(672);
    expect(s.cacheTokensSaved).toBe(45056);
    const bare = statsOf({ status: 'idle', requests: [] });
    expect(bare.totalRequests).toBeNull();
    expect(bare.cacheTokensSaved).toBeNull();
  });

  it('reads every in-flight request with its identity, phase and token counts', () => {
    const s = statsOf(GENERATING_STATUS);
    expect(s.requests.map((r) => [r.id, r.status, r.phase])).toEqual([
      ['req-waiting-3', 'waiting', 'queued'],
      ['req-gen-1', 'running', 'generation'],
      ['req-prefill-2', 'running', 'prefill'],
    ]);
    const gen = s.requests[1];
    expect(gen.completionTokens).toBe(28035);
    expect(gen.maxTokens).toBe(32768);
    expect(gen.promptTokens).toBe(32277);
    expect(gen.elapsedS).toBe(1574);
    expect(gen.tokensPerSecond).toBe(19.9);
    expect(gen.ttftS).toBe(165);
    expect(gen.cachedTokens).toBe(0);
    expect(s.requests[2].ttftS).toBeNull();
    expect(s.activeMemoryGb).toBe(50.7);
    expect(s.cacheHitRate).toBe(0.78);
  });

  it('a body that is not a status body is a NAMED failure, not an empty instrument', () => {
    expect(parseMlxLiveStatus({ detail: 'Not Found' }).ok).toBe(false);
    expect(parseMlxLiveStatus('nope').ok).toBe(false);
    expect(parseMlxLiveStatus(null).ok).toBe(false);
  });

  it('absent sub-objects stay absent (null), never a fabricated 0', () => {
    const s = statsOf({ status: 'not_loaded', model: null, requests: [] });
    expect(s.activeMemoryGb).toBeNull();
    expect(s.cacheHitRate).toBeNull();
    expect(s.generationTps).toBeNull();
    // `cache: {enabled:false}` (prefix cache off) has no hit rate to show.
    expect(statsOf({ ...IDLE_STATUS, cache: { enabled: false } }).cacheHitRate).toBeNull();
  });
});

describe('activity and the live decode rate', () => {
  it('generating when any running request is past its first token', () => {
    expect(mlxActivity(statsOf(GENERATING_STATUS))).toBe('generating');
    expect(liveDecodeTps(statsOf(GENERATING_STATUS))).toBe(19.9);
  });

  it('a request still reading its prompt is prefill, and nothing is being decoded', () => {
    const s = statsOf(PREFILL_STATUS);
    expect(mlxActivity(s)).toBe('prefill');
    // The engine's generation_tps is STICKY (19.91 from the last run); the live rate is 0.
    expect(s.generationTps).toBe(19.91);
    expect(liveDecodeTps(s)).toBe(0);
  });

  it('idle reads 0 live even though the engine still reports the last rate', () => {
    const s = statsOf(IDLE_STATUS);
    expect(mlxActivity(s)).toBe('idle');
    expect(liveDecodeTps(s)).toBe(0);
  });

  it('only waiting requests read as queued; an unloaded engine says so', () => {
    const waitingOnly = { ...IDLE_STATUS, requests: [GENERATING_STATUS.requests[0]] };
    expect(mlxActivity(statsOf(waitingOnly))).toBe('queued');
    expect(mlxActivity(statsOf({ status: 'not_loaded', requests: [] }))).toBe('not_loaded');
  });
});

describe('sparkline windowing', () => {
  it('appends in engine-uptime order and keeps only the window', () => {
    let h = pushSample([], { uptimeS: 0, tps: 0 });
    for (let i = 1; i < SPARK_WINDOW + 10; i++) h = pushSample(h, { uptimeS: i * 2, tps: i });
    expect(h).toHaveLength(SPARK_WINDOW);
    expect(h[0].uptimeS).toBe(20);
    expect(h[h.length - 1].tps).toBe(SPARK_WINDOW + 9);
  });

  it('a second read of the same engine tick replaces the last point instead of doubling it', () => {
    const h = pushSample(
      [
        { uptimeS: 10, tps: 5 },
        { uptimeS: 12, tps: 6 },
      ],
      { uptimeS: 12, tps: 7 }
    );
    expect(h).toEqual([
      { uptimeS: 10, tps: 5 },
      { uptimeS: 12, tps: 7 },
    ]);
  });

  it('an uptime that went backwards is a restarted engine: the old history is dropped', () => {
    const h = pushSample(
      [
        { uptimeS: 900, tps: 19 },
        { uptimeS: 902, tps: 20 },
      ],
      { uptimeS: 3, tps: 0 }
    );
    expect(h).toEqual([{ uptimeS: 3, tps: 0 }]);
  });

  it('points scale to the window peak; fewer than two samples draw nothing', () => {
    expect(sparklinePoints([{ uptimeS: 1, tps: 3 }], 100, 10)).toBeNull();
    expect(
      sparklinePoints(
        [
          { uptimeS: 1, tps: 0 },
          { uptimeS: 3, tps: 10 },
          { uptimeS: 5, tps: 5 },
        ],
        100,
        10
      )
    ).toBe('0.0,10.0 50.0,0.0 100.0,5.0');
    // All-zero (idle) is a flat line on the floor, not a divide-by-zero.
    expect(
      sparklinePoints(
        [
          { uptimeS: 1, tps: 0 },
          { uptimeS: 3, tps: 0 },
        ],
        100,
        10
      )
    ).toBe('0.0,10.0 100.0,10.0');
  });
});

describe('the mount memory fill', () => {
  it('a mount seen from its start measures claimed memory against the model size', () => {
    let w = advanceMountWatch(null, 'm', 90, 96.6);
    expect(w).toEqual({ modelId: 'm', peakFreeGb: 96.6, sawStart: true });
    w = advanceMountWatch(w, 'm', 80.6, null);
    const fill = mountFill(w, 80.6, 31 * GIB)!;
    expect(fill.modelGb).toBe(31);
    expect(fill.claimedGb).toBeCloseTo(16, 5);
    expect(fill.fraction).toBeCloseTo(16 / 31, 5);
  });

  it('a switch frees the old model first: the peak free memory is the baseline', () => {
    let w = advanceMountWatch(null, 'new', 60, 60);
    w = advanceMountWatch(w, 'new', 95, 60); // old model unloaded
    w = advanceMountWatch(w, 'new', 75, 60); // new weights arriving
    expect(w.peakFreeGb).toBe(95);
    expect(mountFill(w, 75, 31 * GIB)!.claimedGb).toBeCloseTo(20, 5);
  });

  it('a view opened mid-mount has no baseline: size only, no fraction', () => {
    const w = advanceMountWatch(null, 'm', 70, null);
    expect(w.sawStart).toBe(false);
    expect(mountFill(w, 70, 31 * GIB)).toEqual({ modelGb: 31, claimedGb: 0, fraction: null });
  });

  it('the fraction never passes 1, and an unknown size draws nothing', () => {
    const w = advanceMountWatch(null, 'm', 96, 96);
    expect(mountFill(w, 40, 31 * GIB)!.fraction).toBe(1);
    expect(mountFill(w, 40, null)).toBeNull();
    expect(mountFill(w, 40, 0)).toBeNull();
  });
});

describe('what a mount would cost (the sidecar gate verdict)', () => {
  it('the running Mac: 31 GB model, 68.6 of 128 GB free — fits above the 12.8 GB reserve', () => {
    const c = mountCost(31 * GIB, 68.6, 128);
    expect(c.reserveGb).toBeCloseTo(12.8, 5);
    expect(c.spareGb).toBeCloseTo(24.8, 5);
    expect(c.verdict).toBe('fits');
  });

  it('under 4 GB above the reserve is tight; past it the gate would refuse', () => {
    expect(mountCost(31 * GIB, 46, 128).verdict).toBe('tight');
    const no = mountCost(31 * GIB, 40, 128);
    expect(no.verdict).toBe('no-fit');
    expect(no.spareGb).toBeCloseTo(-3.8, 5);
  });

  it('the reserve floor is 8 GB on a small machine', () => {
    expect(mountCost(4 * GIB, 20, 32).reserveGb).toBe(8);
  });
});

describe('formatters', () => {
  it('compact token counts and engine elapsed seconds', () => {
    expect(compactTokens(32277)).toBe('32k');
    expect(compactTokens(1500)).toBe('1.5k');
    expect(compactTokens(640)).toBe('640');
    expect(formatElapsed(45.4)).toBe('45s');
    expect(formatElapsed(1574)).toBe('26m 14s');
    expect(formatElapsed(3725)).toBe('1h 2m');
  });
});

describe('a rate needs two tokens — the engine aggregate is never shown (2026-09-23: 1,048,576 tok/s)', () => {
  it('a one-token request reads as no rate, not 2^20', () => {
    const body = {
      status: 'generating',
      generation_tps: 1048576.0,
      requests: [
        {
          request_id: 'title',
          status: 'running',
          phase: 'generation',
          completion_tokens: 1,
          tokens_per_second: 1048576.0,
        },
      ],
    };
    const read = parseMlxLiveStatus(body);
    if (!read.ok) throw new Error(read.detail);
    expect(liveDecodeTps(read.stats)).toBe(0);
  });
});

describe('the prefill (reading) rate — computed prompt tokens over time to first token', () => {
  it('an uncached 32,277-token prompt read in 165 s is 195.6 tok/s (the engine said 195.5)', () => {
    expect(measuredPrefillTps(statsOf(GENERATING_STATUS))).toBeCloseTo(32277 / 165, 5);
  });

  it('cached prompt tokens are not counted as read: (33,000 − 30,600) / 12 s = 200 tok/s', () => {
    expect(measuredPrefillTps(statsOf(CACHED_GENERATING_STATUS))).toBeCloseTo(200, 5);
  });

  it('a request still reading its prompt has no rate yet; idle has none', () => {
    expect(measuredPrefillTps(statsOf(PREFILL_STATUS))).toBe(0);
    expect(measuredPrefillTps(statsOf(IDLE_STATUS))).toBe(0);
  });

  it('a fully cached prompt computed nothing, so it yields no rate rather than infinity', () => {
    const body = {
      ...CACHED_GENERATING_STATUS,
      requests: [{ ...CACHED_GENERATING_STATUS.requests[0], cached_tokens: 33000 }],
    };
    expect(measuredPrefillTps(statsOf(body))).toBe(0);
  });

  it('of two generating requests, the one whose first token landed last is the reading rate', () => {
    const older = { ...GENERATING_STATUS.requests[1] }; // first token 1,409 s ago
    const newer = { ...CACHED_GENERATING_STATUS.requests[0] }; // first token 18 s ago
    const body = { ...IDLE_STATUS, status: 'generating', requests: [older, newer] };
    expect(measuredPrefillTps(statsOf(body))).toBeCloseTo(200, 5);
  });
});

describe('last measured rates — what an idle tile or tray states as "last run"', () => {
  it('keeps the last writing and reading rates through idle reads', () => {
    let last = advanceLastRates(NO_RATES, statsOf(GENERATING_STATUS));
    expect(last.decodeTps).toBe(19.9);
    expect(last.prefillTps).toBeCloseTo(195.6, 1);
    last = advanceLastRates(last, statsOf({ ...IDLE_STATUS, uptime_s: 1990 }));
    expect(last.decodeTps).toBe(19.9);
    expect(last.prefillTps).toBeCloseTo(195.6, 1);
    expect(last.uptimeS).toBe(1990);
  });

  it('nothing measured yet is null — never the sticky engine aggregate', () => {
    const last = advanceLastRates(NO_RATES, statsOf(IDLE_STATUS));
    expect(last).toEqual({ uptimeS: 874.3, decodeTps: null, prefillTps: null });
  });

  it('an engine whose uptime went backwards restarted: its old rates are dropped', () => {
    const before = advanceLastRates(NO_RATES, statsOf(GENERATING_STATUS));
    const after = advanceLastRates(before, statsOf({ ...IDLE_STATUS, uptime_s: 3 }));
    expect(after).toEqual({ uptimeS: 3, decodeTps: null, prefillTps: null });
  });

  it('rates read at a glance: one decimal under 100, whole numbers above', () => {
    expect(formatRate(19.94, 'en-US')).toBe('19.9');
    expect(formatRate(1240.4, 'en-US')).toBe('1,240');
  });
});

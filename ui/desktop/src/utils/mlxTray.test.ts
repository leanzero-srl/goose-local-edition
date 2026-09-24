import { describe, expect, it } from 'vitest';
import {
  MLX_DISTRIBUTED_STALE_MS,
  PHASE_GLYPH,
  buildMlxTrayModel,
  mlxTrayTitle,
  trayTitleText,
  type MlxTrayItem,
} from './mlxTray';
import { phaseDotBitmap } from './phaseDot';
import type { MlxDistributedStatus } from '../acp/mlx-distributed';
import { toMlxDistributedReport } from './mlxDistributedReport';
import {
  FLASH_READY,
  FLASH_SERVING,
  HOSTING_RANK_1,
  STOPPED_WITH_CONFIG,
} from '../components/leanzero-swarm/mlxDistributed.fixtures';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import { attributeServing, type MlxServingRow } from './mlxServing';
import {
  NO_RATES,
  advanceLastRates,
  parseMlxLiveStatus,
  type MlxLiveStats,
} from '../components/leanzero-swarm/mlxLiveStats';
import {
  DIST_READING_STATUS,
  DIST_WRITING_STATUS,
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';

const MODEL = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const CONFIGURED = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';

function statsOf(body: unknown): MlxLiveStats {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return read.stats;
}

function running(body: unknown, over: Partial<MlxEngineSnapshot> = {}): MlxEngineSnapshot {
  const stats = statsOf(body);
  return {
    ...INITIAL_SNAPSHOT,
    mode: 'running',
    modelId: MODEL,
    baseUrl: 'http://127.0.0.1:8090',
    stats,
    last: advanceLastRates(NO_RATES, stats),
    serving: attributeServing([], 0, [], null),
    ...over,
  };
}

const labels = (items: MlxTrayItem[]) =>
  items.map((i) => (i.type === 'separator' ? '---' : i.label));

const actions = (items: MlxTrayItem[]) =>
  items.flatMap((i) => (i.type === 'action' ? [[i.action, i.label, i.enabled]] : []));

const OPTS = { canAct: true, mountModelId: CONFIGURED, distributed: null };

describe('mlxTrayTitle — the text beside the menu-bar icon', () => {
  it('writing shows the live writing rate; reading shows the prompt size; idle says so', () => {
    expect(mlxTrayTitle(running(GENERATING_STATUS))).toBe('19.9 tok/s');
    expect(mlxTrayTitle(running(PREFILL_STATUS))).toBe('Reading 32k');
    expect(mlxTrayTitle(running(IDLE_STATUS))).toBe('Idle');
  });

  it('no engine, nothing; mounting and failed name themselves', () => {
    expect(mlxTrayTitle({ ...INITIAL_SNAPSHOT, mode: 'off' })).toBe('');
    expect(mlxTrayTitle(INITIAL_SNAPSHOT)).toBe('');
    expect(mlxTrayTitle({ ...INITIAL_SNAPSHOT, mode: 'mounting' })).toBe('Mounting');
    expect(mlxTrayTitle({ ...INITIAL_SNAPSHOT, mode: 'failed' })).toBe('MLX failed');
  });

  it('a request that has written one token has no rate yet: the word, never 2^20 tok/s', () => {
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
    expect(mlxTrayTitle(running(body))).toBe('Writing');
  });
});

describe('buildMlxTrayModel — the engine section of the tray menu, per state', () => {
  it('writing: state, model, both rates, who, cache saved, lifetime totals, then Unmount', () => {
    const rows: MlxServingRow[] = [
      {
        id: 1,
        via: 'swarmRouter',
        sessionId: '20260923_7',
        provider: 'omlx',
        model: MODEL,
        nodeId: 'mihai-mlx',
        startedAt: '2026-09-23T20:00:00Z',
        sessionName: 'Memory · verify recall',
        sessionType: 'user',
        sessionError: null,
      },
    ];
    const model = buildMlxTrayModel(
      running(GENERATING_STATUS, { serving: attributeServing(rows, 3, ['bench-r9'], null) }),
      OPTS
    );
    expect(model.title).toBe('19.9 tok/s');
    expect(labels(model.items)).toEqual([
      'LeanZero MLX: writing',
      `Model: ${MODEL}`,
      'Writing 19.9 tok/s',
      'Reading a 32k-token prompt for 2m 45s',
      'Read the last prompt at 196 tok/s',
      'Serving chat: Memory · verify recall',
      "2 requests not from this app's chats or /v1",
      'Swarm run live: bench-r9',
      'Cache saved 45k prompt tokens, 78% of lookups hit',
      'Served 5 requests, 92k read, 672 written',
      'Up 31m 14s, 50.7 GB GPU memory',
      '---',
      'Open Providers',
      'Unmount the MLX engine',
    ]);
    expect(actions(model.items)).toEqual([
      ['open-providers', 'Open Providers', true],
      ['unmount', 'Unmount the MLX engine', true],
    ]);
  });

  it('idle: the last measured rates as facts, no live rate invented', () => {
    const idle = running(IDLE_STATUS, {
      last: advanceLastRates(NO_RATES, statsOf(GENERATING_STATUS)),
    });
    const got = labels(buildMlxTrayModel(idle, OPTS).items);
    expect(got[0]).toBe('LeanZero MLX: idle');
    expect(got).toContain('Last run: wrote 19.9 tok/s, read 196 tok/s');
    expect(got.some((l) => l.startsWith('Writing '))).toBe(false);
  });

  it('an external client routed to the engine is named as such', () => {
    const rows: MlxServingRow[] = [
      {
        id: 1,
        via: 'openaiApi',
        sessionId: 'e1',
        provider: 'omlx',
        model: MODEL,
        nodeId: null,
        startedAt: '2026-09-23T20:00:00Z',
        sessionName: 'OpenAI-compatible request',
        sessionType: 'user',
        sessionError: null,
      },
    ];
    const got = labels(
      buildMlxTrayModel(
        running(PREFILL_STATUS, { serving: attributeServing(rows, 1, [], null) }),
        OPTS
      ).items
    );
    expect(got).toContain(`Serving an external client via /v1: omlx/${MODEL}`);
  });

  it('not mounted: Mount the configured model, enabled only when a window can carry the call', () => {
    const off = { ...INITIAL_SNAPSHOT, mode: 'off' as const, baseUrl: 'http://127.0.0.1:8090' };
    const model = buildMlxTrayModel(off, OPTS);
    expect(model.title).toBe('');
    expect(labels(model.items)[0]).toBe('LeanZero MLX: not mounted');
    expect(actions(model.items)).toEqual([
      ['open-providers', 'Open Providers', true],
      ['mount', 'Mount Qwen3.8-27B-Atlassian-Q8-mlx', true],
    ]);
    expect(
      actions(
        buildMlxTrayModel(off, { canAct: false, mountModelId: CONFIGURED, distributed: null }).items
      )
    ).toEqual([
      ['open-providers', 'Open Providers', false],
      ['mount', 'Mount Qwen3.8-27B-Atlassian-Q8-mlx', false],
    ]);
    expect(
      actions(
        buildMlxTrayModel(off, { canAct: true, mountModelId: null, distributed: null }).items
      )[1]
    ).toEqual(['mount', 'Mount (pick a model in Providers first)', false]);
  });

  it('failed: the error, and Mount to retry', () => {
    const failed = {
      ...INITIAL_SNAPSHOT,
      mode: 'failed' as const,
      modelId: CONFIGURED,
      failedError: 'port 8090 never opened',
    };
    const model = buildMlxTrayModel(failed, OPTS);
    expect(labels(model.items)).toEqual([
      'LeanZero MLX: failed',
      `Model: ${CONFIGURED}`,
      'Error: port 8090 never opened',
      '---',
      'Open Providers',
      'Mount Qwen3.8-27B-Atlassian-Q8-mlx',
    ]);
  });

  it('a stale read says why, and an unreadable "who" says so instead of listing nobody', () => {
    const stale = running(GENERATING_STATUS, {
      statusDetail: 'timeout: no answer within 1500 ms',
      serving: attributeServing([], 3, [], 'goose backend returned 401'),
    });
    const got = labels(buildMlxTrayModel(stale, OPTS).items);
    expect(got).toContain('Stale: timeout: no answer within 1500 ms');
    expect(got).toContain("3 requests not from this app's chats or /v1");
    expect(got).toContain('Who is unknown: goose backend returned 401');
  });
});

describe('the tray while the DISTRIBUTED engine owns this Mac', () => {
  const ready = toMlxDistributedReport(FLASH_READY);
  const fresh = (report = ready) => ({ ...OPTS, distributed: { report, ageMs: 500 } });

  it('the title is the run: ready, requests in flight while serving, held when admission closes', () => {
    expect(buildMlxTrayModel(INITIAL_SNAPSHOT, fresh()).title).toBe('Dist · ready');
    expect(
      buildMlxTrayModel(INITIAL_SNAPSHOT, fresh(toMlxDistributedReport(FLASH_SERVING))).title
    ).toBe('Dist · 2 in flight');
    expect(
      buildMlxTrayModel(
        INITIAL_SNAPSHOT,
        fresh(toMlxDistributedReport({ ...FLASH_SERVING, admissionOpen: false }))
      ).title
    ).toBe('Dist · held');
  });

  it('the menu names the mode, the nodes and backend, per-node memory, restarts and the last alarm', () => {
    const model = buildMlxTrayModel({ ...INITIAL_SNAPSHOT, mode: 'off' }, fresh());
    expect(labels(model.items)).toEqual([
      'LeanZero MLX: distributed, ready',
      'Distributed · MacBook Pro + workhorse · JACCL',
      'Model: rapid-mlx/Qwen3.8-Flash-Next-4bit',
      'MacBook Pro: L0–19 · peak 61.0 of 83.4 GiB budget',
      'workhorse: L20–47 · peak 42.5 of 55.4 GiB budget',
      'In flight: 0',
      'Restarts: 1',
      'Last: restart — restart 1 of the breaker window',
      '---',
      'Open Providers',
      'Stop the distributed engine',
    ]);
    // Mount is refused by goose while the run owns the Mac, so the tray does not offer it.
    expect(actions(model.items).map(([a]) => a)).toEqual(['open-providers', 'stop-distributed']);
  });

  it("while up, main's read of rank 0 speaks through the single engine's words and colours", () => {
    const serving = toMlxDistributedReport({ ...FLASH_SERVING, inflight: 1 });
    const rank0 = (body: unknown) =>
      running(body, {
        engine: 'distributed',
        modelId: null,
        baseUrl: 'http://127.0.0.1:8091',
      });
    const reading = buildMlxTrayModel(rank0(DIST_READING_STATUS), fresh(serving));
    expect(reading.title).toBe('Dist · Reading 7.0k');
    expect(reading.phase).toBe('reading');
    expect(labels(reading.items)).toContain('Reading a 7.0k-token prompt, 2.0k read for 14s');
    expect(labels(reading.items)).toContain('Reading at 152 tok/s');
    expect(labels(reading.items)).not.toContain('In flight: 1');
    expect(reading.items[0]).toMatchObject({ phase: 'reading' });

    const writing = buildMlxTrayModel(rank0(DIST_WRITING_STATUS), fresh(serving));
    expect(writing.title).toBe('Dist · 171 tok/s');
    expect(writing.phase).toBe('writing');
    expect(labels(writing.items)).toContain('Writing 171 tok/s');

    // A read of the single engine never speaks for the run: the counters do.
    const single = buildMlxTrayModel(running(DIST_WRITING_STATUS), fresh(serving));
    expect(single.title).toBe('Dist · 1 in flight');
    expect(single.phase).toBe('writing');
    // And a rank 0 read never speaks for the single engine once the run is gone.
    const gone = buildMlxTrayModel(rank0(DIST_WRITING_STATUS), OPTS);
    expect(gone.title).toBe('');
  });

  it('a node under pressure or unread says so on its line', () => {
    const report = toMlxDistributedReport({
      ...FLASH_READY,
      nodes: [
        { ...FLASH_READY.nodes[0], pressure: 'warn' },
        { ...FLASH_READY.nodes[1], state: 'failed', memoryError: 'ssh workhorse: timed out' },
      ],
    });
    const lines = labels(buildMlxTrayModel(INITIAL_SNAPSHOT, fresh(report)).items);
    expect(lines).toContain('MacBook Pro: L0–19 · peak 61.0 of 83.4 GiB budget · pressure warn');
    expect(lines).toContain('workhorse (failed): memory unread — ssh workhorse: timed out');
  });

  it('a read older than three polls is SAID to be stale, never shown as live', () => {
    const stale = { ...OPTS, distributed: { report: ready, ageMs: MLX_DISTRIBUTED_STALE_MS + 1 } };
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, stale);
    expect(model.title).toBe('Dist · stale');
    expect(labels(model.items)).toContain('Not refreshed for 6s — open goose to read it again');
  });

  it('single mode with a distributed report: the menu says "Single · this Mac"; a failed run is named', () => {
    const stopped = toMlxDistributedReport(STOPPED_WITH_CONFIG);
    const single = buildMlxTrayModel(running(IDLE_STATUS), {
      ...OPTS,
      distributed: { report: stopped, ageMs: 0 },
    });
    expect(labels(single.items).slice(0, 2)).toEqual(['LeanZero MLX: idle', 'Single · this Mac']);
    expect(single.title).toBe('Idle');

    const failed = toMlxDistributedReport({
      ...STOPPED_WITH_CONFIG,
      state: 'failed',
      lastError: 'rank 1 died: exit status 137',
    });
    const off = buildMlxTrayModel(
      { ...INITIAL_SNAPSHOT, mode: 'off' },
      {
        ...OPTS,
        distributed: { report: failed, ageMs: 0 },
      }
    );
    expect(off.title).toBe('Dist failed');
    expect(labels(off.items)).toContain('Distributed engine failed: rank 1 died: exit status 137');
    expect(actions(off.items).map(([a]) => a)).toEqual(['open-providers', 'mount']);
  });
});

describe('the tray while this Mac SERVES a rank of another Mac over LeanZero Link', () => {
  const hosting = toMlxDistributedReport(HOSTING_RANK_1);

  it('says whose engine, which model and backend; offers no Mount (the single engine is refused)', () => {
    const model = buildMlxTrayModel(
      { ...INITIAL_SNAPSHOT, mode: 'off' },
      { ...OPTS, distributed: { report: hosting, ageMs: 0 } }
    );
    expect(model.title).toBe('Rank 1 · serving');
    expect(labels(model.items)).toEqual([
      'LeanZero MLX: serving a rank, serving',
      "Rank 1 of MacBook Pro's distributed engine · JACCL",
      'Model: Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      'Single engine: refused while this Mac serves MacBook Pro',
      '---',
      'Open Providers',
    ]);
    expect(actions(model.items).map(([action]) => action)).toEqual(['open-providers']);
  });

  it('an old read is said to be old here too', () => {
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: { report: hosting, ageMs: MLX_DISTRIBUTED_STALE_MS + 1 },
    });
    expect(model.title).toBe('Rank · stale');
    expect(labels(model.items)).toContain('Not refreshed for 6s — open goose to read it again');
  });
});

/**
 * The tray speaks the tile's palette (lz tokens PHASE_*, mapped by mlxPhase.ts): the title leads
 * with the phase's colour glyph, every state line carries its phase for main's exact-hex dot.
 */
describe('the tray in the engine-phase palette', () => {
  const phases = (items: MlxTrayItem[]) =>
    items.flatMap((i) => (i.type === 'info' && i.phase ? [[i.label, i.phase]] : []));

  it.each<[string, MlxEngineSnapshot, string, string]>([
    ['writing', running(GENERATING_STATUS), 'writing', '🟢 19.9 tok/s'],
    ['reading', running(PREFILL_STATUS), 'reading', '🔵 Reading 32k'],
    ['idle', running(IDLE_STATUS), 'idle', '⚪ Idle'],
    ['mounting', { ...INITIAL_SNAPSHOT, mode: 'mounting' }, 'loading', '🟡 Mounting'],
    ['failed', { ...INITIAL_SNAPSHOT, mode: 'failed' }, 'failed', '🔴 MLX failed'],
  ])('single %s → the %s phase, title "%s"', (_name, snapshot, phase, title) => {
    const model = buildMlxTrayModel(snapshot, OPTS);
    expect(model.phase).toBe(phase);
    expect(trayTitleText(model)).toBe(title);
    expect(model.items[0]).toMatchObject({ type: 'info', phase });
  });

  it('queued requests are ORANGE', () => {
    const queued = running({
      status: 'idle',
      num_waiting: 1,
      requests: [{ request_id: 'q', status: 'waiting', phase: 'queued', prompt_tokens: 50 }],
    });
    const model = buildMlxTrayModel(queued, OPTS);
    expect(model.phase).toBe('held');
    expect(trayTitleText(model)).toBe(`${PHASE_GLYPH.held} Queued 1`);
  });

  it('not mounted: the dark dot on the menu line, and no title at all', () => {
    const model = buildMlxTrayModel({ ...INITIAL_SNAPSHOT, mode: 'off' }, OPTS);
    expect(trayTitleText(model)).toBe('');
    expect(phases(model.items)).toEqual([['LeanZero MLX: not mounted', 'unloaded']]);
  });

  it('distributed: the run and EACH node carry their own phase; a closed admission is orange', () => {
    const starting = toMlxDistributedReport({
      ...FLASH_READY,
      state: 'starting',
      nodes: [
        {
          ...FLASH_READY.nodes[0],
          state: 'loading',
          activeMemoryGb: 12,
          plannedWeightsGb: 48,
        },
        { ...FLASH_READY.nodes[1], state: 'ready' },
      ],
    } as MlxDistributedStatus);
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: { report: starting, ageMs: 0 },
    });
    expect(model.phase).toBe('loading');
    expect(trayTitleText(model)).toBe('🟡 Dist · starting');
    expect(phases(model.items)).toEqual([
      ['LeanZero MLX: distributed, starting', 'loading'],
      [
        'MacBook Pro (loading): L0–19 · loaded 12.0 of 48.0 GB · peak 61.0 of 83.4 GiB b…',
        'loading',
      ],
      ['workhorse (ready): L20–47 · peak 42.5 of 55.4 GiB budget', 'idle'],
    ]);
    const held = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: {
        report: toMlxDistributedReport({ ...FLASH_SERVING, admissionOpen: false }),
        ageMs: 0,
      },
    });
    expect(held.phase).toBe('held');
    expect(trayTitleText(held)).toBe('🟠 Dist · held');
  });

  it('a Mac macOS is making room on is said so, amber, with no load figure', () => {
    const report = toMlxDistributedReport({
      ...FLASH_READY,
      state: 'starting',
      makingRoom: ['workhorse'],
      nodes: [
        FLASH_READY.nodes[0],
        { ...FLASH_READY.nodes[1], state: 'loading', plannedWeightsGb: 80 },
      ],
    } as MlxDistributedStatus);
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: { report, ageMs: 0 },
    });
    expect(phases(model.items)[2]).toEqual([
      'workhorse (making room): L20–47 · peak 42.5 of 55.4 GiB budget',
      'loading',
    ]);
  });

  it('a stale read claims no colour — the words say stale', () => {
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      ...OPTS,
      distributed: {
        report: toMlxDistributedReport(FLASH_SERVING),
        ageMs: MLX_DISTRIBUTED_STALE_MS + 1,
      },
    });
    expect(model.phase).toBeNull();
    expect(trayTitleText(model)).toBe('Dist · stale');
    expect(phases(model.items)).toEqual([]);
  });

  it('the peer serving a rank: "loading rank 1 for MacBook Pro" in amber, grey once joined', () => {
    const loading = toMlxDistributedReport({
      ...HOSTING_RANK_1,
      hosting: {
        ...HOSTING_RANK_1.hosting!,
        state: 'loading',
        loadedBytes: 6 * 2 ** 30,
        plannedWeightBytes: 24 * 2 ** 30,
      },
    } as MlxDistributedStatus);
    const model = buildMlxTrayModel(
      { ...INITIAL_SNAPSHOT, mode: 'off' },
      { ...OPTS, distributed: { report: loading, ageMs: 0 } }
    );
    expect(trayTitleText(model)).toBe('🟡 Rank 1 · loading');
    expect(phases(model.items)).toEqual([
      ['LeanZero MLX: loading rank 1 for MacBook Pro, loaded 6.0 of 24.0 GB', 'loading'],
    ]);
    const joined = buildMlxTrayModel(
      { ...INITIAL_SNAPSHOT, mode: 'off' },
      { ...OPTS, distributed: { report: toMlxDistributedReport(HOSTING_RANK_1), ageMs: 0 } }
    );
    expect(joined.phase).toBe('idle');
  });

  it("main's menu dot is the phase's exact hex, a solid anti-aliased disc", () => {
    const px = 24;
    const buf = phaseDotBitmap('#f59e0b', px);
    const at = (x: number, y: number) => [...buf.subarray((y * px + x) * 4, (y * px + x) * 4 + 4)];
    // centre: fully opaque amber, BGRA
    expect(at(12, 12)).toEqual([0x0b, 0x9e, 0xf5, 255]);
    // corner: outside the disc, transparent
    expect(at(0, 0)).toEqual([0, 0, 0, 0]);
  });
});

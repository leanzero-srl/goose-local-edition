import { describe, expect, it } from 'vitest';
import { buildMlxTrayModel, mlxTrayTitle, type MlxTrayItem } from './mlxTray';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import { attributeServing, type MlxServingRow } from './mlxServing';
import {
  NO_RATES,
  advanceLastRates,
  parseMlxLiveStatus,
  type MlxLiveStats,
} from '../components/leanzero-swarm/mlxLiveStats';
import {
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

const OPTS = { canAct: true, mountModelId: CONFIGURED };

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
      actions(buildMlxTrayModel(off, { canAct: false, mountModelId: CONFIGURED }).items)
    ).toEqual([
      ['open-providers', 'Open Providers', false],
      ['mount', 'Mount Qwen3.8-27B-Atlassian-Q8-mlx', false],
    ]);
    expect(actions(buildMlxTrayModel(off, { canAct: true, mountModelId: null }).items)[1]).toEqual([
      'mount',
      'Mount (pick a model in Providers first)',
      false,
    ]);
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
      'LeanZero MLX: mount failed',
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

import { describe, expect, it, vi } from 'vitest';
import { testClock } from '../test/testClock';
import {
  MlxEngineMonitor,
  isMlxEngineReport,
  isMlxEngineSnapshot,
  mlxEngineConfigFromYaml,
  type MlxEngineMonitorDeps,
  type MlxEngineSnapshot,
} from './mlxEngineMonitor';
import type { MlxLiveStatusResult } from './mlxLiveStatus';
import { routePeerGone } from './routeContact';
import { leftoverPortOf } from '../acp/mlx-engine';
import type { MlxServingRead } from './mlxServing';
import { measuredFigure, type MeasuredRunsFetch, type MlxMeasuredRead } from './mlxMeasuredRuns';
import {
  DIST_READING_STATUS,
  GENERATING_STATUS,
  IDLE_STATUS,
  SPLIT_TURN_BEHIND_LEAVING_3M,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';
import {
  FLASH_MODEL,
  FLASH_READY,
  STOPPED_WITH_CONFIG,
} from '../components/leanzero-swarm/mlxDistributed.fixtures';
import { toMlxDistributedReport, type MlxDistributedReport } from './mlxDistributedReport';

const BASE = 'http://127.0.0.1:8090';
const RANK0 = 'http://127.0.0.1:8091';
/** A published route's Mac, as `routePeerName` names it; a test routes to it unless it names another. */
const PEER = 'Mac Studio';

/** goose's `distributedStatus` of an UP split whose rank 0 answers at `baseUrl`, as main is told. */
const splitUpAt = (baseUrl: string): MlxDistributedReport => ({
  ...toMlxDistributedReport(FLASH_READY),
  baseUrl,
});

/** A goosed's measured-runs answer for one way: `writing` is the median over `runs` runs. */
function wayAnswer(
  kind: 'single' | 'tensor',
  nodes: string[],
  writing: number,
  runs: number
): MeasuredRunsFetch {
  const figure = {
    estimate: { value: writing, low: writing * 0.9, high: writing * 1.1 },
    measured: true,
    runs,
  };
  return {
    ok: true,
    answer: {
      way: {
        placementId: `${kind}:${nodes.join('+')}`,
        placement: { kind, nodes },
        modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
        nodeNames: nodes,
      },
      wayError: null,
      recorded: runs,
      writing: figure,
      writingBasis: null,
      reading: null,
      readingByBucket: [],
      storeErrors: [],
    },
  };
}

const HERE = wayAnswer('single', ['local'], 21.5, 12);
const STUDIO = wayAnswer('single', ['link:studio'], 26.96, 303);
const SPLIT = wayAnswer('tensor', ['local', 'link:studio'], 8.19, 91);

const writingOf = (read: MlxMeasuredRead) =>
  read.kind === 'read' ? (measuredFigure(read.answer.writing)?.median ?? null) : null;

function harness(opts: {
  status: () => MlxLiveStatusResult;
  serving?: () => MlxServingRead;
  configBaseUrl?: string | null;
  /** goose's latest fresh `distributedStatus` report (the Run it row's read); null = none. */
  distributedRun?: () => MlxDistributedReport | null;
  /** A published route; a bare string is a READY route on that relay. */
  remoteRoute?: () =>
    | {
        state: string;
        baseUrl: string | null;
        peerName?: string;
        modelId?: string | null;
        lastError?: string | null;
      }
    | string
    | null;
  /** Every goose backend's measured-runs answer (one per backend). */
  measured?: () => MeasuredRunsFetch[];
}) {
  const snapshots: MlxEngineSnapshot[] = [];
  const clock = { ms: 0 };
  const scheduled: Array<() => void> = [];
  const readServing = vi.fn(async () => opts.serving?.() ?? { ok: true as const, rows: [] });
  const readStatus = vi.fn(async (_baseUrl: string) => opts.status());
  const readMeasured = vi.fn(async () => opts.measured?.() ?? [HERE, STUDIO, SPLIT]);
  const deps: MlxEngineMonitorDeps = {
    readStatus,
    readServing,
    readMeasured,
    configBaseUrl: () => (opts.configBaseUrl === undefined ? BASE : opts.configBaseUrl),
    distributedRun: () => opts.distributedRun?.() ?? null,
    remoteRoute: () => {
      const route = opts.remoteRoute?.() ?? null;
      if (typeof route === 'string') return { state: 'ready', baseUrl: route, peerName: PEER };
      return route && { peerName: PEER, ...route };
    },
    swarmRuns: () => ['bench-r9'],
    onSnapshot: (s) => snapshots.push(s),
    schedule: (fn) => {
      scheduled.push(fn);
      return () => {
        const i = scheduled.indexOf(fn);
        if (i >= 0) scheduled.splice(i, 1);
      };
    },
    intervalMs: 2000,
    now: () => clock.ms,
  };
  return {
    monitor: new MlxEngineMonitor(deps),
    snapshots,
    scheduled,
    readServing,
    readStatus,
    readMeasured,
    clock,
  };
}

const answered = (body: unknown): MlxLiveStatusResult => ({ ok: true, url: BASE, body });
const refused: MlxLiveStatusResult = {
  ok: false,
  url: BASE,
  error: 'unreachable',
  detail: 'connect ECONNREFUSED 127.0.0.1:8090',
};

describe('MlxEngineMonitor — one loop, running only while the engine answers', () => {
  it('an answering engine is RUNNING and schedules exactly one next read', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s.mode).toBe('running');
    expect(s.modelId).toBe('mihai-qwen3.8-27b-atlassian-q8-mlx');
    expect(s.stats?.totalRequests).toBe(5);
    expect(h.scheduled).toHaveLength(1);
    // Idle: nobody to attribute, so goose's list is not even read.
    expect(h.readServing).not.toHaveBeenCalled();
    expect(s.serving).toEqual({
      clients: [],
      unattributed: 0,
      swarmRuns: ['bench-r9'],
      error: null,
    });
  });

  it('a refused connection is the engine gone: OFF, and the loop STOPS', async () => {
    const h = harness({ status: () => refused });
    await h.monitor.tick();
    expect(h.monitor.current().mode).toBe('off');
    expect(h.monitor.current().statusDetail).toBe(
      'unreachable: connect ECONNREFUSED 127.0.0.1:8090'
    );
    expect(h.scheduled).toHaveLength(0);
  });

  it('goose saying "mounting" keeps the loop alive until the port opens', async () => {
    let body: MlxLiveStatusResult = refused;
    const h = harness({ status: () => body });
    h.monitor.reportFromRenderer({ state: 'mounting', baseUrl: BASE, modelId: 'org/m' });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    expect(h.monitor.current().mode).toBe('mounting');
    expect(h.monitor.current().modelId).toBe('org/m');
    expect(h.scheduled).toHaveLength(1);
    body = answered(IDLE_STATUS);
    h.scheduled.shift()!();
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(2), { timeout: testClock() });
    expect(h.monitor.current().mode).toBe('running');
  });

  it('goose saying "failed" stops the loop with its error', async () => {
    const h = harness({ status: () => refused });
    h.monitor.reportFromRenderer({
      state: 'failed',
      baseUrl: BASE,
      lastError: 'port never opened',
    });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    expect(h.monitor.current()).toMatchObject({ mode: 'failed', failedError: 'port never opened' });
    expect(h.scheduled).toHaveLength(0);
  });

  it('an engine that DIED while running is failed with its exit — goose sends no port for it', async () => {
    const exit =
      'the engine process (pid 83454) exited: signal: 9 (SIGKILL) — not restarted automatically; Mount restarts it (the crash breaker applies). Last log lines:\nINFO: loaded';
    const h = harness({ status: () => refused });
    h.monitor.reportFromRenderer({ state: 'failed', modelId: 'org/m', lastError: exit });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    expect(h.monitor.current()).toMatchObject({ mode: 'failed', failedError: exit });
    expect(h.scheduled).toHaveLength(0);
  });

  it('a timeout on a running engine holds the last read and says why it is stale', async () => {
    let result: MlxLiveStatusResult = answered(GENERATING_STATUS);
    const h = harness({ status: () => result });
    await h.monitor.tick();
    result = { ok: false, url: BASE, error: 'timeout', detail: 'no answer within 1500 ms' };
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s.mode).toBe('running');
    expect(s.stats?.requests).toHaveLength(3);
    expect(s.statusDetail).toBe('timeout: no answer within 1500 ms');
  });

  it('something on the port that is not Rapid-MLX is UNKNOWN, never running', async () => {
    const h = harness({ status: () => answered({ detail: 'Not Found' }) });
    await h.monitor.tick();
    expect(h.monitor.current().mode).toBe('unknown');
    expect(h.scheduled).toHaveLength(0);
  });

  it('with requests in flight it reads goose’s list and counts what no door explains', async () => {
    const h = harness({
      status: () => answered(GENERATING_STATUS),
      serving: () => ({
        ok: true,
        rows: [
          {
            id: 1,
            via: 'swarmRouter',
            sessionId: 's1',
            provider: 'omlx',
            model: 'm',
            nodeId: 'mihai-mlx',
            startedAt: '2026-09-23T20:00:00Z',
            sessionName: 'Memory · verify',
            sessionType: 'user',
            sessionError: null,
          },
        ],
      }),
    });
    await h.monitor.tick();
    const serving = h.monitor.current().serving!;
    expect(serving.clients.map((c) => c.kind)).toEqual(['chat']);
    expect(serving.unattributed).toBe(2);
    expect(serving.swarmRuns).toEqual(['bench-r9']);
  });

  it('Q-238: rows LEAVING the batch answer nobody — never counted as work goose cannot name', async () => {
    // E2E #3m on the split since Q-231: goose dropped its three fact checks when the turn started,
    // so its list holds only the turn's lease; rank 0 still lists the three as `leaving`.
    const h = harness({
      status: () => answered(SPLIT_TURN_BEHIND_LEAVING_3M),
      serving: () => ({
        ok: true,
        rows: [
          {
            id: 7,
            via: 'swarmRouter',
            sessionId: '20260927_5',
            provider: 'omlx',
            model: 'm',
            nodeId: 'mihai-mlx',
            startedAt: '2026-09-27T20:16:20Z',
            sessionName: 'Jira Migration Kickoff Notes',
            sessionType: 'user',
            sessionError: null,
          },
        ],
      }),
    });
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s.stats?.requests).toHaveLength(4);
    // Before: 4 engine rows − 1 lease = 3 "unattributed" — the chip said "Shared with other work".
    expect(s.serving).toMatchObject({ unattributed: 0 });
    expect(s.serving?.clients.map((c) => c.kind)).toEqual(['chat']);
  });

  it('goose’s list failing is carried as the reason, the requests all unattributed', async () => {
    const h = harness({
      status: () => answered(GENERATING_STATUS),
      serving: () => ({ ok: false, detail: 'no goose backend is running' }),
    });
    await h.monitor.tick();
    expect(h.monitor.current().serving).toMatchObject({
      clients: [],
      unattributed: 3,
      error: 'no goose backend is running',
    });
  });

  it('Q-129: the runs are goose’s kept runs for this way — a relaunched monitor reads them at its first read', async () => {
    const first = harness({ status: () => answered(GENERATING_STATUS) });
    await first.monitor.tick();
    expect(writingOf(first.monitor.current().measured)).toBe(21.5);
    // The app relaunches: a new monitor, nothing in memory — goose's store still holds the runs.
    const relaunched = harness({ status: () => answered({ ...IDLE_STATUS, uptime_s: 12 }) });
    await relaunched.monitor.tick();
    expect(writingOf(relaunched.monitor.current().measured)).toBe(21.5);
  });

  it('reads goose’s runs again when a turn ends (and once more as goose records it), not every tick', async () => {
    let result: MlxLiveStatusResult = answered({ ...IDLE_STATUS, total_requests_processed: 5 });
    const h = harness({ status: () => result });
    await h.monitor.tick();
    await h.monitor.tick();
    await h.monitor.tick();
    expect(h.readMeasured).toHaveBeenCalledTimes(2);
    result = answered({ ...IDLE_STATUS, total_requests_processed: 6 });
    await h.monitor.tick();
    await h.monitor.tick();
    await h.monitor.tick();
    expect(h.readMeasured).toHaveBeenCalledTimes(4);
  });

  it('an engine that goes away keeps its runs said; a failed read says why, never "none"', async () => {
    let result: MlxLiveStatusResult = answered(GENERATING_STATUS);
    let answers: MeasuredRunsFetch[] = [HERE];
    const h = harness({ status: () => result, measured: () => answers });
    await h.monitor.tick();
    result = refused;
    await h.monitor.tick();
    expect(writingOf(h.monitor.current().measured)).toBe(21.5);
    answers = [{ ok: false, detail: 'goose backend returned 500: reading the store failed' }];
    result = answered({ ...IDLE_STATUS, model: 'another-model' });
    await h.monitor.tick();
    expect(h.monitor.current().measured).toEqual({
      kind: 'unread',
      detail: 'goose backend returned 500: reading the store failed',
    });
  });

  it('a backend whose chat runs another way is never read as this engine’s runs', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS), measured: () => [STUDIO] });
    await h.monitor.tick();
    expect(h.monitor.current().measured).toEqual({
      kind: 'unread',
      detail: "goose records this Mac's chat on single:link:studio, not on the engine read here",
    });
  });

  it('no port anywhere: nothing is read, the state is unknown', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS), configBaseUrl: null });
    await h.monitor.tick();
    expect(h.readStatus).not.toHaveBeenCalled();
    expect(h.monitor.current().mode).toBe('unknown');
  });

  it('wake while a read is in flight does not start a second one; stop cancels the next', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    h.monitor.wake();
    h.monitor.wake();
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.scheduled).toHaveLength(1);
    h.monitor.stop();
    expect(h.scheduled).toHaveLength(0);
  });
});

describe('MlxEngineMonitor — the distributed run is read on its own base while it owns the Mac', () => {
  it("reads rank 0's /v1/status, tags the read distributed, and reads the split way's runs", async () => {
    let dist: string | null = null;
    const bodies: Record<string, unknown> = {
      [BASE]: GENERATING_STATUS,
      'http://127.0.0.1:8091': DIST_READING_STATUS,
    };
    const h = harness({
      status: () => answered(null),
      distributedRun: () => (dist ? splitUpAt(dist) : null),
    });
    h.readStatus.mockImplementation(async (url: string) => answered(bodies[url]));
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(writingOf(h.monitor.current().measured)).toBe(21.5);

    dist = 'http://127.0.0.1:8091';
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(h.readStatus).toHaveBeenLastCalledWith('http://127.0.0.1:8091');
    expect(s.engine).toBe('distributed');
    expect(s.mode).toBe('running');
    // Rank 0's pipeline status names no model: the split's own model, goose's word (Q-417).
    expect(s.modelId).toBe(FLASH_MODEL);
    expect(s.modelDetail).toBeNull();
    expect(s.stats?.requests[0]).toMatchObject({ prefilledTokens: 2048, promptTps: 152.4 });
    // The single engine's runs are not the split's: goose's runs for the split's way.
    expect(writingOf(s.measured)).toBe(8.19);
    expect(h.scheduled.length).toBeGreaterThan(0);

    dist = null;
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(writingOf(h.monitor.current().measured)).toBe(21.5);
  });

  it('an unanswering rank 0 is UNKNOWN with the reason, and never falls back to the single port', async () => {
    const h = harness({ status: () => refused, distributedRun: () => splitUpAt(RANK0) });
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s.engine).toBe('distributed');
    expect(s.mode).toBe('unknown');
    expect(s.statusDetail).toBe('unreachable: connect ECONNREFUSED 127.0.0.1:8090');
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.readStatus).toHaveBeenCalledWith('http://127.0.0.1:8091');
  });
});

describe('MlxEngineMonitor — a split that owns the Mac is read as the split while it starts (Q-350)', () => {
  // Installed 3.0.70, 10:14:12Z, right after the install's relaunch: the restore's split was
  // starting — the Run it row read "Starting" with Stop — while this Mac's single port refused and
  // activity said "single/off unreachable: net::ERR_CONNECTION_REFUSED"; 15 s later
  // "distributed/running". Main held goose's report of the split the whole time: it read the split
  // only once it was up, and this Mac's single engine before that.
  const SAMPLE_REFUSED: MlxLiveStatusResult = {
    ok: false,
    url: BASE,
    error: 'unreachable',
    detail: 'net::ERR_CONNECTION_REFUSED',
  };
  const startingWith = (
    state: string,
    nodes: Array<{ state: string; loadPhase?: string }>
  ): MlxDistributedReport =>
    toMlxDistributedReport({
      ...FLASH_READY,
      state,
      inflight: 0,
      nodes: FLASH_READY.nodes.slice(0, nodes.length).map((n, i) => ({ ...n, ...nodes[i] })),
    });
  const RESTORING = startingWith('starting', [{ state: 'loading' }, { state: 'preflight' }]);

  it("the exact sample: distributed/mounting at the split's phase, the single port never read, then running", async () => {
    let run: MlxDistributedReport = RESTORING;
    const h = harness({ status: () => SAMPLE_REFUSED, distributedRun: () => run });
    // goose runs no single engine: its own status said stopped before the restore began.
    h.monitor.reportFromRenderer({ state: 'stopped', baseUrl: BASE });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    const s = h.monitor.current();
    expect(`${s.engine}/${s.mode}`).toBe('distributed/mounting');
    expect(s.startPhase).toBe('loading');
    expect(s.modelId).toBe(FLASH_MODEL);
    expect(s.statusDetail).toBe(
      'the split is starting (MacBook Pro loading, workhorse preflight) — rank 0 is read once goose says it is ready'
    );
    expect(h.readStatus).not.toHaveBeenCalled();
    expect(isMlxEngineSnapshot(s)).toBe(true);
    // A starting split keeps the loop reading: the next report is followed without a new wake.
    expect(h.scheduled).toHaveLength(1);

    run = splitUpAt(RANK0);
    h.readStatus.mockImplementation(async () => answered(DIST_READING_STATUS));
    h.scheduled.shift()?.();
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(2), { timeout: testClock() });
    const up = h.monitor.current();
    expect(`${up.engine}/${up.mode}`).toBe('distributed/running');
    expect(up.startPhase).toBeNull();
    expect(h.readStatus).toHaveBeenCalledWith(RANK0);
    expect(h.readStatus).not.toHaveBeenCalledWith(BASE);
  });

  it('says how far the split has come: the furthest-behind rank, or recovering', async () => {
    const phaseOf = async (report: MlxDistributedReport) => {
      const h = harness({ status: () => SAMPLE_REFUSED, distributedRun: () => report });
      await h.monitor.tick();
      expect(h.readStatus).not.toHaveBeenCalled();
      return `${h.monitor.current().mode} ${h.monitor.current().startPhase}`;
    };
    expect(await phaseOf(startingWith('preflight', []))).toBe('mounting starting');
    expect(
      await phaseOf(
        startingWith('starting', [{ state: 'loading', loadPhase: 'warming' }, { state: 'loading' }])
      )
    ).toBe('mounting loading');
    expect(
      await phaseOf(
        startingWith('starting', [{ state: 'loading', loadPhase: 'warming' }, { state: 'ready' }])
      )
    ).toBe('mounting warming');
    expect(await phaseOf({ ...startingWith('starting', []), state: 'recovering' })).toBe(
      'mounting recovering'
    );
  });

  it("a stopping split is read at its rank 0 while it still answers, never at this Mac's single port", async () => {
    const h = harness({
      status: () => answered(DIST_READING_STATUS),
      distributedRun: () => ({ ...splitUpAt(RANK0), state: 'stopping' }),
    });
    await h.monitor.tick();
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.readStatus).toHaveBeenCalledWith(RANK0);
    expect(h.monitor.current().engine).toBe('distributed');
  });

  it('a split that let go of the Mac hands the read back to the single engine', async () => {
    const h = harness({
      status: () => answered(IDLE_STATUS),
      distributedRun: () => toMlxDistributedReport(STOPPED_WITH_CONFIG),
    });
    await h.monitor.tick();
    expect(h.readStatus).toHaveBeenCalledWith(BASE);
    expect(`${h.monitor.current().engine}/${h.monitor.current().mode}`).toBe('single/running');
  });

  it('a pushed snapshot with a start phase this build does not produce is refused', async () => {
    const h = harness({ status: () => SAMPLE_REFUSED, distributedRun: () => RESTORING });
    await h.monitor.tick();
    expect(isMlxEngineSnapshot({ ...h.monitor.current(), startPhase: 'booting' })).toBe(false);
    expect(isMlxEngineSnapshot({ ...h.monitor.current(), startPhase: undefined })).toBe(false);
  });
});

describe('MlxEngineMonitor — a remote single is read through the relay while it serves chat', () => {
  const RELAY = 'http://127.0.0.1:61001/relay/cafe';

  it("reads the peer engine's /v1/status on the relay, tags it remote, and reads the peer way's runs", async () => {
    let relay: string | null = null;
    const bodies: Record<string, unknown> = { [BASE]: IDLE_STATUS, [RELAY]: GENERATING_STATUS };
    const h = harness({ status: () => answered(null), remoteRoute: () => relay });
    h.readStatus.mockImplementation(async (url: string) => answered(bodies[url]));
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');

    relay = RELAY;
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(h.readStatus).toHaveBeenLastCalledWith(RELAY);
    expect(s.engine).toBe('remote');
    expect(s.mode).toBe('running');
    expect(writingOf(s.measured)).toBe(26.96);
    expect(h.scheduled.length).toBeGreaterThan(0);

    relay = null;
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(writingOf(h.monitor.current().measured)).toBe(21.5);
  });

  it('the distributed run owns the Mac first: a stale remote base is never read over it', async () => {
    const h = harness({
      status: () => answered(IDLE_STATUS),
      distributedRun: () => splitUpAt(RANK0),
      remoteRoute: () => RELAY,
    });
    await h.monitor.tick();
    expect(h.readStatus).toHaveBeenCalledWith('http://127.0.0.1:8091');
    expect(h.monitor.current().engine).toBe('distributed');
  });

  it('a relay read that times out is LOST CONTACT: no old figures, the reason named, and the loop keeps looking (Q-47)', async () => {
    // recovery-kill-link, 11.6 s: "timeout: no answer within 1500 ms" — the mode used to hold
    // `running`, so the screen had nothing to say while main already knew.
    const timeout: MlxLiveStatusResult = {
      ok: false,
      url: RELAY,
      error: 'timeout',
      detail: 'no answer within 1500 ms',
    };
    let answer: MlxLiveStatusResult = answered(GENERATING_STATUS);
    const h = harness({ status: () => answer, remoteRoute: () => RELAY });
    await h.monitor.tick();
    expect(h.monitor.current().stats).not.toBeNull();
    answer = timeout;
    h.scheduled.length = 0;
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({
      engine: 'remote',
      mode: 'reconnecting',
      stats: null,
      statusDetail: 'timeout: no answer within 1500 ms',
    });
    expect(h.scheduled).toHaveLength(1);
    answer = answered(GENERATING_STATUS);
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'running' });
  });

  it('a relay that refuses, or answers 502, is reconnecting — never the local engine', async () => {
    const h = harness({ status: () => refused, remoteRoute: () => RELAY });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'reconnecting' });
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.readStatus).toHaveBeenCalledWith(RELAY);
    // recovery-relaunch-peer, 18.9 s: "http: engine returned 502".
    const bad: MlxLiveStatusResult = {
      ok: false,
      url: RELAY,
      error: 'http',
      detail: 'engine returned 502',
    };
    const h2 = harness({ status: () => bad, remoteRoute: () => RELAY });
    await h2.monitor.tick();
    expect(h2.monitor.current()).toMatchObject({
      engine: 'remote',
      mode: 'reconnecting',
      statusDetail: 'http: engine returned 502',
    });
  });

  it('a route MOUNTING or FAILED there is the route’s state — this Mac’s engine is never read (relaunch, 19.9 s)', async () => {
    // The recording: the route went `mounting` while the Studio relaunched, and main read THIS
    // Mac's port — "single/off unreachable: net::ERR_CONNECTION_REFUSED" — for 11 seconds.
    let route: { state: string; baseUrl: string | null } = { state: 'mounting', baseUrl: RELAY };
    const h = harness({ status: () => refused, remoteRoute: () => route });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'mounting' });
    expect(h.readStatus).not.toHaveBeenCalled();
    route = { state: 'failed', baseUrl: RELAY };
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'failed' });
    route = { state: 'reconnecting', baseUrl: RELAY };
    await h.monitor.tick();
    expect(h.readStatus).toHaveBeenCalledWith(RELAY);
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'reconnecting' });
    route = { state: 'reconnecting', baseUrl: null };
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'reconnecting' });
    expect(h.readStatus).toHaveBeenCalledTimes(1);
  });
});

describe('MlxEngineMonitor — Q-417: a remote single names the model it serves, as the single engine does', () => {
  const RELAY = 'http://127.0.0.1:61001/relay/cafe';
  const STUDIO_MODEL = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
  /** The Studio's own `/v1/status` on 2026-09-28 (Rapid-MLX's `model`, the id its /v1/models lists). */
  const STUDIO_GENERATING = { ...GENERATING_STATUS, model: STUDIO_MODEL };
  const { model: _unnamed, ...NAMES_NO_MODEL } = GENERATING_STATUS;

  it('3.0.72, 18:10Z: the relay read names the Studio’s model — the engine’s own word first', async () => {
    const h = harness({
      status: () => answered(STUDIO_GENERATING),
      remoteRoute: () => ({ state: 'ready', baseUrl: RELAY, peerName: 'Work’s Mac Studio' }),
    });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({
      engine: 'remote',
      mode: 'running',
      modelId: STUDIO_MODEL,
      modelDetail: null,
      statusDetail: null,
    });
    // What leaves the loop (the tray, every window, mlxEngineActivity) carries it too.
    expect(h.snapshots[h.snapshots.length - 1]?.modelId).toBe(STUDIO_MODEL);
  });

  it('a status that names none reads the route’s model — the single engine’s order, one derivation', async () => {
    const route = { state: 'ready', baseUrl: RELAY, modelId: STUDIO_MODEL };
    const h = harness({ status: () => answered(NAMES_NO_MODEL), remoteRoute: () => route });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ modelId: STUDIO_MODEL, modelDetail: null });
    // This Mac's own engine, the same body, goose's served id: the same derivation.
    const single = harness({ status: () => answered(NAMES_NO_MODEL) });
    single.monitor.reportFromRenderer({ state: 'running', baseUrl: BASE, servedModelId: 'org/m' });
    await single.monitor.tick();
    expect(single.monitor.current()).toMatchObject({ engine: 'single', modelId: 'org/m' });
  });

  it('neither the engine nor the route names a model: null, and why — never a guessed name', async () => {
    const h = harness({
      status: () => answered(NAMES_NO_MODEL),
      remoteRoute: () => ({ state: 'ready', baseUrl: RELAY, modelId: null }),
    });
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s).toMatchObject({ engine: 'remote', mode: 'running', modelId: null });
    expect(s.modelDetail).toBe(
      "the engine's /v1/status names no model, and goose names none for the route"
    );
    // The stats are fresh: the reason is not `statusDetail`, which the tray reads as "Stale".
    expect(s.statusDetail).toBeNull();
    expect(s.stats).not.toBeNull();
  });

  it('a route that has not answered yet carries goose’s word for its model; a lost one keeps it', async () => {
    let route = { state: 'mounting', baseUrl: RELAY, modelId: STUDIO_MODEL };
    const h = harness({ status: () => refused, remoteRoute: () => route });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ mode: 'mounting', modelId: STUDIO_MODEL });
    route = { ...route, state: 'ready' };
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ mode: 'reconnecting', modelId: STUDIO_MODEL });
  });
});

describe('MlxEngineMonitor — Q-409: a linked Mac’s relay capability never leaves the loop', () => {
  // goosed's relay as leanzero-link mints it: holding this path IS the authorization.
  const CAPABILITY = '9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08';
  const RELAY = `http://127.0.0.1:61001/relay/${CAPABILITY}`;
  const leaks = (snapshots: readonly MlxEngineSnapshot[]) =>
    snapshots.filter((s) => JSON.stringify(s).includes(CAPABILITY.slice(0, 3)));

  it('a routed base whose Mac has no name: every snapshot, in every state, carries no capability', async () => {
    // An empty name is the absence the IPC guard admits (`isMlxRemoteReport`); the loop keys the
    // Mac by it and never by the relay URL.
    let answer: MlxLiveStatusResult = answered(GENERATING_STATUS);
    const h = harness({
      status: () => answer,
      remoteRoute: () => ({ state: 'ready', baseUrl: RELAY, peerName: '' }),
    });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({ engine: 'remote', mode: 'running' });
    answer = { ok: false, url: `${RELAY}/v1/status`, error: 'http', detail: 'engine returned 502' };
    h.clock.ms = 10_000;
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({
      mode: 'reconnecting',
      contact: { lostSinceMs: 10_000 },
    });
    answer = {
      ok: false,
      url: RELAY,
      error: 'bad-base-url',
      detail: `engine base URL is not a loopback host: ${RELAY}`,
    };
    await h.monitor.tick();
    // The loop still reads the relay itself — only what it SAYS is redacted.
    expect(h.readStatus).toHaveBeenCalledWith(RELAY);
    expect(h.readStatus).toHaveBeenCalledTimes(3);
    expect(h.snapshots).toHaveLength(3);
    expect(leaks(h.snapshots)).toEqual([]);
    expect(h.monitor.current().baseUrl).toBe('http://127.0.0.1:61001/relay/…');
    expect(h.monitor.current().statusDetail).toBe(
      'bad-base-url: engine base URL is not a loopback host: http://127.0.0.1:61001/relay/…'
    );
  });

  it("a failed routed read's detail is redacted: V8's bad-json message quotes the body at the fault", async () => {
    // Measured (node 22, `Response.json()` on a body quoting its relay path):
    // `Unexpected token '/', ...", "path": /relay/9f8"... is not valid JSON` — ten characters of
    // the body after the fault, the capability's first ones among them.
    const h = harness({
      status: () => ({
        ok: false,
        url: `${RELAY}/v1/status`,
        error: 'bad-json',
        detail: `Unexpected token '/', ...", "path": /relay/${CAPABILITY.slice(0, 3)}"... is not valid JSON`,
      }),
      remoteRoute: () => ({ state: 'ready', baseUrl: RELAY, peerName: 'Work’s Mac Studio' }),
    });
    await h.monitor.tick();
    expect(h.monitor.current()).toMatchObject({
      engine: 'remote',
      mode: 'reconnecting',
      statusDetail: `bad-json: Unexpected token '/', ...", "path": /relay/…"... is not valid JSON`,
    });
    expect(leaks(h.snapshots)).toEqual([]);
  });
});

describe('MlxEngineMonitor — Q-111: how long the route waited, against the comeback it is expected to make', () => {
  const RELAY = 'http://127.0.0.1:61001/relay/cafe';
  const STUDIO = 'Work’s Mac Studio';
  const SEC = 1000;
  // The harness reads at 2 s, as main does: a relaunch's worth is 25 s, the verdict 75 s.

  it('a FRESH launch — nothing measured, no quit notice: the poll anchors it, away at 76 s, silent', async () => {
    let answer: MlxLiveStatusResult = answered(IDLE_STATUS);
    const h = harness({
      status: () => answer,
      remoteRoute: () => ({ state: 'ready', baseUrl: RELAY, peerName: STUDIO }),
    });
    await h.monitor.tick();
    expect(h.monitor.current().contact).toEqual({
      lostSinceMs: null,
      lostForMs: null,
      longestComebackMs: null,
      comebacks: 0,
      saidQuit: false,
      pollMs: 2000,
    });
    answer = refused;
    h.clock.ms = 100 * SEC;
    await h.monitor.tick();
    h.clock.ms = 175 * SEC;
    await h.monitor.tick();
    expect(routePeerGone(h.monitor.current().contact, null)).toBeNull();
    h.clock.ms = 176 * SEC;
    await h.monitor.tick();
    expect(routePeerGone(h.monitor.current().contact, null)).toEqual({
      because: 'silent',
      lostSinceMs: 100 * SEC,
      lostForMs: 76 * SEC,
    });
    // Hours later: still the same steady fact, from the same moment.
    h.clock.ms = 100 * SEC + 8 * 3600 * SEC;
    await h.monitor.tick();
    expect(routePeerGone(h.monitor.current().contact, null)).toMatchObject({
      because: 'silent',
      lostSinceMs: 100 * SEC,
    });
  });

  it('a measured 90 s mount raises the expectation; an 8 h wait never becomes a comeback', async () => {
    let route: { state: string; baseUrl: string | null; peerName: string } = {
      state: 'mounting',
      baseUrl: RELAY,
      peerName: STUDIO,
    };
    let answer: MlxLiveStatusResult = refused;
    const h = harness({ status: () => answer, remoteRoute: () => route });
    await h.monitor.tick();
    h.clock.ms = 90 * SEC;
    route = { ...route, state: 'ready' };
    answer = answered(IDLE_STATUS);
    await h.monitor.tick();
    expect(h.monitor.current().contact).toMatchObject({
      longestComebackMs: 90 * SEC,
      comebacks: 1,
    });
    answer = refused;
    h.clock.ms = 100 * SEC;
    await h.monitor.tick();
    h.clock.ms = 100 * SEC + 200 * SEC;
    await h.monitor.tick();
    // 200 s is past a relaunch's 75 s, but not past 3 × this route's own 90 s mount.
    expect(routePeerGone(h.monitor.current().contact, null)).toBeNull();
    h.clock.ms = 100 * SEC + 271 * SEC;
    await h.monitor.tick();
    expect(routePeerGone(h.monitor.current().contact, null)?.because).toBe('silent');
    h.clock.ms = 100 * SEC + 8 * 3600 * SEC;
    answer = answered(IDLE_STATUS);
    await h.monitor.tick();
    expect(h.monitor.current().contact).toMatchObject({
      lostSinceMs: null,
      lostForMs: null,
      longestComebackMs: 90 * SEC,
      comebacks: 1,
    });
  });

  it('the Mac’s own “quit goose” is kept for the whole wait, though the mesh overwrites the route’s reason', async () => {
    let route: { state: string; baseUrl: string; peerName: string; lastError: string | null } = {
      state: 'reconnecting',
      baseUrl: RELAY,
      peerName: STUDIO,
      lastError: `${STUDIO} does not answer over LeanZero Link right now: ${STUDIO} quit goose`,
    };
    let answer: MlxLiveStatusResult = refused;
    const h = harness({ status: () => answer, remoteRoute: () => route });
    await h.monitor.tick();
    expect(h.monitor.current().contact?.saidQuit).toBe(true);
    route = {
      ...route,
      lastError: `${STUDIO} does not answer over LeanZero Link right now: the LeanZero Link mesh cannot reach it (connect timeout)`,
    };
    h.clock.ms = 3600 * SEC;
    await h.monitor.tick();
    expect(routePeerGone(h.monitor.current().contact, null)).toEqual({ because: 'said-quit' });
    route = { ...route, state: 'ready', lastError: null };
    answer = answered(IDLE_STATUS);
    await h.monitor.tick();
    expect(h.monitor.current().contact?.saidQuit).toBe(false);
  });

  it('a route dropped mid-wait (Stop waiting) never lends that wait to the next route’s mount', async () => {
    let route: { state: string; baseUrl: string | null; peerName: string } | null = {
      state: 'ready',
      baseUrl: RELAY,
      peerName: STUDIO,
    };
    let answer: MlxLiveStatusResult = refused;
    const h = harness({ status: () => answer, remoteRoute: () => route });
    await h.monitor.tick();
    h.clock.ms = 3600 * SEC;
    route = null;
    await h.monitor.tick();
    h.clock.ms = 2 * 3600 * SEC;
    route = { state: 'mounting', baseUrl: RELAY, peerName: STUDIO };
    await h.monitor.tick();
    h.clock.ms = 2 * 3600 * SEC + 30 * SEC;
    route = { ...route, state: 'ready' };
    answer = answered(IDLE_STATUS);
    await h.monitor.tick();
    expect(h.monitor.current().contact).toMatchObject({
      longestComebackMs: 30 * SEC,
      comebacks: 1,
    });
  });

  it('a route that FAILED there answered: the wait closes without a comeback', async () => {
    let route = { state: 'mounting', baseUrl: RELAY, peerName: STUDIO };
    const h = harness({ status: () => refused, remoteRoute: () => route });
    await h.monitor.tick();
    h.clock.ms = 40 * SEC;
    route = { ...route, state: 'failed' };
    await h.monitor.tick();
    expect(h.monitor.current().contact).toMatchObject({
      lostForMs: null,
      longestComebackMs: null,
      comebacks: 0,
    });
  });

  it('a read of this Mac’s own engine carries no contact; the IPC check refuses a malformed one', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    await h.monitor.tick();
    expect(h.monitor.current().contact).toBeNull();
    expect(isMlxEngineSnapshot(h.monitor.current())).toBe(true);
    expect(isMlxEngineSnapshot({ ...h.monitor.current(), contact: { lostForMs: 'x' } })).toBe(
      false
    );
  });
});

describe('mlxEngineConfigFromYaml', () => {
  it('reads the port and the model goose would mount from the real block shape', () => {
    const text = [
      'active_provider: swarm',
      'mlx_engine:',
      '  model_id: Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      '  models_dir: ~/.goose/models',
      '  port: 8090',
      '  context_limit: null',
    ].join('\n');
    expect(mlxEngineConfigFromYaml(text)).toEqual({
      baseUrl: 'http://127.0.0.1:8090',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
    });
  });

  it('no block, no port, or unparseable yaml: null — never a default port of main’s own', () => {
    const none = { baseUrl: null, modelId: null };
    expect(mlxEngineConfigFromYaml('active_provider: swarm\n')).toEqual(none);
    expect(mlxEngineConfigFromYaml('mlx_engine:\n  port: "8090"\n')).toEqual(none);
    expect(mlxEngineConfigFromYaml('mlx_engine: [\n')).toEqual(none);
  });
});

describe('isMlxEngineReport', () => {
  it('accepts what mlxEngineStatus sends and refuses anything else', () => {
    expect(isMlxEngineReport({ state: 'running', baseUrl: BASE, modelId: undefined })).toBe(true);
    expect(isMlxEngineReport({ state: 'exploded' })).toBe(false);
    expect(isMlxEngineReport({ state: 'running', baseUrl: 8090 })).toBe(false);
    expect(isMlxEngineReport(null)).toBe(false);
  });
});

/**
 * Q-277: after a kill -9'd goosed the relaunched goose runs no engine, its own leftover still
 * answers on the port, and the tray read "single/running" beside a panel saying no model mounted.
 * The activity follows goose's word: stopped over its own leftover is OFF, named, and the port is not
 * read; any other listener goose does not run (another goose's live engine) is read as before.
 */
describe('MlxEngineMonitor — goose stopped over its own leftover is OFF (Q-277)', () => {
  it('the leftover answering is not goose running: OFF, named, the loop stops', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    h.monitor.reportFromRenderer({ state: 'stopped', leftoverBaseUrl: BASE });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    const s = h.monitor.current();
    expect(s).toMatchObject({ engine: 'single', mode: 'off', modelId: null, stats: null });
    expect(s.statusDetail).toBe(
      `goose runs no engine: ${BASE} is answered by this goose's own engine left from an earlier run, which nothing runs — a start stops it first`
    );
    expect(h.readStatus).not.toHaveBeenCalled();
    expect(h.scheduled).toHaveLength(0);
  });

  it('the start that stops it: goose running again reads the port again', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    h.monitor.reportFromRenderer({ state: 'stopped', leftoverBaseUrl: BASE });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    h.monitor.reportFromRenderer({ state: 'running', baseUrl: BASE, modelId: 'org/m' });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(2), { timeout: testClock() });
    expect(h.monitor.current().mode).toBe('running');
  });

  it('stopped with no leftover of its own (another goose runs what answers): read as before', async () => {
    const h = harness({ status: () => answered(IDLE_STATUS) });
    h.monitor.reportFromRenderer({ state: 'stopped' });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1), { timeout: testClock() });
    expect(h.monitor.current().mode).toBe('running');
  });

  it('a leftover report is a report; a non-string address is not', () => {
    expect(isMlxEngineReport({ state: 'stopped', leftoverBaseUrl: BASE })).toBe(true);
    expect(isMlxEngineReport({ state: 'stopped', leftoverBaseUrl: 8090 })).toBe(false);
  });
});

describe('leftoverPortOf — what the renderer tells main', () => {
  const base = { restartRequired: false, availableMemoryGb: 40, totalMemoryGb: 128 };
  it('only goose stopped over holders that are all its own leftover', () => {
    const step = (kind: string) => ({ kind, text: '' });
    expect(
      leftoverPortOf({
        ...base,
        state: 'stopped',
        strayListenerPort: 8090,
        strayListenerStep: step('start'),
      })
    ).toBe(8090);
    for (const kind of ['quitStarter', 'restartGoose', 'otherPort', 'kill']) {
      expect(
        leftoverPortOf({
          ...base,
          state: 'stopped',
          strayListenerPort: 8090,
          strayListenerStep: step(kind),
        })
      ).toBeNull();
    }
    expect(
      leftoverPortOf({
        ...base,
        state: 'mounting',
        strayListenerPort: 8090,
        strayListenerStep: step('start'),
      })
    ).toBeNull();
    expect(leftoverPortOf({ ...base, state: 'stopped' })).toBeNull();
  });
});

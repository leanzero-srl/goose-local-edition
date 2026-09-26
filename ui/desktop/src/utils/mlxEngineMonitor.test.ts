import { bookSpreads } from '../components/leanzero-swarm/mlxLiveStats';
import { describe, expect, it, vi } from 'vitest';
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
import type { MlxServingRead } from './mlxServing';
import {
  DIST_READING_STATUS,
  GENERATING_STATUS,
  IDLE_STATUS,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';

const BASE = 'http://127.0.0.1:8090';

function harness(opts: {
  status: () => MlxLiveStatusResult;
  serving?: () => MlxServingRead;
  configBaseUrl?: string | null;
  distributedBaseUrl?: () => string | null;
  /** A published route; a bare string is a READY route on that relay. */
  remoteRoute?: () =>
    | { state: string; baseUrl: string | null; peerName?: string; lastError?: string | null }
    | string
    | null;
}) {
  const snapshots: MlxEngineSnapshot[] = [];
  const clock = { ms: 0 };
  const scheduled: Array<() => void> = [];
  const readServing = vi.fn(async () => opts.serving?.() ?? { ok: true as const, rows: [] });
  const readStatus = vi.fn(async (_baseUrl: string) => opts.status());
  const deps: MlxEngineMonitorDeps = {
    readStatus,
    readServing,
    configBaseUrl: () => (opts.configBaseUrl === undefined ? BASE : opts.configBaseUrl),
    distributedBaseUrl: () => opts.distributedBaseUrl?.() ?? null,
    remoteRoute: () => {
      const route = opts.remoteRoute?.() ?? null;
      return typeof route === 'string' ? { state: 'ready', baseUrl: route } : route;
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
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1));
    expect(h.monitor.current().mode).toBe('mounting');
    expect(h.monitor.current().modelId).toBe('org/m');
    expect(h.scheduled).toHaveLength(1);
    body = answered(IDLE_STATUS);
    h.scheduled.shift()!();
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(2));
    expect(h.monitor.current().mode).toBe('running');
  });

  it('goose saying "failed" stops the loop with its error', async () => {
    const h = harness({ status: () => refused });
    h.monitor.reportFromRenderer({
      state: 'failed',
      baseUrl: BASE,
      lastError: 'port never opened',
    });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1));
    expect(h.monitor.current()).toMatchObject({ mode: 'failed', failedError: 'port never opened' });
    expect(h.scheduled).toHaveLength(0);
  });

  it('an engine that DIED while running is failed with its exit — goose sends no port for it', async () => {
    const exit =
      'the engine process (pid 83454) exited: signal: 9 (SIGKILL) — not restarted automatically; Mount restarts it (the crash breaker applies). Last log lines:\nINFO: loaded';
    const h = harness({ status: () => refused });
    h.monitor.reportFromRenderer({ state: 'failed', modelId: 'org/m', lastError: exit });
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1));
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

  it('measured runs survive into idle and are dropped when the engine goes away', async () => {
    let result: MlxLiveStatusResult = answered(GENERATING_STATUS);
    const h = harness({ status: () => result });
    await h.monitor.tick();
    result = answered({ ...IDLE_STATUS, uptime_s: 1990 });
    await h.monitor.tick();
    expect(bookSpreads(h.monitor.current().rates).writing?.median ?? null).toBe(19.9);
    result = refused;
    await h.monitor.tick();
    expect(bookSpreads(h.monitor.current().rates).writing?.median ?? null).toBeNull();
  });

  it('Q-44: the same model back after a stop or a restart finds its runs; another model starts its own book', async () => {
    let result: MlxLiveStatusResult = answered(GENERATING_STATUS);
    const h = harness({ status: () => result });
    await h.monitor.tick();
    expect(bookSpreads(h.monitor.current().rates).writing?.median ?? null).toBe(19.9);
    // The engine goes away (a relaunch), then answers again with a younger uptime.
    result = refused;
    await h.monitor.tick();
    expect(bookSpreads(h.monitor.current().rates).writing).toBeNull();
    result = answered({ ...IDLE_STATUS, uptime_s: 12 });
    await h.monitor.tick();
    const back = h.monitor.current().rates;
    expect(bookSpreads(back).writing?.median ?? null).toBe(19.9);
    expect(back.restarted).toBe(true);
    // Another model on the same port: its own, empty book.
    result = answered({ ...IDLE_STATUS, model: 'another-model', uptime_s: 5 });
    await h.monitor.tick();
    expect(bookSpreads(h.monitor.current().rates).writing).toBeNull();
    expect(h.monitor.current().rates.restarted).toBe(false);
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
    await vi.waitFor(() => expect(h.snapshots).toHaveLength(1));
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.scheduled).toHaveLength(1);
    h.monitor.stop();
    expect(h.scheduled).toHaveLength(0);
  });
});

describe('MlxEngineMonitor — the distributed run is read on its own base while it owns the Mac', () => {
  it("reads rank 0's /v1/status, tags the read distributed, and drops the single engine's rates", async () => {
    let dist: string | null = null;
    const bodies: Record<string, unknown> = {
      [BASE]: GENERATING_STATUS,
      'http://127.0.0.1:8091': DIST_READING_STATUS,
    };
    const h = harness({
      status: () => answered(null),
      distributedBaseUrl: () => dist,
    });
    h.readStatus.mockImplementation(async (url: string) => answered(bodies[url]));
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(bookSpreads(h.monitor.current().rates).writing?.median ?? null).toBe(19.9);

    dist = 'http://127.0.0.1:8091';
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(h.readStatus).toHaveBeenLastCalledWith('http://127.0.0.1:8091');
    expect(s.engine).toBe('distributed');
    expect(s.mode).toBe('running');
    expect(s.modelId).toBeNull();
    expect(s.stats?.requests[0]).toMatchObject({ prefilledTokens: 2048, promptTps: 152.4 });
    // The single engine's last writing rate is not the split's.
    expect(bookSpreads(s.rates).writing?.median ?? null).toBeNull();
    expect(bookSpreads(s.rates).reading?.median ?? null).toBe(152.4);
    expect(h.scheduled.length).toBeGreaterThan(0);

    dist = null;
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(bookSpreads(h.monitor.current().rates).reading?.median ?? null).not.toBe(152.4);
  });

  it('an unanswering rank 0 is UNKNOWN with the reason, and never falls back to the single port', async () => {
    const h = harness({ status: () => refused, distributedBaseUrl: () => 'http://127.0.0.1:8091' });
    await h.monitor.tick();
    const s = h.monitor.current();
    expect(s.engine).toBe('distributed');
    expect(s.mode).toBe('unknown');
    expect(s.statusDetail).toBe('unreachable: connect ECONNREFUSED 127.0.0.1:8090');
    expect(h.readStatus).toHaveBeenCalledTimes(1);
    expect(h.readStatus).toHaveBeenCalledWith('http://127.0.0.1:8091');
  });
});

describe('MlxEngineMonitor — a remote single is read through the relay while it serves chat', () => {
  const RELAY = 'http://127.0.0.1:61001/relay/cafe';

  it("reads the peer engine's /v1/status on the relay, tags it remote, and keeps its own rates", async () => {
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
    expect(bookSpreads(s.rates).writing?.median ?? null).toBe(19.9);
    expect(h.scheduled.length).toBeGreaterThan(0);

    relay = null;
    await h.monitor.tick();
    expect(h.monitor.current().engine).toBe('single');
    expect(bookSpreads(h.monitor.current().rates).writing?.median ?? null).toBeNull();
  });

  it('the distributed run owns the Mac first: a stale remote base is never read over it', async () => {
    const h = harness({
      status: () => answered(IDLE_STATUS),
      distributedBaseUrl: () => 'http://127.0.0.1:8091',
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

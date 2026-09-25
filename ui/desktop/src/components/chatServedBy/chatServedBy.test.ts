import { describe, expect, it } from 'vitest';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import type { MountLookup } from '../noNodeNotice/mlxMount';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';
import { EMPTY_BOOK, parseMlxLiveStatus } from '../leanzero-swarm/mlxLiveStats';
import {
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from '../leanzero-swarm/mlxLiveStatus.fixtures';
import {
  deriveChatServedBy,
  leaveCause,
  mlxEngineServing,
  reconnectingMac,
  servedReady,
  type ChatServedInputs,
} from './chatServedBy';

/** The Studio reading one prompt while another request WAITS behind it (4.2 s so far). */
const PREFILL_WITH_WAITING = {
  ...PREFILL_STATUS,
  num_waiting: 1,
  requests: [GENERATING_STATUS.requests[0], ...PREFILL_STATUS.requests],
};

/**
 * One derivation, six moments (the frame's proof plan) — every chat surface reads the object this
 * returns, so each moment is pinned as the WHOLE object a surface would see.
 */

const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const MLX_NODE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const POOL: MountLookup = { state: 'ready', devices: [MLX_NODE], settings: SETTINGS };
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 63.9,
  totalMemoryGb: 128,
};
const RUNNING: MlxEngineStatus = {
  ...STOPPED,
  state: 'running',
  modelId: HF,
  servedModelId: ALIAS,
  contextWindow: 262144,
};
/** Round 1: the 27B served from the Studio over Link, this Mac's engine unmounted. */
const ROUTE: MlxRemoteSingleStatus = {
  state: 'ready',
  peer: 'worksmacstudio-lan-9c1e2a',
  peerHostname: 'WorksMacStudio.lan',
  peerComputerName: "Work's Mac Studio",
  baseUrl: 'http://127.0.0.1:61001/relay/cafe',
  modelId: HF,
  servedModelId: ALIAS,
  contextWindow: 262144,
};
const SPLIT: MlxDistributedStatus = {
  ...FLASH_READY,
  nodes: FLASH_READY.nodes.map((n, i) => ({
    ...n,
    name: i === 0 ? 'Mihai Macbook' : 'Work’s Mac Studio',
  })),
  modelId: HF,
  servedModelId: ALIAS,
  contextLimit: 65536,
};
const OTHER_WINDOW: MlxDistributedStatus = {
  mode: 'single',
  state: 'stopped',
  admissionOpen: true,
  nodes: [],
  events: [],
  restarts: 0,
  owner: {
    state: 'answering',
    pid: 4242,
    baseUrl: 'http://127.0.0.1:8191',
    servedModelId: ALIAS,
    modelId: HF,
    backend: 'jaccl',
    nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
  },
};

function snapshot(
  engine: MlxEngineSnapshot['engine'],
  body: unknown,
  serving: MlxEngineSnapshot['serving'] = null
): MlxEngineSnapshot {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return {
    engine,
    mode: 'running',
    modelId: HF,
    baseUrl: null,
    stats: read.stats,
    statusDetail: null,
    rates: EMPTY_BOOK,
    serving,
    failedError: null,
  };
}

const inputs = (over: Partial<ChatServedInputs>): ChatServedInputs => ({
  provider: 'swarm',
  lookup: POOL,
  single: STOPPED,
  distributed: null,
  remote: null,
  remoteReadError: null,
  main: null,
  sessionId: 's-mine',
  turnInFlight: false,
  thisMac: 'This Mac',
  engineLabel: 'LeanZero MLX',
  ...over,
});

describe('deriveChatServedBy — the six moments', () => {
  it('THIS MAC running, idle: its model, "This Mac", idle grey, its window, nobody else', () => {
    const served = deriveChatServedBy(
      inputs({ single: RUNNING, main: snapshot('single', IDLE_STATUS) })
    );
    expect(served).toEqual({
      engine: 'single',
      model: HF,
      where: ['This Mac'],
      peerNodeId: null,
      foreign: false,
      contextWindow: 262144,
      phase: 'idle',
      activity: 'idle',
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: { kind: 'ready' },
    });
    expect(servedReady(served)).toBe(true);
  });

  it('a ROUTE to the Studio, ready and writing: the Studio by its owner’s name, never "unmounted" from this Mac’s stopped engine (Q-4)', () => {
    const served = deriveChatServedBy(
      inputs({ remote: ROUTE, main: snapshot('remote', GENERATING_STATUS) })
    );
    expect(served).toEqual({
      engine: 'remote',
      model: HF,
      where: ["Work's Mac Studio"],
      peerNodeId: 'worksmacstudio-lan-9c1e2a',
      foreign: false,
      contextWindow: 262144,
      phase: 'writing',
      activity: 'generating',
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: { kind: 'remote', status: ROUTE },
    });
    expect(servedReady(served)).toBe(true);
  });

  it('a route MOUNTING there: amber, no window yet, not ready — and main’s read of another engine is not borrowed', () => {
    const mounting = { ...ROUTE, state: 'mounting', contextWindow: null };
    const served = deriveChatServedBy(
      inputs({ remote: mounting, main: snapshot('single', GENERATING_STATUS) })
    );
    expect(served).toEqual({
      engine: 'remote',
      model: HF,
      where: ["Work's Mac Studio"],
      peerNodeId: 'worksmacstudio-lan-9c1e2a',
      foreign: false,
      contextWindow: null,
      phase: 'loading',
      activity: null,
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      // While it loads there, this Mac's own Mount stays one click away (Q-57).
      readiness: { kind: 'remote', status: mounting, instead: { kind: 'switch', mount: HF } },
    });
    expect(servedReady(served)).toBe(false);
  });

  it('the SPLIT up (this window’s run): both Macs by name, its limit, ready', () => {
    const served = deriveChatServedBy(inputs({ distributed: SPLIT }));
    expect(served).toEqual({
      engine: 'split',
      model: HF,
      where: ['Mihai Macbook', 'Work’s Mac Studio'],
      peerNodeId: null,
      foreign: false,
      contextWindow: 65536,
      contextFromFreeMemory: false,
      phase: 'idle',
      activity: null,
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: { kind: 'ready' },
    });
  });

  it('a split owned by ANOTHER WINDOW: named as its run, answering is all this window can say', () => {
    const served = deriveChatServedBy(inputs({ distributed: OTHER_WINDOW }));
    expect(served).toEqual({
      engine: 'split',
      model: HF,
      where: ['Mihai Macbook', 'Work’s Mac Studio'],
      peerNodeId: null,
      foreign: true,
      contextWindow: null,
      phase: 'idle',
      activity: null,
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: { kind: 'ready' },
    });
    const notAnswering = deriveChatServedBy(
      inputs({
        distributed: { ...OTHER_WINDOW, owner: { ...OTHER_WINDOW.owner!, state: 'notAnswering' } },
      })
    );
    expect(notAnswering.phase).toBeNull();
    expect(notAnswering.readiness.kind).toBe('distributed');
  });

  it('NOTHING runs: no engine, but the model a Mount would bring is named on this Mac, unloaded', () => {
    const served = deriveChatServedBy(inputs({}));
    expect(served).toEqual({
      engine: 'none',
      model: HF,
      where: ['This Mac'],
      peerNodeId: null,
      foreign: false,
      contextWindow: null,
      phase: 'unloaded',
      activity: null,
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: {
        kind: 'unmounted',
        nodes: ['mihai-mlx'],
        target: { kind: 'ok', modelId: HF, servedId: ALIAS },
        fact: 'down',
      },
    });
    expect(servedReady(served)).toBe(false);
  });
});

describe('deriveChatServedBy — busy with others (Q-17)', () => {
  it('round 1, 08:42: the Studio reads another client’s 39k prompt while a request waits — busy, reading, never "ready" alone', () => {
    const served = deriveChatServedBy(
      inputs({
        remote: ROUTE,
        main: snapshot('remote', PREFILL_WITH_WAITING, {
          clients: [],
          unattributed: 1,
          swarmRuns: [],
          error: null,
        }),
      })
    );
    expect(served.phase).toBe('reading');
    expect(served.busyWithOthers).toEqual({
      requests: 1,
      readingTokens: PREFILL_STATUS.requests[0].prompt_tokens,
    });
  });

  it('Q-40: another request running BESIDE ours makes nobody wait (Rapid-MLX batches) — no bar without a request WAITING', () => {
    const main = (body: unknown) =>
      snapshot('remote', body, { clients: [], unattributed: 1, swarmRuns: [], error: null });
    expect(
      deriveChatServedBy(inputs({ remote: ROUTE, main: main(PREFILL_STATUS) })).busyWithOthers
    ).toBeNull();
    // A canary the engine has held for 0.4 s is a blink, never a bar (R3's 0.3–0.5 s canaries).
    const blink = {
      ...PREFILL_WITH_WAITING,
      requests: [
        { ...GENERATING_STATUS.requests[0], elapsed_s: 0.4, prompt_tokens: 27 },
        ...PREFILL_STATUS.requests,
      ],
    };
    expect(
      deriveChatServedBy(inputs({ remote: ROUTE, main: main(blink) })).busyWithOthers
    ).toBeNull();
  });

  it('Q-39/Q-50: goose’s own background calls (the end-of-turn reviewer, a hidden session) are never "another request"', () => {
    // The reviewer is a detached task outside any session's context: its router lease has no
    // session — the recording's "reading another request's 140-token prompt" during the user's
    // own turn.
    const reviewer = {
      key: 'session:row-7',
      kind: 'session' as const,
      sessionId: null,
      sessionName: null,
      sessionType: null,
      count: 1,
    };
    const hidden = {
      key: 'session:h1',
      kind: 'session' as const,
      sessionId: 'h1',
      sessionName: null,
      sessionType: 'hidden',
      count: 1,
    };
    const mine = {
      key: 'chat:s-mine',
      kind: 'chat' as const,
      sessionId: 's-mine',
      sessionName: 'story',
      count: 1,
    };
    const busy = (clients: NonNullable<MlxEngineSnapshot['serving']>['clients']) =>
      deriveChatServedBy(
        inputs({
          remote: ROUTE,
          turnInFlight: true,
          main: snapshot('remote', PREFILL_WITH_WAITING, {
            clients,
            unattributed: 0,
            swarmRuns: [],
            error: null,
          }),
        })
      ).busyWithOthers;
    expect(busy([mine, reviewer, hidden])).toBeNull();
    // A sub-agent or a scheduled job is its own session — still someone else's request.
    const scheduled = { ...hidden, key: 'session:j1', sessionId: 'j1', sessionType: 'scheduled' };
    expect(busy([mine, reviewer, scheduled])).toEqual({ requests: 1, readingTokens: null });
  });

  it('this chat’s own turn is not "others"; another chat’s and an external client’s are', () => {
    const mine = {
      key: 'c1',
      kind: 'chat' as const,
      sessionId: 's-mine',
      sessionName: 'a',
      count: 1,
    };
    const theirs = {
      key: 'c2',
      kind: 'chat' as const,
      sessionId: 's-other',
      sessionName: 'b',
      count: 1,
    };
    const ext = { key: 'e', kind: 'external' as const, model: ALIAS, count: 2 };
    const serving = (clients: NonNullable<MlxEngineSnapshot['serving']>['clients']) =>
      deriveChatServedBy(
        inputs({
          single: RUNNING,
          main: snapshot('single', GENERATING_STATUS, {
            clients,
            unattributed: 0,
            swarmRuns: [],
            error: null,
          }),
        })
      ).busyWithOthers;
    expect(serving([mine])).toBeNull();
    // Only the reading prompt of someone else is named; this engine is writing, so none is.
    expect(serving([mine, theirs, ext])).toEqual({ requests: 3, readingTokens: null });
  });

  it('while this chat’s turn runs, an unattributed request may be ours (the omlx provider) — never called someone else’s', () => {
    const main = snapshot('single', GENERATING_STATUS, {
      clients: [],
      unattributed: 1,
      swarmRuns: [],
      error: null,
    });
    expect(
      deriveChatServedBy(inputs({ single: RUNNING, main, turnInFlight: true })).busyWithOthers
    ).toBeNull();
    expect(
      deriveChatServedBy(inputs({ single: RUNNING, main, turnInFlight: false })).busyWithOthers
    ).toEqual({ requests: 1, readingTokens: null });
  });

  it('an idle engine, or a "who" goose could not read, claims nobody', () => {
    const idle = snapshot('single', IDLE_STATUS, {
      clients: [],
      unattributed: 1,
      swarmRuns: [],
      error: null,
    });
    expect(deriveChatServedBy(inputs({ single: RUNNING, main: idle })).busyWithOthers).toBeNull();
    const unknownWho = snapshot('single', GENERATING_STATUS, {
      clients: [],
      unattributed: 0,
      swarmRuns: [],
      error: 'in-flight list unreadable',
    });
    expect(
      deriveChatServedBy(inputs({ single: RUNNING, main: unknownWho })).busyWithOthers
    ).toBeNull();
  });
});

describe('deriveChatServedBy — what it refuses to name', () => {
  it('a cloud provider: nothing, and readiness unknown', () => {
    const served = deriveChatServedBy(inputs({ provider: 'anthropic', single: RUNNING }));
    expect(served.engine).toBe('none');
    expect(served.model).toBeNull();
    expect(served.readiness).toEqual({ kind: 'unknown' });
  });

  it('a swarm pool with an LM Studio node: no one engine is named — unless the route serves', () => {
    const mixed: MountLookup = {
      ...POOL,
      devices: [MLX_NODE, { id: 'studio-lm', model_id: 'qwen', weight: 1, enabled: true }],
    };
    expect(deriveChatServedBy(inputs({ lookup: mixed, single: RUNNING })).engine).toBe('none');
    expect(deriveChatServedBy(inputs({ lookup: mixed, single: RUNNING })).model).toBeNull();
    expect(deriveChatServedBy(inputs({ lookup: mixed, remote: ROUTE })).engine).toBe('remote');
  });

  it('the omlx provider rides the same rule', () => {
    const served = deriveChatServedBy(inputs({ provider: 'omlx', remote: ROUTE }));
    expect(served.engine).toBe('remote');
    expect(served.where).toEqual(["Work's Mac Studio"]);
  });
});

describe('mlxEngineServing — the ONE order (split, route, another window’s split, this Mac)', () => {
  it('this window’s split wins over a route read at the same time (the backend refuses both)', () => {
    expect(mlxEngineServing(RUNNING, SPLIT, ROUTE, 'This Mac').engine).toBe('split');
    expect(mlxEngineServing(RUNNING, OTHER_WINDOW, ROUTE, 'This Mac').engine).toBe('remote');
    expect(mlxEngineServing(RUNNING, null, { state: 'off' }, 'This Mac').engine).toBe('single');
    expect(mlxEngineServing(STOPPED, null, null, 'This Mac').engine).toBe('none');
  });
});

describe('a failed route read is not "no route" (works-prover on cut 1)', () => {
  it('keeps the route and its Mac — never "not running" plus Mount — and says it is reconnecting', () => {
    const served = deriveChatServedBy(
      inputs({ remote: ROUTE, remoteReadError: 'ACP read failed: goosed busy' })
    );
    expect(served.engine).toBe('remote');
    expect(served.where).toEqual(["Work's Mac Studio"]);
    expect(served.readiness.kind).toBe('reconnecting');
  });
});

describe('RECONNECTING — the Mac that serves chat stopped answering (Q-47/Q-48)', () => {
  const lookupFor = (settings: MlxEngineSettings): MountLookup => ({ ...POOL, settings });

  it('the route’s own `reconnecting`: amber, named, and "Run on this Mac instead" mounts what the bar’s Mount would', () => {
    const served = deriveChatServedBy(
      inputs({ remote: { ...ROUTE, state: 'reconnecting', lastError: 'Link peer refused' } })
    );
    expect(served.engine).toBe('remote');
    expect(served.where).toEqual(["Work's Mac Studio"]);
    expect(served.phase).toBe('loading');
    expect(servedReady(served)).toBe(false);
    expect(served.readiness).toEqual({
      kind: 'reconnecting',
      status: { ...ROUTE, state: 'reconnecting', lastError: 'Link peer refused' },
      why: 'Link peer refused',
      cause: null,
      instead: { kind: 'switch', mount: HF },
    });
  });

  it('the renderer’s route read FAILING while the route is the engine is reconnecting — never a blank bar (was phase null)', () => {
    const served = deriveChatServedBy(
      inputs({ remote: ROUTE, remoteReadError: 'ACP read failed: goosed busy' })
    );
    expect(served.phase).toBe('loading');
    expect(served.readiness).toMatchObject({
      kind: 'reconnecting',
      why: 'ACP read failed: goosed busy',
    });
  });

  it('main’s relay read failing (kill-link, 11.6 s) is reconnecting even while the route still says ready', () => {
    const lost: MlxEngineSnapshot = {
      engine: 'remote',
      mode: 'reconnecting',
      modelId: null,
      baseUrl: ROUTE.baseUrl ?? null,
      stats: null,
      statusDetail: 'timeout: no answer within 1500 ms',
      rates: EMPTY_BOOK,
      serving: null,
      failedError: null,
    };
    const served = deriveChatServedBy(inputs({ remote: ROUTE, main: lost }));
    expect(served.readiness).toMatchObject({
      kind: 'reconnecting',
      why: 'timeout: no answer within 1500 ms',
    });
    expect(served.busyWithOthers).toBeNull();
    // main's read of ANOTHER engine (this Mac's single) never says the route's Mac is lost.
    const elsewhere = { ...lost, engine: 'single' as const };
    expect(deriveChatServedBy(inputs({ remote: ROUTE, main: elsewhere })).readiness.kind).toBe(
      'remote'
    );
  });

  it('reconnectingMac names the Mac for the chat’s status line — only while reconnecting', () => {
    expect(
      reconnectingMac(deriveChatServedBy(inputs({ remote: { ...ROUTE, state: 'reconnecting' } })))
    ).toBe("Work's Mac Studio");
    expect(reconnectingMac(deriveChatServedBy(inputs({ remote: ROUTE })))).toBeNull();
  });

  it('`failed` stays red: the peer answered that its engine failed', () => {
    const served = deriveChatServedBy(inputs({ remote: { ...ROUTE, state: 'failed' } }));
    expect(served.phase).toBe('failed');
    expect(served.readiness.kind).toBe('remote');
  });

  it('Run here: this Mac already serving is a route drop only; no saved model offers nothing (Open Engine)', () => {
    const reconnecting = { ...ROUTE, state: 'reconnecting' };
    const serving = deriveChatServedBy(inputs({ remote: reconnecting, single: RUNNING }));
    expect(serving.readiness).toMatchObject({ instead: { kind: 'switch', mount: null } });
    const none = deriveChatServedBy(
      inputs({
        provider: 'omlx',
        remote: reconnecting,
        lookup: lookupFor({ ...SETTINGS, modelId: undefined }),
      })
    );
    expect(none.readiness).toMatchObject({ kind: 'reconnecting', instead: { kind: 'none' } });
    const omlx = deriveChatServedBy(inputs({ provider: 'omlx', remote: reconnecting }));
    expect(omlx.readiness).toMatchObject({ instead: { kind: 'switch', mount: HF } });
  });
});

describe('THIS turn on the engine (Q-13) and the Mac that said it is leaving (Q-54)', () => {
  const mine = {
    key: 'chat:s-mine',
    kind: 'chat' as const,
    sessionId: 's-mine',
    sessionName: 'story',
    count: 1,
  };
  const reviewer = {
    key: 'session:row-7',
    kind: 'session' as const,
    sessionId: null,
    sessionName: null,
    sessionType: null,
    count: 1,
  };
  const withRates = (snap: MlxEngineSnapshot): MlxEngineSnapshot => ({
    ...snap,
    rates: {
      uptimeS: 900,
      runs: new Map([
        ['a', { decodeTps: 19.9, prefillTps: 300 }],
        ['b', { decodeTps: 20.1, prefillTps: 318 }],
        ['c', { decodeTps: null, prefillTps: 340 }],
      ]),
      restarted: false,
    },
  });

  it('the Studio reading this chat’s 32k prompt is the turn’s request, with the engine’s measured read rate', () => {
    const served = deriveChatServedBy(
      inputs({
        remote: ROUTE,
        turnInFlight: true,
        main: withRates(
          snapshot('remote', PREFILL_STATUS, {
            clients: [mine, reviewer],
            unattributed: 0,
            swarmRuns: [],
            error: null,
          })
        ),
      })
    );
    expect(served.turnRequest).toMatchObject({ phase: 'prefill', promptTokens: 32277 });
    expect(served.readTps).toBe(318);
  });

  it('with another client on the engine, which request is ours cannot be proven — nothing is claimed', () => {
    const theirs = { ...mine, key: 'chat:s-other', sessionId: 's-other' };
    const served = deriveChatServedBy(
      inputs({
        remote: ROUTE,
        turnInFlight: true,
        main: snapshot('remote', PREFILL_STATUS, {
          clients: [mine, theirs],
          unattributed: 0,
          swarmRuns: [],
          error: null,
        }),
      })
    );
    expect(served.turnRequest).toBeNull();
    // No turn in flight, no claim either.
    expect(
      deriveChatServedBy(
        inputs({
          remote: ROUTE,
          main: snapshot('remote', PREFILL_STATUS, {
            clients: [mine],
            unattributed: 0,
            swarmRuns: [],
            error: null,
          }),
        })
      ).turnRequest
    ).toBeNull();
  });

  it('the route’s reason in fdc737969’s words names the cause; any other reason names none', () => {
    const quit = deriveChatServedBy(
      inputs({
        remote: {
          ...ROUTE,
          state: 'reconnecting',
          lastError:
            "Work's Mac Studio does not answer over LeanZero Link right now: Work's Mac Studio quit goose",
        },
      })
    );
    expect(quit.readiness).toMatchObject({ kind: 'reconnecting', cause: 'quit' });
    expect(leaveCause("Work's Mac Studio is restarting goose")).toBe('restart');
    expect(leaveCause('timeout: no answer within 1500 ms')).toBeNull();
    expect(leaveCause(null)).toBeNull();
  });
});

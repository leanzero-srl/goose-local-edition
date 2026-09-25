import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import { foreignOwner, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { remoteRouteUp, type MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import type { MlxServing } from '../../utils/mlxServing';
import type { EnginePhase } from '../lz/tokens';
import { ownsTheMac } from '../leanzero-swarm/mlxDistributed';
import { routePeerName } from '../leanzero-swarm/macs';
import { activityPhase, remotePhase, runPhase, singlePhase } from '../leanzero-swarm/mlxPhase';
import {
  MLX_STATUS_POLL_MS,
  mlxActivity,
  type MlxActivity,
  type MlxLiveStats,
} from '../leanzero-swarm/mlxLiveStats';
import { MLX_PROVIDER_ID } from '../settings/models/leanzeroSelectorPolicy';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import {
  distributedFact,
  distributedServes,
  distributedSummary,
  engineFact,
  resolveMountTarget,
  type EngineFact,
  type MountLookup,
  type MountTarget,
} from '../noNodeNotice/mlxMount';

/**
 * WHERE CHAT GOES — one derivation every chat surface reads (the model chip, the readiness bar, the
 * context counter's MLX window). Five surfaces used to decide it from different reads at different
 * times and contradicted each other on one screen: a NODES strip said "unmounted" under a green
 * "Serving from Work's Mac Studio · ready", the chip said "swarm", the bar said "ready" while the
 * Studio read another client's 39k prompt (quality round 1: Q-4 Q-5 Q-8 Q-12 Q-17). Pure and
 * React-free; `useChatServedBy` gathers its reads.
 */

/**
 * Which MLX engine answers this Mac's chat:
 *  - `single`: this Mac's own engine (running, loading or failed);
 *  - `remote`: a LeanZero Link peer's engine, chat routed there (remote single);
 *  - `split`: the distributed engine (this window's run, or another window's — `foreign`);
 *  - `none`: no MLX engine serves chat — nothing is mounted, or the provider/pool is one this
 *    renderer cannot see into (a cloud provider, an LM Studio node).
 */
export type ChatEngine = 'single' | 'remote' | 'split' | 'none';

/** Requests on the serving engine that are not this chat's — a new turn waits behind them. */
export interface ChatBusy {
  requests: number;
  /** The prompt the engine is reading for one of them, when that is what it is doing. */
  readingTokens: number | null;
}

/**
 * Can the ACTIVE provider answer a message right now? Only what the renderer can actually know:
 *
 *  - `swarm`: the router (crates/goose/src/providers/swarm_router.rs) routes to ENABLED devices only
 *    (`enabled` absent = false). Zero enabled devices can never serve. When every enabled device is a
 *    LOCAL `mlx-sidecar` node, the engine status this app supervises is the whole truth: one of them
 *    served → ready, none → not ready. Any other device (LM Studio, cloud, a remote MLX host) is a
 *    node this surface cannot probe, so the answer is `unknown` — never a fake green, never a fake red.
 *  - the LeanZero MLX provider (`omlx`): the engine itself.
 *  - everything else: `unknown`.
 *
 * A status poll that has not answered, or failed, is `unknown`; so is a stray listener on the
 * engine's port (something serves there that this app's manager does not know about).
 *
 * While the DISTRIBUTED engine owns this Mac it is the local MLX node (the router probes it, a
 * single mount is refused): serving the node's id is `ready`, anything else is `distributed` —
 * its state and mode, never a Mount offer.
 *
 * While chat is routed to a LeanZero Link peer's engine (REMOTE SINGLE) that engine is the MLX node
 * (the router adds it and sets this Mac's sidecar aside; `omlx` follows the relay): `remote` —
 * or `reconnecting` while that Mac does not answer (`lostContactWith`).
 */
export type ComposerReadiness =
  | { kind: 'unknown' }
  | { kind: 'ready' }
  | { kind: 'no-nodes' }
  | { kind: 'unmounted'; nodes: string[]; target: MountTarget; fact: EngineFact }
  | { kind: 'distributed'; nodes: string[]; status: MlxDistributedStatus; wanted: string | null }
  | { kind: 'remote'; status: MlxRemoteSingleStatus }
  | {
      kind: 'reconnecting';
      status: MlxRemoteSingleStatus;
      /** The failed read's own words (the route's, the renderer's or main's); null = none given. */
      why: string | null;
      /** What "Run on this Mac instead" does — this Mac's readiness with the route set aside. */
      instead: RunHere;
    };

/**
 * Chat back on this Mac while the route's Mac does not answer: drop the route, then mount `mount`
 * (null = this Mac's engine already serves or mounts what chat needs). `none` = nothing this Mac
 * could run is known (no saved model, a mismatch, a pool this renderer cannot see into) — the bar
 * offers the Engine view instead.
 */
export type RunHere = { kind: 'switch'; mount: string | null } | { kind: 'none' };

const UNKNOWN: ComposerReadiness = { kind: 'unknown' };

function statusIsKnowable(status: MlxEngineStatus | null): status is MlxEngineStatus {
  return status != null && status.strayListenerPort == null;
}

/**
 * The route to a peer serves chat — unless this window's split owns the Mac. The two are refused
 * together by the backend (`distributedOwnsThisMac`); should both ever be read at once, the split
 * wins, as the Engine tab and the context window have always read it.
 */
export function routeServesChat(
  remote: MlxRemoteSingleStatus | null,
  distributed: MlxDistributedStatus | null
): remote is MlxRemoteSingleStatus {
  return remote != null && remoteRouteUp(remote) && !ownsTheMac(distributed);
}

/**
 * The route is published but its Mac does not answer right now — the one state the recovery
 * recordings had no word for (Q-47/Q-48: ten blank seconds, then main fell back to reading THIS
 * Mac's engine). Any of three reads says so: the route's own `reconnecting` (the peer's status op
 * refused over Link, or the relay gave no answer), the renderer's route read failing, or main's
 * read of the peer's engine through the relay failing. The words of whichever read failed, or
 * null when it gave none; null = contact is not known to be lost. `failed` is not this: it means
 * the peer answered that its engine failed.
 */
export function lostContactWith(
  remote: MlxRemoteSingleStatus,
  remoteReadError: string | null,
  main: MlxEngineSnapshot | null
): { why: string | null } | null {
  if (remote.state === 'reconnecting') return { why: remote.lastError ?? null };
  if (remoteReadError != null) return { why: remoteReadError };
  if (main?.engine === 'remote' && main.mode === 'reconnecting') {
    return { why: main.statusDetail };
  }
  return null;
}

function runHere(local: ComposerReadiness): RunHere {
  if (local.kind === 'ready') return { kind: 'switch', mount: null };
  if (local.kind !== 'unmounted' || local.target.kind !== 'ok') return { kind: 'none' };
  return { kind: 'switch', mount: local.fact === 'mounting' ? null : local.target.modelId };
}

const isLocalMlx = (d: SwarmDeviceRow) =>
  d.engine === 'mlx-sidecar' &&
  d.host == null &&
  (d.provider == null || d.provider.toLowerCase() === 'lmstudio');

export function swarmReadiness(
  lookup: MountLookup,
  status: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  remote: MlxRemoteSingleStatus | null = null
): ComposerReadiness {
  if (routeServesChat(remote, distributed)) return { kind: 'remote', status: remote };
  if (lookup.state !== 'ready') return UNKNOWN;
  const enabled = lookup.devices.filter((d) => d.enabled === true);
  if (enabled.length === 0) return { kind: 'no-nodes' };
  if (!enabled.every(isLocalMlx)) return UNKNOWN;
  if (distributed && (ownsTheMac(distributed) || foreignOwner(distributed))) {
    if (enabled.some((d) => distributedFact(distributed, d.model_id) === 'up')) {
      return { kind: 'ready' };
    }
    return {
      kind: 'distributed',
      nodes: enabled.map((d) => d.id),
      status: distributed,
      wanted: enabled[0].model_id,
    };
  }
  if (!statusIsKnowable(status)) return UNKNOWN;
  const targets = enabled.map((d) => resolveMountTarget(d.id, lookup.devices, lookup.settings));
  const facts = enabled.map((d) => engineFact(status, d.model_id));
  if (facts.includes('up')) return { kind: 'ready' };
  const target = targets.find((t) => t.kind === 'ok') ?? targets[0];
  const fact = facts.includes('mounting')
    ? 'mounting'
    : facts.includes('failed')
      ? 'failed'
      : 'down';
  return { kind: 'unmounted', nodes: enabled.map((d) => d.id), target, fact };
}

export function mlxProviderReadiness(
  settings: MlxEngineSettings | null,
  status: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  engineLabel: string,
  remote: MlxRemoteSingleStatus | null = null
): ComposerReadiness {
  if (routeServesChat(remote, distributed)) return { kind: 'remote', status: remote };
  if (distributed && (ownsTheMac(distributed) || foreignOwner(distributed))) {
    // The omlx provider follows the distributed engine's port while it owns the Mac — this
    // window's run or another's (mlx_engine.rs align_omlx_host_env) — and asks for whatever id
    // that engine lists.
    return distributedServes(distributed)
      ? { kind: 'ready' }
      : { kind: 'distributed', nodes: [engineLabel], status: distributed, wanted: null };
  }
  if (!settings || !statusIsKnowable(status)) return UNKNOWN;
  if (status.state === 'running') return { kind: 'ready' };
  const target: MountTarget = settings.modelId
    ? {
        kind: 'ok',
        modelId: settings.modelId,
        servedId: settings.servedModelName || settings.modelId,
      }
    : { kind: 'none' };
  return {
    kind: 'unmounted',
    nodes: [engineLabel],
    target,
    fact: engineFact(status, target.kind === 'ok' ? target.servedId : null),
  };
}

/** The MLX engine that answers this Mac's chat, by the one rule — no provider, no pool. */
export interface MlxEngineServing {
  engine: ChatEngine;
  /** The HF id it serves (or is loading), else the id chat requests carry. */
  model: string | null;
  /** Its Mac(s), by their one name; `thisMac` is the caller's localized "This Mac". */
  where: string[];
  peerNodeId: string | null;
  foreign: boolean;
  /** The window it reports while it answers — never a default. */
  contextWindow: number | null;
}

const NO_ENGINE: MlxEngineServing = {
  engine: 'none',
  model: null,
  where: [],
  peerNodeId: null,
  foreign: false,
  contextWindow: null,
};

/**
 * Which engine answers, and its facts: this window's split while it owns the Mac, else a route to a
 * peer while it is up, else another window's split, else this Mac's single engine while it runs,
 * loads or failed. `swarmContextLimit` reads the window from here too, so the counter and the chip
 * can never name two engines.
 */
export function mlxEngineServing(
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  remote: MlxRemoteSingleStatus | null,
  thisMac: string
): MlxEngineServing {
  if (distributed && ownsTheMac(distributed)) {
    const up = distributed.state === 'ready' || distributed.state === 'serving';
    return {
      engine: 'split',
      model: distributed.modelId ?? distributed.servedModelId ?? null,
      where: splitNames(distributed),
      peerNodeId: null,
      foreign: false,
      contextWindow: up ? (distributed.contextLimit ?? null) : null,
    };
  }
  if (routeServesChat(remote, distributed)) {
    return {
      engine: 'remote',
      model: remote.modelId ?? remote.servedModelId ?? null,
      where: [routePeerName(remote)],
      peerNodeId: remote.peer ?? null,
      foreign: false,
      contextWindow: remote.state === 'ready' ? (remote.contextWindow ?? null) : null,
    };
  }
  const foreign = foreignOwner(distributed);
  if (distributed && foreign) {
    return {
      engine: 'split',
      model: foreign.modelId ?? foreign.servedModelId ?? null,
      where: splitNames(distributed),
      peerNodeId: null,
      foreign: true,
      contextWindow: null,
    };
  }
  if (
    statusIsKnowable(single) &&
    (single.state === 'running' || single.state === 'mounting' || single.state === 'failed')
  ) {
    return {
      engine: 'single',
      model: single.modelId ?? single.servedModelId ?? null,
      where: [thisMac],
      peerNodeId: null,
      foreign: false,
      contextWindow: single.state === 'running' ? (single.contextWindow ?? null) : null,
    };
  }
  return NO_ENGINE;
}

function splitNames(distributed: MlxDistributedStatus): string[] {
  const summary = distributedSummary(distributed);
  return summary.mode === 'distributed' ? summary.nodeNames : [];
}

export interface ChatServedBy extends MlxEngineServing {
  /** The colour it is in (lz ENGINE PHASE); null = no claim (nothing read, or unreadable). */
  phase: EnginePhase | null;
  /** What it is doing, from main's live read of THAT engine; null when main has none. */
  activity: MlxActivity | null;
  busyWithOthers: ChatBusy | null;
  /** Can the active provider answer — the readiness bar's actions hang off it. */
  readiness: ComposerReadiness;
}

export interface ChatServedInputs {
  provider: string | null | undefined;
  lookup: MountLookup;
  /** This Mac's single engine (the composer's poll). */
  single: MlxEngineStatus | null;
  distributed: MlxDistributedStatus | null;
  remote: MlxRemoteSingleStatus | null;
  /** Why the last route read failed (the last status is kept); while set, the route's state is unknown. */
  remoteReadError: string | null;
  /** main's latest read of the engine that serves (activity, who it serves); null = none. */
  main: MlxEngineSnapshot | null;
  /** The chat asking — its own requests are not "others". */
  sessionId: string | null;
  /**
   * This chat has a turn in flight. A turn goose did not lease through the router (the `omlx`
   * provider) is not in goose's in-flight list, so the engine counts it "unattributed": while our
   * own turn runs, an unattributed request may be ours and is never called someone else's.
   */
  turnInFlight: boolean;
  thisMac: string;
  /** The omlx provider's node name in the readiness wording ("LeanZero MLX"). */
  engineLabel: string;
}

const SNAPSHOT_ENGINE: Record<Exclude<ChatEngine, 'none'>, MlxEngineSnapshot['engine']> = {
  single: 'single',
  remote: 'remote',
  split: 'distributed',
};

/** main's live stats — only when main's read is of THE engine that serves chat. */
function liveStatsOf(main: MlxEngineSnapshot | null, engine: ChatEngine): MlxLiveStats | null {
  if (!main || engine === 'none' || main.engine !== SNAPSHOT_ENGINE[engine]) return null;
  return main.mode === 'running' ? main.stats : null;
}

/**
 * goose's own background call on the engine — the end-of-turn reviewer, a title, a recall: a
 * router lease with no session (the reviewer is a detached task outside any session's context,
 * crates/goose/src/turn_assessment.rs) or a hidden one. Never "another request" (Q-39): the chat
 * the user is in caused it.
 */
function gooseBackground(client: MlxServing['clients'][number]): boolean {
  return (
    client.kind === 'session' &&
    (client.sessionId == null || client.sessionType == null || client.sessionType === 'hidden')
  );
}

/**
 * A request the engine holds WAITING — the only fact that makes a new turn wait: Rapid-MLX batches
 * what it runs, so a request running beside ours delays nobody's start (Q-40). One the engine has
 * held for less than a read interval is a blink the reads cannot show as more than a flicker (the
 * 0.3–0.5 s canaries); one whose age the engine does not report counts — it IS waiting.
 */
function holdsARequestWaiting(stats: MlxLiveStats): boolean {
  return stats.requests.some(
    (r) => r.status === 'waiting' && (r.elapsedS == null || r.elapsedS * 1000 >= MLX_STATUS_POLL_MS)
  );
}

function busyWith(
  main: MlxEngineSnapshot,
  stats: MlxLiveStats,
  activity: MlxActivity,
  sessionId: string | null,
  turnInFlight: boolean
): ChatBusy | null {
  if (activity === 'idle' || activity === 'not_loaded') return null;
  if (!holdsARequestWaiting(stats)) return null;
  const serving = main.serving;
  // Who the requests are is unknown: nothing is claimed about them.
  if (!serving || serving.error) return null;
  let own = 0;
  let others = 0;
  for (const client of serving.clients) {
    const mine = client.kind !== 'external' && sessionId != null && client.sessionId === sessionId;
    if (mine) own += client.count;
    else if (!gooseBackground(client)) others += client.count;
  }
  if (!(turnInFlight && own === 0)) others += serving.unattributed;
  if (others <= 0) return null;
  const reading =
    activity === 'prefill' && own === 0
      ? stats.requests
          .filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
          .sort((a, b) => (b.elapsedS ?? 0) - (a.elapsedS ?? 0))[0]
      : undefined;
  return { requests: others, readingTokens: reading?.promptTokens ?? null };
}

function phaseOf(
  serving: MlxEngineServing,
  inputs: ChatServedInputs,
  activity: MlxActivity | null
): EnginePhase | null {
  const { single, distributed, remote } = inputs;
  switch (serving.engine) {
    case 'single':
      return singlePhase(single?.state ?? null, false, activity);
    case 'remote':
      if (remote && lostContactWith(remote, inputs.remoteReadError, inputs.main)) {
        return 'loading';
      }
      return remotePhase(remote?.state ?? 'off', activity);
    case 'split': {
      if (serving.foreign) {
        // Only its owner knows the supervisor's state; answering is all this window can say.
        return foreignOwner(distributed)?.state === 'answering'
          ? activity
            ? activityPhase(activity)
            : 'idle'
          : null;
      }
      return distributed ? runPhase(distributed.state, distributed.admissionOpen, activity) : null;
    }
    case 'none':
      return statusIsKnowable(single) && single.state === 'stopped' ? 'unloaded' : null;
  }
}

/** Nothing served, nothing known: a provider this renderer cannot see into. */
const NOT_MLX: ChatServedBy = {
  ...NO_ENGINE,
  phase: null,
  activity: null,
  busyWithOthers: null,
  readiness: UNKNOWN,
};

export function deriveChatServedBy(inputs: ChatServedInputs): ChatServedBy {
  const { provider, lookup, single, distributed, remote, main, sessionId, thisMac } = inputs;
  const isSwarm = provider === 'swarm';
  const isMlx = provider === MLX_PROVIDER_ID;
  if (!isSwarm && !isMlx) return NOT_MLX;

  const readinessVia = (route: MlxRemoteSingleStatus | null) =>
    isSwarm
      ? swarmReadiness(lookup, single, distributed, route)
      : mlxProviderReadiness(
          lookup.state === 'ready' ? lookup.settings : null,
          single,
          distributed,
          inputs.engineLabel,
          route
        );
  let readiness = readinessVia(remote);
  if (readiness.kind === 'remote') {
    const lost = lostContactWith(readiness.status, inputs.remoteReadError, main);
    if (lost) {
      readiness = {
        kind: 'reconnecting',
        status: readiness.status,
        why: lost.why,
        instead: runHere(readinessVia(null)),
      };
    }
  }

  // A swarm pool with a node this renderer cannot probe (LM Studio, cloud) — or not read yet —
  // sends a turn to whichever node is idle: no one engine can be named, unless a route or the
  // split is up (the router serves the MLX node there whatever else the pool holds).
  const poolIsLocalMlx =
    lookup.state === 'ready' &&
    lookup.devices.some((d) => d.enabled === true) &&
    lookup.devices.filter((d) => d.enabled === true).every(isLocalMlx);
  let serving = mlxEngineServing(single, distributed, remote, thisMac);
  if (isSwarm && serving.engine !== 'remote' && !poolIsLocalMlx) serving = NO_ENGINE;

  if (serving.engine === 'none') {
    // Nothing runs: name the model a Mount would bring, on this Mac, so the chip never falls back
    // to a provider id.
    const target = readiness.kind === 'unmounted' ? readiness.target : null;
    const model =
      target?.kind === 'ok'
        ? target.modelId
        : isMlx && lookup.state === 'ready'
          ? (lookup.settings.modelId ?? null)
          : null;
    const named = model != null && (isMlx || poolIsLocalMlx);
    return {
      ...NO_ENGINE,
      model: named ? model : null,
      where: named ? [thisMac] : [],
      phase: named ? phaseOf(NO_ENGINE, inputs, null) : null,
      activity: null,
      busyWithOthers: null,
      readiness,
    };
  }

  const stats = liveStatsOf(main, serving.engine);
  const activity = stats ? mlxActivity(stats) : null;
  return {
    ...serving,
    phase: phaseOf(serving, inputs, activity),
    activity,
    busyWithOthers:
      stats && activity && main
        ? busyWith(main, stats, activity, sessionId, inputs.turnInFlight)
        : null,
    readiness,
  };
}

/**
 * The readiness bar speaks only when something needs the user: loading, failed, no model, no
 * nodes, a split not answering, or the engine busy with another client's request. A ready engine
 * is the chip's to name — a permanent green "ready" bar is noise (Q-8).
 */
export function servedReady(served: ChatServedBy): boolean {
  const { readiness } = served;
  return (
    readiness.kind === 'ready' ||
    (readiness.kind === 'remote' && readiness.status.state === 'ready')
  );
}

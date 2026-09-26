import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import {
  mlxEngineMount,
  mlxEngineSettingsRead,
  type MlxEngineSettings,
  type MlxEngineStatus,
} from '../../acp/mlx-engine';
import {
  foreignOwner,
  latestMlxDistributedStatus,
  subscribeMlxDistributedStatus,
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import {
  backendName,
  modeSummary,
  ownsTheMac,
  type MlxModeSummary,
} from '../leanzero-swarm/mlxDistributed';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import { mlxServingIntent, type MlxServingIntent } from '../../acp/mlx-serving-intent';
import { mlxModelShort } from '../leanzero-swarm/nodes';
import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { distributedStateWord } from '../leanzero-swarm/mlxModeLabel';
import { errorMessage } from '../../utils/conversionUtils';

/**
 * The device → model → mount resolution shared by the two surfaces that offer "Mount": the
 * transcript's no-node notice (after a refused turn) and the composer's readiness strip (before
 * one). One copy, so the two can never disagree about which model a node needs.
 */

/**
 * What a "Mount" click would mount for a swarm device: the engine's persisted HF model, but only
 * when that model is served under the alias the device names (mlx_engine.served_model_name, else the
 * HF id itself). Anything else is stated, never guessed — mounting a different model would leave the
 * node refusing the next turn for the same reason.
 */
export type MountTarget =
  | { kind: 'ok'; modelId: string; servedId: string }
  | { kind: 'mismatch'; served: string; wanted: string }
  | { kind: 'none' };

export function resolveMountTarget(
  nodeId: string,
  devices: SwarmDeviceRow[],
  settings: MlxEngineSettings
): MountTarget {
  const device = devices.find((d) => d.id === nodeId);
  if (!device || device.engine !== 'mlx-sidecar' || device.host != null || !settings.modelId) {
    return { kind: 'none' };
  }
  const served = settings.servedModelName || settings.modelId;
  if (served !== device.model_id) {
    return { kind: 'mismatch', served, wanted: device.model_id };
  }
  return { kind: 'ok', modelId: settings.modelId, servedId: served };
}

/**
 * One model on disk, three names (Q-128): its HF id, the alias `mlx_engine.servedModelName` gives the
 * ONE model `mlx_engine.modelId` names, and the Add-node alias `<label>-<tag>-mlx` beside the device
 * id `<label>-mlx`. The mirror of goose-sidecar `model_identity::node_names_model` — both are pinned
 * to model_identity.fixture.json, so the router and this window cannot disagree.
 */
export function nodeNamesModel(
  nodeId: string,
  nodeModelId: string,
  served: string,
  repo: string
): boolean {
  if (nodeModelId === served || nodeModelId === repo) return true;
  const label = nodeId.endsWith('-mlx') ? nodeId.slice(0, -'-mlx'.length) : '';
  return label !== '' && nodeModelId === `${label}-${mlxModelShort(repo)}-mlx`;
}

/** The HF directory an engine serving `served` loaded — `model_identity::served_repo`. */
export function servedRepo(settings: MlxEngineSettings, served: string): string {
  return settings.servedModelName === served && settings.modelId ? settings.modelId : served;
}

/** A swarm device the router probes as this Mac's MLX engine (`nodes_from_config`). */
function isSidecarNode(d: SwarmDeviceRow): boolean {
  return (
    d.enabled === true &&
    d.engine === 'mlx-sidecar' &&
    (d.provider == null || d.provider.toLowerCase() === 'lmstudio')
  );
}

export interface ChatNode {
  nodeId: string;
  /** The node is set to another model and follows the owner's own start (its model_id is kept). */
  follows: boolean;
}

/**
 * Which pool node takes chat while this Mac's MLX engine serves `served` — the router's
 * `mlx_node_verdict` + `one_node_per_engine`: the first node that names the model in any form;
 * else, only when the owner STARTED that model through goose (the serving intent), the heaviest
 * node (the first on a tie) follows it; else none.
 */
export function chatNodeOf(
  devices: SwarmDeviceRow[],
  settings: MlxEngineSettings,
  intent: MlxServingIntent | null,
  served: string
): ChatNode | null {
  const repo = servedRepo(settings, served);
  const nodes = devices.filter(isSidecarNode);
  const named = nodes.find((d) => nodeNamesModel(d.id, d.model_id, served, repo));
  if (named) return { nodeId: named.id, follows: false };
  const started =
    intent != null &&
    (intent.kind === 'single' || intent.kind === 'split') &&
    (intent.modelId === repo || intent.modelId === served);
  if (!started) return null;
  const heaviest = nodes.reduce<SwarmDeviceRow | null>(
    (best, d) => (best == null || d.weight > best.weight ? d : best),
    null
  );
  return heaviest ? { nodeId: heaviest.id, follows: true } : null;
}

/**
 * The id a device is served under while `served` answers: `served` when the pool gives this node
 * the engine (it names the model or follows the owner's start), else the node's own model_id —
 * what `engineFact` / `distributedFact` compare against. `null` served = nothing answers.
 */
export function nodeServedId(
  lookup: Extract<MountLookup, { state: 'ready' }>,
  device: SwarmDeviceRow,
  served: string | null
): string {
  if (served == null) return device.model_id;
  const chat = chatNodeOf(lookup.devices, lookup.settings, lookup.intent, served);
  return chat?.nodeId === device.id ? served : device.model_id;
}

/** An HF repo id reads by its last path segment ("org/Qwen3.8-27B-mlx" → "Qwen3.8-27B-mlx"). */
export function shortModelName(modelId: string): string {
  const segment = modelId.split('/').filter(Boolean).pop();
  return segment ?? modelId;
}

export type EngineFact = 'up' | 'mounting' | 'failed' | 'down';

/**
 * While the distributed engine owns this Mac it IS the local MLX node: the swarm router probes ITS
 * port (swarm_router.rs `probe_mlx`) and a single mount is refused, so the single engine's status
 * says nothing and Mount is never offered. `null` = it does not own the Mac. The id compared is the
 * one the ranks serve (`servedModelId`, derived by the backend exactly like the single engine's
 * served name) — never re-derived here.
 */
export function distributedFact(
  distributed: MlxDistributedStatus | null,
  servedId: string | null
): EngineFact | null {
  if (!distributed) return null;
  const foreign = foreignOwner(distributed);
  if (foreign) {
    // Another window's goosed supervises it: its own /v1/models answer is the whole fact here.
    return foreign.state === 'answering' && servedId != null && foreign.servedModelId === servedId
      ? 'up'
      : 'down';
  }
  // Serving a rank of ANOTHER Mac's engine: nothing here answers requests, and the single engine
  // is refused (the backend's `hostingRank`) — so Mount is never offered.
  if (!ownsTheMac(distributed)) return distributed.hosting ? 'down' : null;
  const { state } = distributed;
  if (state === 'preflight' || state === 'starting') return 'mounting';
  if (
    (state === 'ready' || state === 'serving') &&
    servedId != null &&
    distributed.servedModelId === servedId
  ) {
    return 'up';
  }
  return 'down';
}

/** The distributed engine that owns this Mac, whichever goosed supervises it, for the mode line. */
export function distributedSummary(distributed: MlxDistributedStatus): MlxModeSummary {
  const foreign = foreignOwner(distributed);
  if (foreign) {
    return {
      mode: 'distributed',
      nodeNames: foreign.nodeNames ?? [],
      backend: backendName(foreign.backend),
    };
  }
  return modeSummary(distributed);
}

const i18n = defineMessages({
  foreignAnswering: {
    id: 'mlxMount.foreignAnswering',
    defaultMessage: 'Ready · owned by another window',
  },
  foreignNotAnswering: {
    id: 'mlxMount.foreignNotAnswering',
    defaultMessage: 'Not answering · owned by another window',
  },
});

/** Its state in words: this window's run in the backend's vocabulary, another window's as the
 *  answer its engine gave (only its owner knows the supervisor's state). */
export function distributedStateLabel(intl: IntlShape, distributed: MlxDistributedStatus): string {
  const foreign = foreignOwner(distributed);
  if (foreign) {
    return intl.formatMessage(
      foreign.state === 'answering' ? i18n.foreignAnswering : i18n.foreignNotAnswering
    );
  }
  if (!ownsTheMac(distributed) && distributed.hosting) {
    return distributedStateWord(intl, distributed.hosting.state);
  }
  return distributedStateWord(intl, distributed.state);
}

/** It answers requests: this window's run is ready/serving, another window's answers /v1/models. */
export function distributedServes(distributed: MlxDistributedStatus): boolean {
  const foreign = foreignOwner(distributed);
  if (foreign) return foreign.state === 'answering';
  return distributed.state === 'ready' || distributed.state === 'serving';
}

/** Why it does not answer: this window's last error, or the probe of another window's engine. */
export function distributedProblem(distributed: MlxDistributedStatus): string | null {
  const foreign = foreignOwner(distributed);
  if (foreign) return foreign.detail ?? null;
  return distributed.lastError ?? null;
}

/** The id the owning engine serves — this window's run, or the one another window published. */
export function distributedServedId(distributed: MlxDistributedStatus): string | null {
  return foreignOwner(distributed)?.servedModelId ?? distributed.servedModelId ?? null;
}

/** The distributed status the rest of the window last read — no poll of its own. */
export function useLatestMlxDistributedStatus(): MlxDistributedStatus | null {
  return useSyncExternalStore(subscribeMlxDistributedStatus, latestMlxDistributedStatus);
}

/** The id this Mac's single engine answers under while it runs; null otherwise. */
export function singleServedId(status: MlxEngineStatus | null): string | null {
  return status?.state === 'running' ? (status.servedModelId ?? status.modelId ?? null) : null;
}

/** The live SINGLE engine against the id a node needs served. No status is `down` — the caller
 *  decides whether "no status" is knowable (a poll that has not answered yet is not a fact). Callers
 *  ask `distributedFact` first: while the distributed engine owns the Mac this status is moot. */
export function engineFact(status: MlxEngineStatus | null, servedId: string | null): EngineFact {
  if (!status) return 'down';
  if (status.state === 'mounting') return 'mounting';
  if (status.state === 'failed') return 'failed';
  const serving = status.servedModelId ?? status.modelId;
  if (status.state === 'running' && servedId != null && serving === servedId) return 'up';
  return 'down';
}

export type MountLookup =
  | { state: 'loading' }
  | { state: 'failed'; error: string }
  | {
      state: 'ready';
      devices: SwarmDeviceRow[];
      settings: MlxEngineSettings;
      /** What the owner last started (the serving intent); null = nothing, or an unreadable record. */
      intent: MlxServingIntent | null;
    };

/**
 * Reads the swarm devices, the MLX engine's saved settings and the serving intent while `armed`. `refreshKey` re-reads
 * (the composer bumps it when the window regains focus — the operator may have changed a device in
 * Providers meanwhile). A failed read is a state the caller renders, never an empty pool.
 */
export function useMountLookup(
  armed: boolean,
  readSwarm: () => Promise<SwarmConfig | null>,
  refreshKey: unknown = null
): MountLookup {
  const [lookup, setLookup] = useState<MountLookup>({ state: 'loading' });
  const readRef = useRef(readSwarm);
  readRef.current = readSwarm;

  useEffect(() => {
    if (!armed) return undefined;
    let alive = true;
    void (async () => {
      try {
        const [raw, settings, started] = await Promise.all([
          readRef.current(),
          mlxEngineSettingsRead(),
          mlxServingIntent(),
        ]);
        const devices = Array.isArray(raw?.devices) ? raw.devices : [];
        if (alive) setLookup({ state: 'ready', devices, settings, intent: started.intent });
      } catch (e) {
        if (alive) setLookup({ state: 'failed', error: errorMessage(e, String(e)) });
      }
    })();
    return () => {
      alive = false;
    };
  }, [armed, refreshKey]);

  return lookup;
}

/**
 * The Mount action with its in-between state. The mount call returns before the engine flips to
 * mounting, so the node whose mount was requested is held until a status poll that landed AFTER
 * the request answers — the stale "stopped" from the previous poll must not bring Mount back.
 */
export function useMlxMount(status: MlxEngineStatus | null) {
  const [mountErrors, setMountErrors] = useState<Record<string, string>>({});
  const [requesting, setRequesting] = useState<{ nodeId: string; seen: unknown } | null>(null);
  const statusRef = useRef(status);
  statusRef.current = status;

  useEffect(() => {
    if (requesting && requesting.seen !== undefined && status !== requesting.seen) {
      setRequesting(null);
    }
  }, [status, requesting]);

  const mount = useCallback(async (nodeId: string, modelId: string) => {
    setRequesting({ nodeId, seen: undefined });
    setMountErrors((prev) => {
      const { [nodeId]: _dropped, ...rest } = prev;
      return rest;
    });
    try {
      await mlxEngineMount(modelId);
      setRequesting({ nodeId, seen: statusRef.current });
    } catch (e) {
      setMountErrors((prev) => ({ ...prev, [nodeId]: errorMessage(e, String(e)) }));
      setRequesting(null);
    }
  }, []);

  return { requestingNodeId: requesting?.nodeId ?? null, mountErrors, mount };
}

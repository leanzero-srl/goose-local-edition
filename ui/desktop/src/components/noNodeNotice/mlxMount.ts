import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import {
  mlxEngineMount,
  mlxEngineSettingsRead,
  type MlxEngineSettings,
  type MlxEngineStatus,
} from '../../acp/mlx-engine';
import {
  latestMlxDistributedStatus,
  subscribeMlxDistributedStatus,
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import { ownsTheMac } from '../leanzero-swarm/mlxDistributed';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
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
  if (!distributed || !ownsTheMac(distributed)) return null;
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

/** The distributed status the rest of the window last read — no poll of its own. */
export function useLatestMlxDistributedStatus(): MlxDistributedStatus | null {
  return useSyncExternalStore(subscribeMlxDistributedStatus, latestMlxDistributedStatus);
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
  | { state: 'ready'; devices: SwarmDeviceRow[]; settings: MlxEngineSettings };

/**
 * Reads the swarm devices and the MLX engine's saved settings while `armed`. `refreshKey` re-reads
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
        const [raw, settings] = await Promise.all([readRef.current(), mlxEngineSettingsRead()]);
        const devices = Array.isArray(raw?.devices) ? raw.devices : [];
        if (alive) setLookup({ state: 'ready', devices, settings });
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

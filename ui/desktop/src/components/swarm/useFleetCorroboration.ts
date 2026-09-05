import { useEffect, useMemo, useState } from 'react';
import { acpReadConfig } from '../../acp/config';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import { deviceFromModelId, useFleetStatus } from './useFleet';

/**
 * THE TRUTH FEED for deriveFleet's dead-lane demotion — separate from DISPLAY on purpose.
 *
 * U-H2 (branch review, 2026-09-01): the panel fed deriveFleet from `useFleetStatus(1500, lmStudioVisible)`,
 * and `showLmStudioFleet` defaults OFF, so on a default install the feed was `{}`: `reportedNodes` empty →
 * `fleetReporting` false → a lane the engine opened and never closed rendered a dead node as "working"
 * for as long as the panel stayed open (the 2026-08-28 lie, back by configuration). A display toggle must
 * never switch off a truth. And the feed shelled `lms ps` only, so a LeanZero MLX sidecar device — which
 * never appears in `lms ps` — was structurally a cloud node to deriveFleet: never reported, never demoted.
 *
 * Two feeds, one shape, both from polls that ALREADY exist (deriveFleet's own rule: no timer is added for
 * this — a clock is what was deleted everywhere else):
 *   - LM Studio: `useFleetStatus` (`lms ps --json` through main), ALWAYS enabled here. Every node it lists
 *     is REPORTED; generating/processingPrompt is BUSY.
 *   - MLX sidecar: `useMlxEngineStatusPoll` (the same status the engine surfaces already poll), armed only
 *     when the swarm config declares a LOCAL `mlx-sidecar` device — no chatter otherwise. `running` with a
 *     clean probe means the engine REPLIED, so the device is REPORTED. BUSY is the engine's OWN count
 *     (Q1, 2026-09-02): `activeRequests` is Rapid-MLX's `/v1/status` num_running + num_waiting, read by the
 *     sidecar on that same probe; `> 0` puts the device in busyNodes. An ABSENT count (an older agent, or a
 *     refused `/v1/status` probe named in `activeRequestsError`) is "the engine did not say": the device
 *     stays reported and never busy, so its lanes keep the pre-Q1 exposure — demotion on the digest window
 *     alone (15 min for an open call) — rather than failing closed. A remote sidecar (`host` set) is not
 *     corroborated by the local engine and stays out — treated as cloud, never demoted.
 *
 * Names are the engine's CANONICAL node names (`deviceFromModelId` joined on the run's `poolNodes`,
 * the `{id | model_id → node}` map from run_started/pool_resolved since 748084b97) — what deriveFleet's
 * `lmsName` compares against and what the FLEET rows are keyed by. Without a map (no run open yet, a
 * log that predates `node`) the prefix before the first dash, which reads the sidecar on the LM Studio
 * host and the LM Studio node as ONE name.
 */
export interface FleetCorroboration {
  /** lms ps state by short node name — for surfaces that DISPLAY the LM Studio dot (gate those on the
   *  showLmStudioFleet setting); the truth arrays below are derived from it and never gated. */
  nodeStatus: Record<string, string>;
  /** Every node some feed replied about this poll, idle ones included. */
  reportedNodes: string[];
  /** Nodes LM Studio reports generating or prompt-processing, plus the local sidecar devices whose
   *  engine reports `activeRequests > 0`. */
  busyNodes: string[];
  /** Local sidecar devices whose engine answered `running` — in reportedNodes; busy only by its count. */
  mlxNodes: string[];
  /** Sidecar nodes whose engine is RUNNING but did not say whether anything is in flight — no
   *  `activeRequests` on the status (an older agent, or a refused `/v1/status` probe). The node is
   *  neither idle nor busy: it is UNKNOWN, and the fleet zone must say so rather than read idle. */
  busyUnknownNodes: string[];
  /** The engine's own reason for the unknown, verbatim (`activeRequestsError`); undefined when the
   *  status simply carried no count. */
  busyUnknownReason?: string;
}

const BUSY_STATES: ReadonlySet<string> = new Set(['generating', 'processingPrompt']);

function localSidecarRows(cfg: SwarmConfig | null): SwarmDeviceRow[] {
  const rows: SwarmDeviceRow[] = Array.isArray(cfg?.devices) ? cfg.devices : [];
  return rows.filter((d) => d.engine === 'mlx-sidecar' && d.enabled !== false && !d.host);
}

/**
 * Whether `lms ps` has anything to report on: true unless the config declares devices and EVERY
 * enabled one is an mlx-sidecar. Measured 2026-09-05 on this machine (one sidecar device, zero LM
 * Studio devices): the feed spawned `lms ps` every 1.5 s for a fleet that had no LM Studio node. An
 * absent or empty device list is the legacy LM Studio discovery pool and keeps the probe.
 */
export function lmStudioFeedWanted(cfg: SwarmConfig | null): boolean {
  const rows: SwarmDeviceRow[] = Array.isArray(cfg?.devices) ? cfg.devices : [];
  const enabled = rows.filter((d) => d.enabled !== false);
  return enabled.length === 0 || enabled.some((d) => d.engine !== 'mlx-sidecar');
}

/** The configured local sidecar devices under the run's canonical node names — `workhorse-mlx` when
 *  the pool map names the device, the model-id prefix (`workhorse`, colliding with LM Studio) without. */
export function localSidecarNames(
  rows: SwarmDeviceRow[],
  poolNodes?: Record<string, string>
): string[] {
  const names = rows.map((d) => deviceFromModelId(d.model_id || d.id, poolNodes)).filter(Boolean);
  return Array.from(new Set(names)).sort();
}

export function useFleetCorroboration(
  pollMs = 1500,
  poolNodes?: Record<string, string>
): FleetCorroboration {
  // The configured local sidecar devices, read once per mount: adding a device mid-run is a config
  // edit, and the next mount sees it. Unreadable config means no sidecar feed — no fabricated device —
  // and keeps the LM Studio probe (an unreadable config cannot prove the fleet has no LM Studio node).
  const [sidecarRows, setSidecarRows] = useState<SwarmDeviceRow[]>([]);
  // `null` until the config has answered: the first `lms ps` waits for the one read that can say
  // whether an LM Studio node exists at all, so an MLX-only pool never spawns it even once.
  const [lmsWanted, setLmsWanted] = useState<boolean | null>(null);
  useEffect(() => {
    let alive = true;
    void acpReadConfig('swarm', false)
      .then((raw) => {
        if (!alive) return;
        const cfg = (raw as SwarmConfig | null) ?? null;
        setSidecarRows(localSidecarRows(cfg));
        setLmsWanted(lmStudioFeedWanted(cfg));
      })
      .catch(() => {
        if (!alive) return;
        setSidecarRows([]);
        setLmsWanted(true);
      });
    return () => {
      alive = false;
    };
  }, []);
  const nodeStatus = useFleetStatus(pollMs, lmsWanted === true, poolNodes);
  // The names are re-joined whenever the pool map arrives or changes (the first fold lands after the
  // config read), but the fold hands a fresh map object every tick, so the ARRAY identity is pinned to
  // its content: the memo below and the poll's `enabled` flag only move when a name actually does.
  const sidecarNamesKey = localSidecarNames(sidecarRows, poolNodes).join('\n');
  const sidecarNames = useMemo(
    () => (sidecarNamesKey ? sidecarNamesKey.split('\n') : []),
    [sidecarNamesKey]
  );

  const { status: mlx } = useMlxEngineStatusPoll(sidecarNames.length > 0);
  const mlxReplied = mlx?.state === 'running' && !mlx.probeError;
  // Busy needs the count ITSELF: `undefined` is the engine not saying (never treated as 0).
  const mlxCounted = mlxReplied && typeof mlx.activeRequests === 'number';
  const mlxBusy = mlxCounted && (mlx.activeRequests as number) > 0;
  const mlxBusyUnknown = mlxReplied && !mlxCounted;
  const mlxBusyUnknownReason = mlxBusyUnknown ? mlx.activeRequestsError : undefined;

  return useMemo(() => {
    const mlxNodes = mlxReplied ? sidecarNames : [];
    const reported = new Set<string>([...Object.keys(nodeStatus), ...mlxNodes]);
    const lmsBusy = Object.entries(nodeStatus)
      .filter(([, st]) => BUSY_STATES.has(st))
      .map(([n]) => n);
    const busyNodes = Array.from(new Set([...lmsBusy, ...(mlxBusy ? mlxNodes : [])]));
    return {
      nodeStatus,
      reportedNodes: Array.from(reported).sort(),
      busyNodes,
      mlxNodes,
      busyUnknownNodes: mlxBusyUnknown ? mlxNodes : [],
      busyUnknownReason: mlxBusyUnknownReason,
    };
  }, [nodeStatus, mlxReplied, mlxBusy, mlxBusyUnknown, mlxBusyUnknownReason, sidecarNames]);
}

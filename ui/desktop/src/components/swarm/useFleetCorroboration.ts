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
 *     clean probe means the engine REPLIED, so the device is REPORTED. It is never BUSY: the status DTO
 *     (MlxEngineStatus) carries no generating/idle fact, so "not busy" is unknowable for a sidecar and the
 *     demotion of a sidecar lane rests on the digest window alone (15 min for an open call) — the same
 *     exposure an LM Studio lane has during a tool call, when lms reports idle too. A remote sidecar
 *     (`host` set) is not corroborated by the local engine and stays out — treated as cloud, never demoted.
 *
 * Names are the short node names deriveFleet's `lmsName` compares against (`deviceFromModelId`).
 */
export interface FleetCorroboration {
  /** lms ps state by short node name — for surfaces that DISPLAY the LM Studio dot (gate those on the
   *  showLmStudioFleet setting); the truth arrays below are derived from it and never gated. */
  nodeStatus: Record<string, string>;
  /** Every node some feed replied about this poll, idle ones included. */
  reportedNodes: string[];
  /** Nodes LM Studio reports generating or prompt-processing. */
  busyNodes: string[];
  /** Local sidecar devices whose engine answered `running` — in reportedNodes, busy-unknowable. */
  mlxNodes: string[];
}

const BUSY_STATES: ReadonlySet<string> = new Set(['generating', 'processingPrompt']);

function localSidecarNames(cfg: SwarmConfig | null): string[] {
  const rows: SwarmDeviceRow[] = Array.isArray(cfg?.devices) ? cfg.devices : [];
  const names = rows
    .filter((d) => d.engine === 'mlx-sidecar' && d.enabled !== false && !d.host)
    .map((d) => deviceFromModelId(d.model_id || d.id))
    .filter(Boolean);
  return Array.from(new Set(names)).sort();
}

export function useFleetCorroboration(pollMs = 1500): FleetCorroboration {
  const nodeStatus = useFleetStatus(pollMs, true);

  // The configured local sidecar devices, read once per mount: adding a device mid-run is a config
  // edit, and the next mount sees it. Unreadable config means no sidecar feed — no fabricated device.
  const [sidecarNames, setSidecarNames] = useState<string[]>([]);
  useEffect(() => {
    let alive = true;
    void acpReadConfig('swarm', false)
      .then((raw) => {
        if (alive) setSidecarNames(localSidecarNames((raw as SwarmConfig | null) ?? null));
      })
      .catch(() => {
        if (alive) setSidecarNames([]);
      });
    return () => {
      alive = false;
    };
  }, []);

  const { status: mlx } = useMlxEngineStatusPoll(sidecarNames.length > 0);
  const mlxReplied = mlx?.state === 'running' && !mlx.probeError;

  return useMemo(() => {
    const mlxNodes = mlxReplied ? sidecarNames : [];
    const reported = new Set<string>([...Object.keys(nodeStatus), ...mlxNodes]);
    const busyNodes = Object.entries(nodeStatus)
      .filter(([, st]) => BUSY_STATES.has(st))
      .map(([n]) => n);
    return {
      nodeStatus,
      reportedNodes: Array.from(reported).sort(),
      busyNodes,
      mlxNodes,
    };
  }, [nodeStatus, mlxReplied, sidecarNames]);
}

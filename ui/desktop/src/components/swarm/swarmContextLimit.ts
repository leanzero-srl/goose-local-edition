import { acpReadConfig } from '../../acp/config';
import { mlxEngineStatus, type MlxEngineStatus } from '../../acp/mlx-engine';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import { fetchSwarmContextLimit } from './useFleet';

/**
 * The context window the `swarm` provider can rely on, read from the ENGINES the pool actually
 * runs on. `fetchSwarmContextLimit` alone reads LM Studio's `/api/v0/models`, so on an MLX-only
 * pool (this machine, 2026-09-05: one `mlx-sidecar` device, zero LM Studio devices) it answered
 * null and the composer fell to the generic 128k default while the sidecar served a model whose
 * window the engine status reports. Every engine present in the pool is asked; the MIN across
 * them is the honest ceiling the whole fleet can hold. Null when no engine answered — the caller
 * keeps its default and nothing is invented.
 */
export interface SwarmPoolLimitDeps {
  readConfig: () => Promise<unknown>;
  lmStudioLimit: () => Promise<number | null>;
  mlxStatus: () => Promise<MlxEngineStatus>;
}

/** Which engines the configured pool spans. No/empty devices is the legacy LM Studio discovery pool. */
export function poolEngines(cfg: SwarmConfig | null): { lmStudio: boolean; localMlx: boolean } {
  const rows: SwarmDeviceRow[] = Array.isArray(cfg?.devices) ? cfg.devices : [];
  const enabled = rows.filter((d) => d.enabled !== false);
  return {
    lmStudio: enabled.length === 0 || enabled.some((d) => d.engine !== 'mlx-sidecar'),
    localMlx: enabled.some((d) => d.engine === 'mlx-sidecar' && !d.host),
  };
}

export async function swarmPoolContextLimit(deps: SwarmPoolLimitDeps): Promise<number | null> {
  let cfg: SwarmConfig | null = null;
  try {
    cfg = ((await deps.readConfig()) as SwarmConfig | null) ?? null;
  } catch {
    // An unreadable config cannot prove the pool has no LM Studio node — poolEngines(null) keeps
    // the LM Studio read, which is what the composer did before this helper existed.
  }
  const engines = poolEngines(cfg);
  const reads: Array<Promise<number | null>> = [];
  if (engines.lmStudio) reads.push(deps.lmStudioLimit().catch(() => null));
  if (engines.localMlx) {
    reads.push(
      deps
        .mlxStatus()
        .then((s) => (s.state === 'running' && s.contextWindow != null ? s.contextWindow : null))
        .catch(() => null)
    );
  }
  const limits = (await Promise.all(reads)).filter((n): n is number => typeof n === 'number' && n > 0);
  return limits.length ? Math.min(...limits) : null;
}

export function fetchSwarmPoolContextLimit(): Promise<number | null> {
  return swarmPoolContextLimit({
    readConfig: () => acpReadConfig('swarm', false),
    lmStudioLimit: () => fetchSwarmContextLimit(),
    mlxStatus: () => mlxEngineStatus(),
  });
}

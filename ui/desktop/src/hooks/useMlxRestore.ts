import { useEffect } from 'react';
import { mlxEngineMount, mlxEngineStatus } from '../acp/mlx-engine';
import { mlxDistributedStart, mlxDistributedStatus } from '../acp/mlx-distributed';
import { mlxRemoteSingleStart, mlxRemoteSingleStatus } from '../acp/mlx-remote-single';
import { leanzeroLinkStatus } from '../acp/leanzero-link';
import { mlxServingIntent } from '../acp/mlx-serving-intent';
import { MLX_STATUS_POLL_MS } from '../components/leanzero-swarm/mlxLiveStats';
import { runRestore, type RestoreDeps } from '../components/leanzero-swarm/mlxRestore';
import { useFeatures } from '../contexts/FeaturesContext';

/** The restore's calls: the same ACP calls the Providers view and Run it make. */
export function restoreDeps(features: {
  mlxDistributed: boolean;
  leanzeroLink: boolean;
}): RestoreDeps {
  return {
    readIntent: mlxServingIntent,
    singleStatus: () => mlxEngineStatus(),
    mount: (modelId) => mlxEngineMount(modelId),
    remoteStatus: mlxRemoteSingleStatus,
    remoteStart: mlxRemoteSingleStart,
    distributedStatus: async () => (features.mlxDistributed ? mlxDistributedStatus() : null),
    distributedStart: () => mlxDistributedStart(null),
    linkState: async () => (features.leanzeroLink ? leanzeroLinkStatus() : null),
    wait: () => new Promise((resolve) => setTimeout(resolve, MLX_STATUS_POLL_MS)),
  };
}

/**
 * At launch, bring back what served chat before the app quit (components/leanzero-swarm/
 * mlxRestore.ts). Every window runs this hook and its own goosed, but ONE restore per app launch:
 * main hands the claim to the first window that asks (`mlx-restore-claim`).
 */
export function useMlxRestore(): void {
  const { mlxEngine, mlxDistributed, leanzeroLink, isLoading } = useFeatures();
  useEffect(() => {
    if (isLoading || !mlxEngine) return undefined;
    const claim = (window as unknown as { electron?: { mlxRestoreClaim?: () => Promise<boolean> } })
      .electron?.mlxRestoreClaim;
    if (!claim) return undefined;
    // Once claimed the restore runs to its end whatever this component does: it lives in module
    // state (the line every surface reads), and a claim spent on a remount would restore nothing.
    void claim().then((mine) => {
      if (mine) void runRestore(restoreDeps({ mlxDistributed, leanzeroLink }));
    });
    return undefined;
  }, [isLoading, mlxEngine, mlxDistributed, leanzeroLink]);
}

import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import { mlxEngineServing } from '../chatServedBy/chatServedBy';

export interface RecipeChatTarget {
  /** The id chat requests carry on that engine (its served alias). */
  model: string;
  /** Its OpenAI base URL on this Mac (the split's rank 0, the route's loopback relay, the sidecar). */
  baseUrl: string;
  where: 'split' | 'remote' | 'single';
}

/**
 * The engine goose serves chat with, as the recipe interview must reach it (Q-7). The wizard read
 * only this Mac's SINGLE engine (and LM Studio): with chat on the split across the Macs, or routed
 * to a linked Mac, it said "no fleet model is served" while a model answered every chat. The
 * engine is chosen by `mlxEngineServing` — the one rule the chip and the counter read — and only
 * while it answers; null when nothing serves (the wizard says so, never a guess).
 */
export function recipeChatTarget(
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  remote: MlxRemoteSingleStatus | null
): RecipeChatTarget | null {
  const serving = mlxEngineServing(single, distributed, remote, '');
  const pick = (
    where: RecipeChatTarget['where'],
    up: boolean,
    s: { servedModelId?: string | null; modelId?: string | null; baseUrl?: string | null } | null
  ): RecipeChatTarget | null => {
    const model = s?.servedModelId ?? s?.modelId ?? null;
    return up && model && s?.baseUrl ? { model, baseUrl: s.baseUrl, where } : null;
  };
  switch (serving.engine) {
    case 'split':
      return serving.foreign
        ? null
        : pick(
            'split',
            distributed?.state === 'ready' || distributed?.state === 'serving',
            distributed
          );
    case 'remote':
      return pick('remote', remote?.state === 'ready', remote);
    case 'single':
      return pick('single', single?.state === 'running' && !single.probeError, single);
    default:
      return null;
  }
}

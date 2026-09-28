import { useCallback } from 'react';
import { useModelAndProvider } from '../ModelAndProviderContext';
import { SWARM_DISPLAY_NAME, SWARM_PROVIDER_ID } from '../../branding';

/**
 * "Use this node for this chat" (design §8.5): sets THIS session's model to a node, a strategy or
 * Any node (Auto) — `node:<id>`, `strategy:<id>`, `swarm` on the swarm provider — through the same
 * `changeModel` the model picker uses (acpSetSessionProviderModel; its own toast on either outcome).
 * The next turn shows the loader line when the node needs loading. Resolves whether it was applied.
 */
export function useRunChatOn(
  sessionId: string | null,
  onModelChanged?: (override: { model: string; provider: string }) => void
): (model: string, label: string) => Promise<boolean> {
  const { changeModel } = useModelAndProvider();
  return useCallback(
    async (model: string, label: string) => {
      const ok = await changeModel(sessionId, {
        name: model,
        provider: SWARM_PROVIDER_ID,
        alias: label,
        subtext: SWARM_DISPLAY_NAME,
      });
      if (ok) onModelChanged?.({ model, provider: SWARM_PROVIDER_ID });
      return ok;
    },
    [changeModel, sessionId, onModelChanged]
  );
}

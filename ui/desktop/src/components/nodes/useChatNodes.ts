import { useCallback } from 'react';
import { nodesSetChatNodes } from '../../acp/nodes';
import { SWARM_PROVIDER_ID } from '../../branding';
import { noteSessionProviderModel } from '../ModelAndProviderContext';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';

/** A write's outcome: applied (the chat now runs on `model`, or its set is gone), or refused. */
export type ChatNodesWrite = { applied: true; model: string | null } | { applied: false; refusals: string[] };

/**
 * The chip's one door to a chat's own nodes (Q-359): goosed's `nodes/setChatNodes`, which stores
 * the set AND sets the chat's model to it in one call. What it answers is mirrored at once into the
 * two places the renderer holds a chat's model (the composer's override and the session store), and
 * the nodes are read again (an event, never a clock). A refusal comes back in goosed's words.
 */
export function useChatNodes(
  sessionId: string | null,
  onModelChanged?: (override: { model: string; provider: string }) => void
): (nodes: string[], answerOnNext: boolean) => Promise<ChatNodesWrite> {
  return useCallback(
    async (nodes: string[], answerOnNext: boolean): Promise<ChatNodesWrite> => {
      if (!sessionId) return { applied: false, refusals: [] };
      try {
        const answer = await nodesSetChatNodes(sessionId, nodes, answerOnNext);
        if (!answer.write.written && nodes.length > 0) {
          return {
            applied: false,
            refusals: (answer.write.refusals ?? []).map((r) => r.message),
          };
        }
        const model = answer.model ?? null;
        if (model) {
          noteSessionProviderModel(sessionId, SWARM_PROVIDER_ID, model);
          onModelChanged?.({ model, provider: SWARM_PROVIDER_ID });
        }
        return { applied: true, model };
      } catch (e) {
        return { applied: false, refusals: [mlxErrorMessage(e, String(e))] };
      } finally {
        refreshGlanceNodes();
      }
    },
    [sessionId, onModelChanged]
  );
}

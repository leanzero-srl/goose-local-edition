import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { NodeSwap } from '../../utils/nodeSwap';
import { loadPhaseWord } from '../nodes/loadPhaseWord';

const i18n = defineMessages({
  // DESIGN-NODES-AND-STRATEGIES.md §8.7, as the table words it.
  turnLoading: {
    id: 'nodes.turnLoading',
    defaultMessage: 'Loading {node} for this chat: {phase}',
  },
  // The ledger's words for a way the loader stopped (Q-254): what the stopped way reads, on every
  // surface, while the loader loads the node — never "Failed" or "No model is mounted".
  swappingTo: { id: 'nodes.swappingTo', defaultMessage: 'Swapping to {node}' },
});

/**
 * The node loader's state as ONE chat sees it (chatServedBy.ts `chatLoaderOf`):
 *  - `loading`: the loader is loading `swap.target`; `forThisChat` when this chat's turn is in flight
 *    and the node is one its model runs on (`node:`/`strategy:`) — the turn waits on this load;
 *  - `waiting`: this chat's turn is queued in the loader for one of its nodes, in the loader's words.
 */
export type ChatLoader =
  | { kind: 'loading'; swap: NodeSwap; forThisChat: boolean }
  | { kind: 'waiting'; node: string; reason: string };

/** "Swapping to {node}" — the words every surface says a way the loader stopped with. */
export function swappingText(intl: IntlShape, node: string): string {
  return intl.formatMessage(i18n.swappingTo, { node });
}

/**
 * The loader line — the composer bar's headline, the chip's word and its menu line read this one
 * text: "Loading {node} for this chat: {phase}" once the way names its phase, "Swapping to {node}"
 * before it does or for a chat the load is not for, and a wait in the loader's own words.
 */
export function loaderText(intl: IntlShape, loader: ChatLoader): string {
  // The loader's own sentence, as it wrote it — only its first letter raised for a headline.
  if (loader.kind === 'waiting')
    return loader.reason.charAt(0).toUpperCase() + loader.reason.slice(1);
  const { swap, forThisChat } = loader;
  if (forThisChat && swap.phase != null) {
    return intl.formatMessage(i18n.turnLoading, {
      node: swap.target.name,
      phase: loadPhaseWord(intl, swap.phase),
    });
  }
  return swappingText(intl, swap.target.name);
}

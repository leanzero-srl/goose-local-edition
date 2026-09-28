import type { IntlShape } from 'react-intl';
import type { NodeRole, NodeServedTurnDto, NodesReadResponse_unstable } from '@aaif/goose-sdk';
import { defineMessages } from '../../i18n';
import { ROLE_WORD } from '../nodes/NodeChips';
import { nodeNamesById } from '../nodes/model';

const i18n = defineMessages({
  // DESIGN-NODES-AND-STRATEGIES.md §8.7 `nodes.fellBack`, as the table words it.
  fellBack: {
    id: 'nodes.fellBack',
    defaultMessage: "{role} is on {node} ({rank}): {primary} can't run: {reason}",
  },
  rank: {
    id: 'nodes.rankOrdinal',
    defaultMessage: '{rank, selectordinal, one {#st} two {#nd} few {#rd} other {#th}}',
  },
  retry: { id: 'nodes.fellBackRetry', defaultMessage: 'Retry {primary}' },
});

/**
 * The turn this chat was last served took a later entry of its role's chain (the router's served
 * record, `nodes/servedLast`): which role, on which node, its rank, and why the 1st could not run.
 */
export interface ChatFellBack {
  role: NodeRole;
  node: string;
  rank: number;
  primary: string;
  primaryId: string;
  reason: string;
}

/**
 * The fallback the router recorded for the last turn; null when the 1st served, the record names no
 * role (a `node:` or Auto chat), or it does not say which entry was the 1st and why it did not run.
 */
export function fellBackOf(
  record: NodeServedTurnDto | null | undefined,
  read: NodesReadResponse_unstable
): ChatFellBack | null {
  if (!record || record.rank <= 1 || record.role == null || record.reason == null) return null;
  // The router's reason is the 1st entry's words in `tried` (swarm_router.rs `served_turn`).
  const primary = (record.tried ?? []).find((t) => t.reason === record.reason);
  if (!primary) return null;
  const names = nodeNamesById(read.nodes);
  return {
    role: record.role,
    node: names[record.node] ?? record.node,
    rank: record.rank,
    primary: names[primary.node] ?? primary.node,
    primaryId: primary.node,
    reason: record.reason,
  };
}

export function fellBackText(intl: IntlShape, fell: ChatFellBack): string {
  return intl.formatMessage(i18n.fellBack, {
    role: intl.formatMessage(ROLE_WORD[fell.role]),
    node: fell.node,
    rank: intl.formatMessage(i18n.rank, { rank: fell.rank }),
    primary: fell.primary,
    reason: fell.reason,
  });
}

export function fellBackRetryText(intl: IntlShape, fell: ChatFellBack): string {
  return intl.formatMessage(i18n.retry, { primary: fell.primary });
}

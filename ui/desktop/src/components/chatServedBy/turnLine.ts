import type { IntlShape } from 'react-intl';
import type {
  NodeRole,
  NodeServedTurnDto,
  NodeServingOtherDto,
  NodesReadResponse_unstable,
} from '@aaif/goose-sdk';
import { defineMessages } from '../../i18n';
import { ROLE_WORD } from '../nodes/NodeChips';
import { nodeNamesById } from '../nodes/model';
import { servingOtherNamed } from '../../utils/nodeSwap';

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
  // Q-441 / Q-443: an action that stops the node the Mac serves for other chats says so.
  takeOverNow: {
    id: 'nodes.takeOverNow',
    defaultMessage: 'Load {node} now (stops {serving} for {chats})',
  },
  takeOverNowHint: {
    id: 'nodes.takeOverNowHint',
    defaultMessage:
      'A reply running on {serving} finishes first; nothing is cut. The strategy’s setting stays as it is.',
  },
  // Q-428: the 1st was left to the node its Mac serves for other chats (the role's "Use the next
  // node"): no "can't run" — it could, and was not interrupted.
  fellBackServingOther: {
    id: 'nodes.fellBackServingOther',
    defaultMessage: '{role} is on {node} ({rank}): {mac} is serving {serving} for {chats}',
  },
  servingChats: {
    id: 'nodes.servingChats',
    defaultMessage: '{count, plural, one {chat {names}} other {chats {names}}}',
  },
  servingChatQuoted: { id: 'nodes.servingChatQuoted', defaultMessage: '"{chat}"' },
  servingAnotherChat: { id: 'nodes.servingAnotherChat', defaultMessage: 'another chat' },
  // Q-381: the person asked this one turn onto a later node ("Answer on {next} for now").
  fellBackAsked: {
    id: 'nodes.fellBackAsked',
    defaultMessage:
      '{role} is on {node} ({rank}) for this turn, as you asked. The next message goes to {primary} again.',
  },
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
  /** The person asked this turn past the 1st ("Answer on {next} for now", Q-381). */
  asked: boolean;
  /** The 1st was left to the node its Mac serves for other chats (Q-428); null = another reason. */
  servingOther: NodeServingOtherDto | null;
}

/** 'chat "Kickoff notes"', 'chats "A" and "B"' — the chats a node serves, in the person's words. */
export function servingChatsText(intl: IntlShape, chats: readonly string[]): string {
  if (chats.length === 0) return intl.formatMessage(i18n.servingAnotherChat);
  return intl.formatMessage(i18n.servingChats, {
    count: chats.length,
    names: intl.formatList(
      chats.map((chat) => intl.formatMessage(i18n.servingChatQuoted, { chat }))
    ),
  });
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
    asked: record.askedForThisTurn === true,
    servingOther: record.servingOther ? servingOtherNamed(read, record.servingOther) : null,
  };
}

export function fellBackText(intl: IntlShape, fell: ChatFellBack): string {
  if (fell.asked) {
    return intl.formatMessage(i18n.fellBackAsked, {
      role: intl.formatMessage(ROLE_WORD[fell.role]),
      node: fell.node,
      rank: intl.formatMessage(i18n.rank, { rank: fell.rank }),
      primary: fell.primary,
    });
  }
  if (fell.servingOther) {
    return intl.formatMessage(i18n.fellBackServingOther, {
      role: intl.formatMessage(ROLE_WORD[fell.role]),
      node: fell.node,
      rank: intl.formatMessage(i18n.rank, { rank: fell.rank }),
      mac: fell.servingOther.mac,
      serving: fell.servingOther.serving,
      chats: servingChatsText(intl, fell.servingOther.chats),
    });
  }
  return intl.formatMessage(i18n.fellBack, {
    role: intl.formatMessage(ROLE_WORD[fell.role]),
    node: fell.node,
    rank: intl.formatMessage(i18n.rank, { rank: fell.rank }),
    primary: fell.primary,
    reason: fell.reason,
  });
}

/**
 * The fallback line's action. On a Q-428 "Use the next node" line it is NOT a retry: the loader
 * would stop the node the Mac serves for the chats it names (a demand from no turn takes the Mac
 * over) — so it says exactly that.
 */
export function fellBackRetryText(intl: IntlShape, fell: ChatFellBack): string {
  if (fell.servingOther) return takeOverNowText(intl, fell.primary, fell.servingOther);
  return intl.formatMessage(i18n.retry, { primary: fell.primary });
}

/** "Load {node} now (stops {serving} for chat "…")" — Q-441's and Q-443's one label. */
export function takeOverNowText(
  intl: IntlShape,
  node: string,
  other: NodeServingOtherDto
): string {
  return intl.formatMessage(i18n.takeOverNow, {
    node,
    serving: other.serving,
    chats: servingChatsText(intl, other.chats),
  });
}

export function takeOverNowHint(intl: IntlShape, other: NodeServingOtherDto): string {
  return intl.formatMessage(i18n.takeOverNowHint, { serving: other.serving });
}

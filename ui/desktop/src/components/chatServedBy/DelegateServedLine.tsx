import { useEffect, useState } from 'react';
import type { IntlShape } from 'react-intl';
import type { NodeServedTurnDto } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { nodesServedLast } from '../../acp/nodes';
import { useGlanceNodes } from '../engineGlance/glanceStore';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { nodeNamesById } from '../nodes/model';
import { TYPE, cx } from '../lz';

/**
 * The node a delegate ran on (Q-359: a chat's delegates share its nodes), under the delegate's
 * card: the router's served record of the delegate's own session (`nodes/servedLast`), read once
 * the delegate is done — an event, never a poll. No record (a delegate on Auto or another provider,
 * or one that never reached a node) says nothing: there is no node to name. A read that failed says
 * so in its own words — never a guessed node.
 */

const i18n = defineMessages({
  on: { id: 'delegateServed.on', defaultMessage: 'on {node}' },
  loaded: {
    id: 'delegateServed.loaded',
    defaultMessage: 'on {node} · loaded for this delegate in {duration}',
  },
  passedOver: {
    id: 'delegateServed.passedOver',
    defaultMessage: 'on {node}: {primary} can’t run ({reason})',
  },
  unread: {
    id: 'delegateServed.unread',
    defaultMessage: 'The node this delegate ran on could not be read: {error}',
  },
});

/** The words for a delegate's served record, names read from the nodes (never ids, Q-255). */
export function delegateServedText(
  intl: IntlShape,
  record: NodeServedTurnDto,
  names: Record<string, string>
): string {
  const node = names[record.node] ?? record.node;
  // The router's reason is the chain's 1st entry's words in `tried` (swarm_router.rs chain_record).
  const primary =
    record.reason != null ? (record.tried ?? []).find((t) => t.reason === record.reason) : null;
  if (primary && primary.node !== record.node) {
    return intl.formatMessage(i18n.passedOver, {
      node,
      primary: names[primary.node] ?? primary.node,
      reason: record.reason,
    });
  }
  if (record.loadedMs != null) {
    return intl.formatMessage(i18n.loaded, {
      node,
      duration: formatElapsed(record.loadedMs / 1000),
    });
  }
  return intl.formatMessage(i18n.on, { node });
}

export function DelegateServedLine({ sessionId }: { sessionId: string }) {
  const intl = useIntl();
  const store = useGlanceNodes();
  const [answer, setAnswer] = useState<
    { kind: 'record'; record: NodeServedTurnDto } | { kind: 'failed'; error: string } | null
  >(null);
  useEffect(() => {
    let alive = true;
    nodesServedLast(sessionId)
      .then((read) => alive && setAnswer(read.record ? { kind: 'record', record: read.record } : null))
      .catch(
        (e: unknown) =>
          alive && setAnswer({ kind: 'failed', error: mlxErrorMessage(e, String(e)) })
      );
    return () => {
      alive = false;
    };
  }, [sessionId]);
  if (!answer) return null;
  const names = store.kind === 'read' ? nodeNamesById(store.read.nodes) : {};
  return (
    <p className={cx('px-4 py-1.5 break-words', TYPE.meta)} data-testid="delegate-served-line">
      {answer.kind === 'record'
        ? delegateServedText(intl, answer.record, names)
        : intl.formatMessage(i18n.unread, { error: answer.error })}
    </p>
  );
}

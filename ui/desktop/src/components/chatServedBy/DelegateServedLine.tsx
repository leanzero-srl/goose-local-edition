import { useEffect, useState } from 'react';
import type { IntlShape } from 'react-intl';
import type { NodeServedTurnDto, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { nodesServedLast } from '../../acp/nodes';
import { useGlanceNodes } from '../engineGlance/glanceStore';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { loadPhaseWord } from '../nodes/loadPhaseWord';
import { nodeNamesById } from '../nodes/model';
import { TYPE, cx } from '../lz';

/**
 * The node a delegate runs on (Q-359: a chat's delegates share its nodes), under the delegate's
 * card. While the node loader LOADS a node this delegate demanded (Q-382 — the load's own fact,
 * `nodes/residency`'s `loading.demandedBy`), the card says so with the load's phase. Otherwise it
 * is the router's served record of the delegate's own session (`nodes/servedLast`), read when the
 * card mounts, when the delegate's run ends and when its load ends — events, never a poll. No
 * record (a delegate on Auto or another provider, or one that never reached a node) says nothing:
 * there is no node to name. A read that failed says so in its own words — never a guessed node.
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
  loading: {
    id: 'delegateServed.loading',
    defaultMessage: 'Loading {node} for this delegate: {phase}',
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

/** The node the loader is loading for `sessionId` (a load it demanded), with the load's phase. */
export function delegateLoadOf(
  residency: NodesResidencyResponse_unstable,
  sessionId: string
): { node: string; phase: string | null } | null {
  for (const { node, residency: r } of residency.nodes) {
    if (r.kind === 'loading' && (r.demandedBy ?? []).includes(sessionId)) {
      return { node, phase: r.phase ?? null };
    }
  }
  return null;
}

export function DelegateServedLine({
  sessionId,
  running = false,
}: {
  sessionId: string;
  /** The delegate's run is still going (its card is loading). */
  running?: boolean;
}) {
  const intl = useIntl();
  const store = useGlanceNodes();
  const load = store.kind === 'read' ? delegateLoadOf(store.residency, sessionId) : null;
  const loading = load != null;
  const [answer, setAnswer] = useState<
    { kind: 'record'; record: NodeServedTurnDto } | { kind: 'failed'; error: string } | null
  >(null);
  useEffect(() => {
    if (loading) return;
    let alive = true;
    nodesServedLast(sessionId)
      .then(
        (read) => alive && setAnswer(read.record ? { kind: 'record', record: read.record } : null)
      )
      .catch(
        (e: unknown) => alive && setAnswer({ kind: 'failed', error: mlxErrorMessage(e, String(e)) })
      );
    return () => {
      alive = false;
    };
  }, [sessionId, running, loading]);
  const names = store.kind === 'read' ? nodeNamesById(store.read.nodes) : {};
  if (load) {
    return (
      <p className={cx('px-4 py-1.5 break-words', TYPE.meta)} data-testid="delegate-loading-line">
        {intl.formatMessage(i18n.loading, {
          node: names[load.node] ?? load.node,
          phase: loadPhaseWord(intl, load.phase),
        })}
      </p>
    );
  }
  if (!answer) return null;
  return (
    <p className={cx('px-4 py-1.5 break-words', TYPE.meta)} data-testid="delegate-served-line">
      {answer.kind === 'record'
        ? delegateServedText(intl, answer.record, names)
        : intl.formatMessage(i18n.unread, { error: answer.error })}
    </p>
  );
}

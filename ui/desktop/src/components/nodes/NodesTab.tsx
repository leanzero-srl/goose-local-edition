import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { ArrowRight, Eye, Laptop, Plus, RefreshCw, Server } from 'lucide-react';
import type { NodesServingKind } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { Button, EmptyState, RADIUS, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { ToneBanner } from '../leanzero-swarm/studio';
import { WithMacs } from '../leanzero-swarm/useMacs';
import { useCutGuard } from '../leanzero-swarm/cutGuard';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { dropRoute } from '../leanzero-swarm/routeSwitch';
import { mlxEngineUnmount } from '../../acp/mlx-engine';
import { mlxDistributedStop } from '../../acp/mlx-distributed';
import { nodesEnsureServing, nodesRemoveNode, nodesWrite } from '../../acp/nodes';
import type { MlxEngineKind } from '../../utils/mlxInFlight';
import { cloudHref, mlxHref, nodesHref } from '../../utils/navigationUtils';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import { NodeCard, type NodeCardAction } from './NodeCard';
import { NewNodeDialog, type NewNodeStart } from './NewNodeDialog';
import { usedByOf } from './nodeGlance';
import { useNodeFacts } from './useNodeFacts';
import { nodeIdFor, putNode, uniqueName } from './nodeDraft';
import { RemoveConfirmDialog, type RemoveConfirmation } from './RemoveConfirmDialog';
import { chatNodeSetCount, type NodeDef, type NodesConfig, type ResolvedNodeDef } from './model';

/**
 * THE NODES TAB (DESIGN-NODES-AND-STRATEGIES.md §8.2): every node definition as a card, in two
 * groups — On your Macs (LeanZero MLX) and In the cloud — with New node, the one-Mac hint and the
 * empty state. S6's NodesView hosts it (and "Your swarm pool" under it).
 *
 * It adds NO poller: its facts are `useNodeFacts` (the same set the Strategies tab's pickers read),
 * under `WithMacs` mounted here exactly as Providers mounts it (the two never render together).
 */

const i18n = defineMessages({
  new: { id: 'nodes.new', defaultMessage: 'New node' },
  groupMacs: { id: 'nodes.groupMacs', defaultMessage: 'On your Macs · LeanZero MLX' },
  groupCloud: { id: 'nodes.groupCloud', defaultMessage: 'In the cloud' },
  manageMacs: { id: 'nodes.manageMacs', defaultMessage: 'Manage Macs and models' },
  manageCloud: { id: 'nodes.manageCloud', defaultMessage: 'Manage cloud providers' },
  oneMacHint: {
    id: 'nodes.oneMacHint',
    defaultMessage: 'Add another Mac to run models too big for this one',
  },
  emptyTitle: { id: 'nodes.emptyTitle', defaultMessage: 'No nodes yet' },
  emptyBody: {
    id: 'nodes.emptyBody',
    defaultMessage: 'Connect your Macs and run a model, or add a cloud model.',
  },
  setUpMacs: { id: 'nodes.setUpMacs', defaultMessage: 'Set up your Macs' },
  lmStudioHidden: {
    id: 'nodes.lmStudioHidden',
    defaultMessage:
      '{count, plural, one {# LM Studio device} other {# LM Studio devices}} in your swarm config {count, plural, one {is} other {are}} not shown here',
  },
  readFailed: { id: 'nodes.readFailed', defaultMessage: 'Your nodes could not be read' },
  swarmUnreadable: {
    id: 'nodes.swarmUnreadable',
    defaultMessage: 'Your swarm pool could not be read',
  },
  reading: { id: 'nodes.reading', defaultMessage: 'Reading your nodes…' },
  retry: { id: 'nodes.retry', defaultMessage: 'Read again' },
  startTitle: { id: 'nodes.startTitle', defaultMessage: 'Start {node}?' },
  startStops: { id: 'nodes.startStops', defaultMessage: 'Starting it stops {names}.' },
  keepRunning: { id: 'nodes.keepRunning', defaultMessage: 'Keep {names}' },
  startAction: { id: 'nodes.startAction', defaultMessage: 'Start {node}' },
  stopAction: { id: 'nodes.stopAction', defaultMessage: 'Stop {node}' },
  started: { id: 'nodes.started', defaultMessage: '{node} is serving.' },
  failed: { id: 'nodes.actionFailed', defaultMessage: 'The action failed' },
  saved: { id: 'nodes.saved', defaultMessage: 'Saved.' },
  removeTitle: { id: 'nodes.removeTitle', defaultMessage: 'Remove {node}?' },
  removeBody: {
    id: 'nodes.removeBody',
    defaultMessage:
      'The node is removed from your nodes. Your Macs, models and providers stay as they are.',
  },
  removePoolBody: {
    id: 'nodes.removePoolBody',
    defaultMessage:
      'Only this card goes: its model stays in your swarm pool, so chats on Any node (Auto) keep running on it, and so do swarm builds that use the pool. To bring the card back, choose “Show removed pool nodes” on this page.',
  },
  removedPoolNodes: {
    id: 'nodes.removedPoolNodes',
    defaultMessage:
      '{count, plural, one {# node you removed from your swarm pool is not shown} other {# nodes you removed from your swarm pool are not shown}}',
  },
  showRemovedPoolNodes: {
    id: 'nodes.showRemovedPoolNodes',
    defaultMessage: 'Show removed pool nodes',
  },
  removeFromStrategies: {
    id: 'nodes.removeAlsoFromStrategies',
    defaultMessage: 'Also remove it from {strategies}',
  },
  removeFromUsingStrategies: {
    id: 'nodes.removeAlsoFromUsingStrategies',
    defaultMessage: 'Also remove it from the strategies that use it',
  },
  removeFromStrategiesWhy: {
    id: 'nodes.removeAlsoFromStrategiesWhy',
    defaultMessage:
      '{count, plural, one {A strategy uses this node} other {# strategies use this node}}, so it can’t be removed on its own.',
  },
  removeFromChatNodeSets: {
    id: 'nodes.removeAlsoFromChatNodeSets',
    defaultMessage:
      '{count, plural, one {Also take it out of # chat’s node set} other {Also take it out of # chats’ node sets}}',
  },
  removeFromChatNodeSetsWhy: {
    id: 'nodes.removeAlsoFromChatNodeSetsWhy',
    defaultMessage:
      '{count, plural, one {A chat runs on it with other nodes} other {# chats run on it with other nodes}}. A chat left with no node is told so on its next message.',
  },
  removeNewChatsAuto: {
    id: 'nodes.removeNewChatsAuto',
    defaultMessage: 'Start new chats on Any node (Auto) instead',
  },
  removeNewChatsAutoWhy: {
    id: 'nodes.removeNewChatsAutoWhy',
    defaultMessage: 'New chats start on this node now, so it can’t be removed on its own.',
  },
  removeLiveChats: {
    id: 'nodes.removeLiveChats',
    defaultMessage:
      'Remove it anyway: {count, plural, one {# chat is} other {# chats are}} set to this node',
  },
  removeLiveChatsWhy: {
    id: 'nodes.removeLiveChatsWhy',
    defaultMessage:
      '{count, plural, one {Its} other {Their}} next message will say the node was removed and ask you to pick another.',
  },
});

const ENGINE_OF: Record<NodesServingKind, MlxEngineKind> = {
  single: 'single',
  remoteSingle: 'remote',
  split: 'distributed',
};

type Notice = { tone: 'ok' | 'err'; text: string };

/** The busy key of "Show removed pool nodes" (node ids never start with ':' — `valid_id`). */
const RESTORE_BUSY = ':restore-pool';

interface RemoveState {
  node: ResolvedNodeDef;
  alsoFromStrategies: boolean;
  alsoFromChatNodeSets: boolean;
  andNewChatsAuto: boolean;
  acknowledged: number | null;
  refusals: { code: string; message: string; liveSessions?: number | null }[];
  busy: boolean;
}

/**
 * A node's removal, its confirmations built from the config the dialog was opened on (the
 * strategies that use it, new chats starting on it) and from the engine's own count of the chats
 * set to it (it arrives as a refusal carrying `liveSessions`; there is no other reader of it).
 * A refusal a box answers is never shown again as an error (Q-259).
 */
function RemoveDialog({
  state,
  config,
  onChange,
  onConfirm,
  onClose,
}: {
  state: RemoveState;
  config: NodesConfig | null;
  onChange: (next: RemoveState) => void;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const intl = useIntl();
  const id = state.node.def.id;
  const codes = new Set(state.refusals.map((r) => r.code));
  const strategyNames = [
    ...new Set((config ? usedByOf(config, id) : []).map((u) => u.strategyName)),
  ];
  const inStrategies = strategyNames.length > 0 || codes.has('nodeInUse');
  const chatSets = config ? chatNodeSetCount(config, id) : 0;
  const inChatSets = chatSets > 0 || codes.has('nodeInChatNodeSets');
  const forNewChats =
    (config?.forNewChats?.kind === 'node' && config.forNewChats.id === id) ||
    codes.has('nodeIsForNewChats');
  // The count rides the refusal as a number (`liveSessions`); words without it offer nothing to
  // acknowledge, never a count guessed from the message — they stay a refusal, in red.
  const liveCount =
    state.refusals.find((r) => r.code === 'liveSessionsNotAcknowledged')?.liveSessions ?? null;

  const confirmations: RemoveConfirmation[] = [];
  if (inStrategies) {
    confirmations.push({
      key: 'strategies',
      label:
        strategyNames.length > 0
          ? intl.formatMessage(i18n.removeFromStrategies, {
              strategies: intl.formatList(strategyNames, { type: 'conjunction' }),
            })
          : intl.formatMessage(i18n.removeFromUsingStrategies),
      description: intl.formatMessage(i18n.removeFromStrategiesWhy, {
        count: Math.max(strategyNames.length, 1),
      }),
      checked: state.alsoFromStrategies,
      onChange: (v) => onChange({ ...state, alsoFromStrategies: v }),
      testId: 'node-remove-from-strategies',
    });
  }
  if (inChatSets) {
    confirmations.push({
      key: 'chatNodeSets',
      label: intl.formatMessage(i18n.removeFromChatNodeSets, { count: Math.max(chatSets, 1) }),
      description: intl.formatMessage(i18n.removeFromChatNodeSetsWhy, {
        count: Math.max(chatSets, 1),
      }),
      checked: state.alsoFromChatNodeSets,
      onChange: (v) => onChange({ ...state, alsoFromChatNodeSets: v }),
      testId: 'node-remove-from-chat-node-sets',
    });
  }
  if (forNewChats) {
    confirmations.push({
      key: 'newChats',
      label: intl.formatMessage(i18n.removeNewChatsAuto),
      description: intl.formatMessage(i18n.removeNewChatsAutoWhy),
      checked: state.andNewChatsAuto,
      onChange: (v) => onChange({ ...state, andNewChatsAuto: v }),
      testId: 'node-remove-and-auto',
    });
  }
  if (liveCount != null) {
    confirmations.push({
      key: 'liveChats',
      label: intl.formatMessage(i18n.removeLiveChats, { count: liveCount }),
      description: intl.formatMessage(i18n.removeLiveChatsWhy, { count: liveCount }),
      checked: state.acknowledged === liveCount,
      onChange: (v) => onChange({ ...state, acknowledged: v ? liveCount : null }),
      testId: 'node-remove-acknowledge',
    });
  }
  const answered = new Set<string>(['nodeInUse', 'nodeInChatNodeSets', 'nodeIsForNewChats']);
  if (liveCount != null) answered.add('liveSessionsNotAcknowledged');

  return (
    <RemoveConfirmDialog
      title={intl.formatMessage(i18n.removeTitle, { node: state.node.def.name })}
      body={intl.formatMessage(state.node.def.poolDevice ? i18n.removePoolBody : i18n.removeBody)}
      confirmations={confirmations}
      refusals={state.refusals.filter((r) => !answered.has(r.code))}
      busy={state.busy}
      onConfirm={onConfirm}
      onClose={onClose}
      testIdPrefix="node-remove"
    />
  );
}

export interface NodesTabProps {
  /** "Edit in your swarm pool": the host scrolls to (or opens) the pool section. */
  onEditInPool: () => void;
}

export function NodesTab(props: NodesTabProps) {
  return (
    <WithMacs>
      <NodesTabBody {...props} />
    </WithMacs>
  );
}

function NodesTabBody({ onEditInPool }: NodesTabProps) {
  const intl = useIntl();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const highlight = searchParams.get('node');
  const { store, nodes, servingNode, macCount, glanceOf } = useNodeFacts();
  const { guard, dialog: cutDialog } = useCutGuard();

  const read = store.kind === 'read' ? store.read : null;
  const residency = store.kind === 'read' ? store.residency : null;
  const mlxNodes = nodes.filter((n) => n.def.kind === 'mlx');
  const cloudNodes = nodes.filter((n) => n.def.kind !== 'mlx');

  const [notices, setNotices] = useState<Record<string, Notice>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [dialog, setDialog] = useState<NewNodeStart | null>(null);
  const [removing, setRemoving] = useState<RemoveState | null>(null);
  const [restoreNotice, setRestoreNotice] = useState<string | null>(null);
  const declinedCount = read?.config.declined?.length ?? 0;
  const say = (id: string, notice: Notice) => setNotices((prev) => ({ ...prev, [id]: notice }));

  const highlighted = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (highlight) highlighted.current?.scrollIntoView?.({ block: 'center' });
  }, [highlight, nodes.length]);

  const startNode = async (node: ResolvedNodeDef) => {
    setBusy(node.def.id);
    try {
      const answer = await nodesEnsureServing(node.def.id);
      say(
        node.def.id,
        answer.kind === 'ready'
          ? { tone: 'ok', text: intl.formatMessage(i18n.started, { node: node.def.name }) }
          : answer.kind === 'wait'
            ? { tone: 'ok', text: answer.reason }
            : { tone: 'err', text: answer.reason }
      );
    } catch (e) {
      say(node.def.id, { tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.failed)) });
    } finally {
      setBusy(null);
      refreshGlanceNodes();
    }
  };

  const stopServing = async (node: ResolvedNodeDef, kind: NodesServingKind) => {
    setBusy(node.def.id);
    try {
      if (kind === 'single') await mlxEngineUnmount();
      else if (kind === 'remoteSingle') await dropRoute().routeGone;
      else await mlxDistributedStop();
    } catch (e) {
      say(node.def.id, { tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.failed)) });
    } finally {
      setBusy(null);
      refreshGlanceNodes();
    }
  };

  const writeDef = async (def: NodeDef) => {
    setBusy(def.id);
    try {
      const response = await putNode(def, read?.config);
      say(
        def.id,
        response.written
          ? { tone: 'ok', text: intl.formatMessage(i18n.saved) }
          : { tone: 'err', text: (response.refusals ?? []).map((r) => r.message).join('; ') }
      );
    } catch (e) {
      say(def.id, { tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.failed)) });
    } finally {
      setBusy(null);
      refreshGlanceNodes();
    }
  };

  const remove = async (state: RemoveState) => {
    setRemoving({ ...state, busy: true });
    try {
      const response = await nodesRemoveNode(state.node.def.id, {
        alsoFromStrategies: state.alsoFromStrategies,
        ...(state.alsoFromChatNodeSets ? { alsoFromChatNodeSets: true } : {}),
        andNewChatsAuto: state.andNewChatsAuto,
        ...(state.acknowledged != null ? { acknowledgedSessions: state.acknowledged } : {}),
      });
      if (response.written) {
        setRemoving(null);
        refreshGlanceNodes();
        return;
      }
      setRemoving({ ...state, busy: false, refusals: response.refusals ?? [] });
    } catch (e) {
      setRemoving({
        ...state,
        busy: false,
        refusals: [{ code: 'error', message: mlxErrorMessage(e, intl.formatMessage(i18n.failed)) }],
      });
    }
  };

  /** Every pool node the person removed comes back: adoption takes a device no longer declined. */
  const showRemovedPoolNodes = async () => {
    if (!read) return;
    setBusy(RESTORE_BUSY);
    try {
      const response = await nodesWrite({ ...read.config, declined: [] });
      if (!response.written) {
        setRestoreNotice((response.refusals ?? []).map((r) => r.message).join('; '));
      } else {
        setRestoreNotice(null);
      }
    } catch (e) {
      setRestoreNotice(mlxErrorMessage(e, intl.formatMessage(i18n.failed)));
    } finally {
      setBusy(null);
      refreshGlanceNodes();
    }
  };

  /** "Create and start": what it stops is said beside the button; the cut guard still asks when
   * a reply is being written on the engine it stops (Q-148), exactly as a card's Start does. */
  const startNew = (node: ResolvedNodeDef) => {
    const way = residency?.serving;
    if (!way) {
      void startNode(node);
      return;
    }
    void guard(
      [ENGINE_OF[way.kind]],
      intl.formatMessage(i18n.startAction, { node: node.def.name }),
      () => void startNode(node)
    );
  };

  const onAction = (node: ResolvedNodeDef, action: NodeCardAction) => {
    const def = node.def;
    switch (action.kind) {
      case 'start': {
        const way = residency?.serving;
        const stops = servingNode?.def.name ?? null;
        if (!way || !stops) {
          void startNode(node);
          return;
        }
        void guard(
          [ENGINE_OF[way.kind]],
          intl.formatMessage(i18n.startAction, { node: def.name }),
          () => void startNode(node),
          {
            title: intl.formatMessage(i18n.startTitle, { node: def.name }),
            message: intl.formatMessage(i18n.startStops, { names: stops }),
            cancel: intl.formatMessage(i18n.keepRunning, { names: stops }),
          }
        );
        return;
      }
      case 'stop': {
        const way = residency?.serving;
        if (!way) return;
        void guard(
          [ENGINE_OF[way.kind]],
          intl.formatMessage(i18n.stopAction, { node: def.name }),
          () => void stopServing(node, way.kind)
        );
        return;
      }
      case 'openRunIt':
        navigate(mlxHref('engine'));
        return;
      case 'setUp':
        navigate(cloudHref());
        return;
      case 'pinWay': {
        const way = residency?.serving;
        // The model a follows node serves NOW is the one to pin (its device names an alias).
        setDialog({
          kind: 'pin',
          model: way && way.kind !== 'remoteSingle' ? way.modelId : null,
        });
        return;
      }
      case 'edit':
        setDialog({ kind: 'edit', node });
        return;
      case 'editInPool':
        onEditInPool();
        return;
      case 'duplicate': {
        const name = uniqueName(
          def.name,
          nodes.map((n) => n.def.name)
        );
        const id = nodeIdFor(
          name,
          nodes.map((n) => n.def.id)
        );
        void writeDef({ ...def, id, name, origin: 'user' });
        return;
      }
      case 'remove':
        setRemoving({
          node,
          alsoFromStrategies: false,
          alsoFromChatNodeSets: false,
          andNewChatsAuto: false,
          acknowledged: null,
          refusals: [],
          busy: false,
        });
        return;
      case 'keepLoaded':
        void writeDef({ ...def, keepLoaded: action.value });
        return;
      case 'openStrategy':
        navigate(`${nodesHref('strategies')}&strategy=${encodeURIComponent(action.id)}`);
        return;
    }
  };

  const card = (node: ResolvedNodeDef) => (
    <div
      key={node.def.id}
      ref={node.def.id === highlight ? highlighted : undefined}
      className="min-w-0"
    >
      <NodeCard
        node={node}
        glance={glanceOf(node)}
        usedBy={read ? usedByOf(read.config, node.def.id) : []}
        highlighted={node.def.id === highlight}
        busy={busy === node.def.id}
        notice={notices[node.def.id] ?? null}
        onAction={(action) => onAction(node, action)}
      />
    </div>
  );

  const grid = 'grid grid-cols-1 gap-3 min-[1000px]:grid-cols-2 min-[1400px]:grid-cols-3';
  const groupHeader = (title: string, link: string, onClick: () => void) => (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <span className={TYPE.zone}>{title}</span>
      <Button variant="ghost" size="sm" icon={<ArrowRight />} onClick={onClick}>
        {link}
      </Button>
    </div>
  );

  return (
    <div className="flex flex-col gap-5" data-testid="nodes-cards">
      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button
          variant="primary"
          icon={<Plus />}
          onClick={() => setDialog({ kind: 'new' })}
          data-testid="nodes-new"
        >
          {intl.formatMessage(i18n.new)}
        </Button>
      </div>

      {store.kind === 'failed' && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.readFailed)}
          text={store.error}
          testId="nodes-read-failed"
          action={
            <Button variant="secondary" size="sm" icon={<RefreshCw />} onClick={refreshGlanceNodes}>
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      )}
      {store.kind === 'unread' && (
        <p className={TYPE.bodyMuted} data-testid="nodes-reading">
          {intl.formatMessage(i18n.reading)}
        </p>
      )}
      {read?.swarmError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.swarmUnreadable)}
          text={read.swarmError}
          testId="nodes-swarm-error"
        />
      )}

      {read && nodes.length === 0 && (
        <div className={cx('py-6', SURFACE.card)} data-testid="nodes-empty">
          <EmptyState
            icon={<Server />}
            title={intl.formatMessage(i18n.emptyTitle)}
            body={intl.formatMessage(i18n.emptyBody)}
            action={
              <div className="flex flex-wrap justify-center gap-2">
                <Button
                  variant="secondary"
                  icon={<Laptop />}
                  onClick={() => navigate(mlxHref('macs'))}
                >
                  {intl.formatMessage(i18n.setUpMacs)}
                </Button>
                <Button
                  variant="primary"
                  icon={<Plus />}
                  onClick={() => setDialog({ kind: 'new' })}
                >
                  {intl.formatMessage(i18n.new)}
                </Button>
              </div>
            }
          />
        </div>
      )}

      {mlxNodes.length > 0 && (
        <section className="flex flex-col gap-3" data-testid="nodes-group-macs">
          {groupHeader(
            intl.formatMessage(i18n.groupMacs),
            intl.formatMessage(i18n.manageMacs),
            () => navigate(mlxHref('macs'))
          )}
          <div className={grid}>{mlxNodes.map(card)}</div>
          {macCount === 1 && (
            <button
              type="button"
              onClick={() => navigate(mlxHref('macs'))}
              className={cx(
                'inline-flex items-center gap-1.5 self-start text-lz-body hover:underline',
                WEIGHT.semibold,
                TONE_TEXT.accent,
                RADIUS.control
              )}
              data-testid="nodes-one-mac"
            >
              {intl.formatMessage(i18n.oneMacHint)}
              <ArrowRight className="size-4" />
            </button>
          )}
        </section>
      )}

      {cloudNodes.length > 0 && (
        <section className="flex flex-col gap-3" data-testid="nodes-group-cloud">
          {groupHeader(
            intl.formatMessage(i18n.groupCloud),
            intl.formatMessage(i18n.manageCloud),
            () => navigate(cloudHref())
          )}
          <div className={grid}>{cloudNodes.map(card)}</div>
        </section>
      )}

      {declinedCount > 0 && (
        <div className="flex flex-wrap items-center gap-2" data-testid="nodes-removed-pool">
          <span className={TYPE.meta}>
            {intl.formatMessage(i18n.removedPoolNodes, { count: declinedCount })}
          </span>
          <Button
            variant="secondary"
            size="sm"
            icon={<Eye />}
            disabled={busy === RESTORE_BUSY}
            onClick={() => void showRemovedPoolNodes()}
            data-testid="nodes-show-removed-pool"
          >
            {intl.formatMessage(i18n.showRemovedPoolNodes)}
          </Button>
          {restoreNotice && (
            <span
              className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
              data-testid="nodes-show-removed-pool-failed"
            >
              {restoreNotice}
            </span>
          )}
        </div>
      )}
      {read && read.lmStudioHidden > 0 && (
        <p className={TYPE.meta} data-testid="nodes-lmstudio-hidden">
          {intl.formatMessage(i18n.lmStudioHidden, { count: read.lmStudioHidden })}
        </p>
      )}
      {(read?.notes ?? []).map((note) => (
        <p key={note} className={TYPE.meta}>
          {note}
        </p>
      ))}

      {dialog && (
        <NewNodeDialog
          open
          start={dialog}
          nodes={nodes}
          serving={residency?.serving ?? null}
          onClose={() => setDialog(null)}
          onSaved={(def, startIt) => {
            refreshGlanceNodes();
            if (startIt) {
              const saved = {
                def,
                model: def.model,
                provider: def.provider,
                modelFrom: { kind: 'own' as const },
              };
              startNew(saved);
            }
          }}
          onOpenCloudProviders={() => navigate(cloudHref())}
          onOpenModels={() => navigate(mlxHref('models'))}
        />
      )}
      {removing && (
        <RemoveDialog
          state={removing}
          config={read?.config ?? null}
          onChange={setRemoving}
          onConfirm={() => void remove(removing)}
          onClose={() => setRemoving(null)}
        />
      )}
      {cutDialog}
    </div>
  );
}

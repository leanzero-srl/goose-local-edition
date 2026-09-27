import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { ArrowRight, Laptop, Plus, RefreshCw, Server } from 'lucide-react';
import type { NodesServingKind } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Checkbox, EmptyState, RADIUS, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { ToneBanner } from '../leanzero-swarm/studio';
import { WithMacs } from '../leanzero-swarm/useMacs';
import { useCutGuard } from '../leanzero-swarm/cutGuard';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { dropRoute } from '../leanzero-swarm/routeSwitch';
import { mlxEngineUnmount } from '../../acp/mlx-engine';
import { mlxDistributedStop } from '../../acp/mlx-distributed';
import { nodesEnsureServing, nodesRemoveNode } from '../../acp/nodes';
import type { MlxEngineKind } from '../../utils/mlxInFlight';
import { cloudHref, mlxHref, nodesHref } from '../../utils/navigationUtils';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import { NodeCard, type NodeCardAction } from './NodeCard';
import { NewNodeDialog, type NewNodeStart } from './NewNodeDialog';
import { usedByOf } from './nodeGlance';
import { useNodeFacts } from './useNodeFacts';
import { nodeIdFor, putNode, uniqueName } from './nodeDraft';
import type { NodeDef, ResolvedNodeDef } from './model';

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
      'Your swarm pool keeps its device; this node is not shown here again. Edit the pool itself below.',
  },
  removeFromStrategies: {
    id: 'nodes.removeFromStrategies',
    defaultMessage: 'Remove it from those strategies too',
  },
  removeAndAuto: {
    id: 'nodes.removeAndAuto',
    defaultMessage: 'Remove, and start new chats on Any node (Auto)',
  },
  removeUsedByChats: {
    id: 'nodes.removeUsedByChats',
    defaultMessage:
      '{count, plural, one {# chat is} other {# chats are}} set to this node. Their next message will say it was removed.',
  },
  removeConfirm: { id: 'nodes.removeConfirm', defaultMessage: 'Remove' },
  cancel: { id: 'nodes.cancel', defaultMessage: 'Cancel' },
  refused: { id: 'nodes.refused', defaultMessage: 'Not removed' },
});

const ENGINE_OF: Record<NodesServingKind, MlxEngineKind> = {
  single: 'single',
  remoteSingle: 'remote',
  split: 'distributed',
};

type Notice = { tone: 'ok' | 'err'; text: string };

interface RemoveState {
  node: ResolvedNodeDef;
  alsoFromStrategies: boolean;
  andNewChatsAuto: boolean;
  acknowledged: number | null;
  refusals: { code: string; message: string; liveSessions?: number | null }[];
  busy: boolean;
}

function RemoveDialog({
  state,
  onChange,
  onConfirm,
  onClose,
}: {
  state: RemoveState;
  onChange: (next: RemoveState) => void;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const intl = useIntl();
  const codes = new Set(state.refusals.map((r) => r.code));
  const live = state.refusals.find((r) => r.code === 'liveSessionsNotAcknowledged');
  // The count rides the refusal as a number (`liveSessions`); words without it offer nothing to
  // acknowledge, never a count guessed from the message.
  const liveCount = live?.liveSessions ?? null;
  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx('flex w-[30rem] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div data-testid="node-remove-dialog" className="flex flex-col gap-2">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>
            {intl.formatMessage(i18n.removeTitle, { node: state.node.def.name })}
          </h2>
        </OverlayDialogTitle>
        <p className={TYPE.body}>
          {intl.formatMessage(state.node.def.poolDevice ? i18n.removePoolBody : i18n.removeBody)}
        </p>
      </div>
      {state.refusals.length > 0 && (
        <div role="alert" className="flex flex-col gap-2" data-testid="node-remove-refusals">
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.refused)}
          </span>
          {state.refusals.map((r) => (
            <p key={r.code + r.message} className={cx('break-words', TYPE.body)}>
              {r.message}
            </p>
          ))}
          {codes.has('nodeInUse') && (
            <Checkbox
              checked={state.alsoFromStrategies}
              onChange={(v) => onChange({ ...state, alsoFromStrategies: v })}
              label={intl.formatMessage(i18n.removeFromStrategies)}
              testId="node-remove-from-strategies"
            />
          )}
          {codes.has('nodeIsForNewChats') && (
            <Checkbox
              checked={state.andNewChatsAuto}
              onChange={(v) => onChange({ ...state, andNewChatsAuto: v })}
              label={intl.formatMessage(i18n.removeAndAuto)}
              testId="node-remove-and-auto"
            />
          )}
          {liveCount != null && (
            <Checkbox
              checked={state.acknowledged === liveCount}
              onChange={(v) => onChange({ ...state, acknowledged: v ? liveCount : null })}
              label={intl.formatMessage(i18n.removeUsedByChats, { count: liveCount })}
              testId="node-remove-acknowledge"
            />
          )}
        </div>
      )}
      <div className="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" onClick={onClose}>
          {intl.formatMessage(i18n.cancel)}
        </Button>
        <Button
          variant="destructive"
          disabled={state.busy}
          onClick={onConfirm}
          data-testid="node-remove-confirm"
        >
          {intl.formatMessage(i18n.removeConfirm)}
        </Button>
      </div>
    </OverlayDialog>
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
              void startNode(saved);
            }
          }}
          onOpenCloudProviders={() => navigate(cloudHref())}
          onOpenModels={() => navigate(mlxHref('models'))}
        />
      )}
      {removing && (
        <RemoveDialog
          state={removing}
          onChange={setRemoving}
          onConfirm={() => void remove(removing)}
          onClose={() => setRemoving(null)}
        />
      )}
      {cutDialog}
    </div>
  );
}

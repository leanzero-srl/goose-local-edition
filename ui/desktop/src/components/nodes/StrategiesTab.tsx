import { useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Plus, RefreshCw, Route as RouteIcon } from 'lucide-react';
import type { BuildEligibility } from '../../acp/nodes';
import { nodesRemoveStrategy, nodesWrite } from '../../acp/nodes';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Checkbox, EmptyState, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { ToneBanner } from '../leanzero-swarm/studio';
import { WithMacs } from '../leanzero-swarm/useMacs';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import type { NodeStrategy, NodesConfig, ResolvedNodeDef } from './model';
import { measuredStart, type Read } from './nodeGlance';
import { nodeIdFor, uniqueName } from './nodeDraft';
import { strategyFit, type MeasuredLoad } from './strategyFit';
import { StrategyCard, type StrategyCardAction } from './StrategyCard';
import { StrategyEditor, newStrategyDraft } from './StrategyEditor';
import { useNodeFacts } from './useNodeFacts';

/**
 * THE STRATEGIES TAB (DESIGN-NODES-AND-STRATEGIES.md §8.4): every strategy as a card, New strategy,
 * and the editor. `&strategy=<id>` in the URL opens that strategy's editor (a card's "Used by" chip
 * and the Nodes tab link here); closing the editor takes it out of the URL in place, so Back does
 * not reopen it. An id no strategy carries is said, never silently ignored.
 *
 * Every change — save, use for new chats, use for swarm builds, duplicate, remove — goes through
 * the one door (`nodes/write`, `nodes/removeStrategy`) with its refusals shown verbatim.
 */

const i18n = defineMessages({
  subtitle: {
    id: 'strategies.subtitle',
    defaultMessage:
      'A strategy says which node does what: its roles, the order to try nodes in, and when to use the next one.',
  },
  new: { id: 'strategies.new', defaultMessage: 'New strategy' },
  defaultName: { id: 'strategies.defaultName', defaultMessage: 'New strategy' },
  emptyTitle: { id: 'strategies.emptyTitle', defaultMessage: 'No strategies yet' },
  emptyBody: {
    id: 'strategies.emptyBody',
    defaultMessage:
      'Make one to say which node chats use first, which node takes over when it can’t run, and which nodes swarm builds use.',
  },
  noSuchStrategy: {
    id: 'strategies.noSuchStrategy',
    defaultMessage: 'There is no strategy “{id}”. It may have been removed.',
  },
  linkLabel: { id: 'strategies.linkLabel', defaultMessage: 'Link' },
  dismiss: { id: 'strategies.dismiss', defaultMessage: 'Close' },
  readFailed: { id: 'strategies.readFailed', defaultMessage: 'Your strategies could not be read' },
  reading: { id: 'strategies.reading', defaultMessage: 'Reading your strategies…' },
  retry: { id: 'strategies.retry', defaultMessage: 'Read again' },
  saved: { id: 'strategies.saved', defaultMessage: 'Saved.' },
  failed: { id: 'strategies.actionFailed', defaultMessage: 'The change could not be saved' },
  removeTitle: { id: 'strategies.removeTitle', defaultMessage: 'Remove {name}?' },
  removeBody: {
    id: 'strategies.removeBody',
    defaultMessage: 'The strategy is removed. Its nodes stay as they are.',
  },
  removeAndAuto: {
    id: 'strategies.removeAndAuto',
    defaultMessage: 'Remove, and start new chats on Any node (Auto)',
  },
  removeAndPool: {
    id: 'strategies.removeAndPool',
    defaultMessage: 'Remove, and let swarm builds use your swarm pool',
  },
  removeConfirm: { id: 'strategies.removeConfirm', defaultMessage: 'Remove' },
  cancel: { id: 'strategies.cancel', defaultMessage: 'Cancel' },
  notRemoved: { id: 'strategies.notRemoved', defaultMessage: 'Not removed' },
});

type Editing = { kind: 'new'; initial: NodeStrategy; seq: number } | { kind: 'stored'; id: string };

interface RemoveState {
  strategy: NodeStrategy;
  andNewChatsAuto: boolean;
  andBuildsPool: boolean;
  refusals: { code: string; message: string }[];
  busy: boolean;
}

export interface StrategiesTabProps {
  eligibility: Record<string, Read<BuildEligibility>>;
}

export function StrategiesTab(props: StrategiesTabProps) {
  return (
    <WithMacs>
      <StrategiesBody {...props} />
    </WithMacs>
  );
}

function StrategiesBody({ eligibility }: StrategiesTabProps) {
  const intl = useIntl();
  const [searchParams, setSearchParams] = useSearchParams();
  const linked = searchParams.get('strategy');
  const { store, nodes, servingNode, loads, macs, glanceOf } = useNodeFacts();
  const read = store.kind === 'read' ? store.read : null;
  const config: NodesConfig | null = read?.config ?? null;
  const strategies = config?.strategies ?? [];

  const [fresh, setFresh] = useState<{ initial: NodeStrategy; seq: number } | null>(null);
  const [saving, setSaving] = useState(false);
  const [refusals, setRefusals] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [notices, setNotices] = useState<Record<string, { tone: 'ok' | 'err'; text: string }>>({});
  const [removing, setRemoving] = useState<RemoveState | null>(null);

  const editing: Editing | null = fresh
    ? { kind: 'new', ...fresh }
    : linked && strategies.some((s) => s.id === linked)
      ? { kind: 'stored', id: linked }
      : null;
  const missingLink = linked != null && read != null && !strategies.some((s) => s.id === linked);

  const measured: MeasuredLoad = (node: ResolvedNodeDef) =>
    measuredStart(node, loads[node.def.id] ?? { kind: 'reading' });

  const highlighted = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (linked) highlighted.current?.scrollIntoView?.({ block: 'center' });
  }, [linked, strategies.length]);

  const clearLink = () =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete('strategy');
        return next;
      },
      { replace: true }
    );
  const openStored = (id: string) =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set('strategy', id);
        return next;
      },
      { replace: true }
    );

  const closeEditor = () => {
    setRefusals([]);
    if (fresh) setFresh(null);
    else clearLink();
  };

  const write = async (next: NodesConfig): Promise<string[] | null> => {
    try {
      const response = await nodesWrite(next);
      return response.written ? null : (response.refusals ?? []).map((r) => r.message);
    } catch (e) {
      return [mlxErrorMessage(e, intl.formatMessage(i18n.failed))];
    } finally {
      refreshGlanceNodes();
    }
  };

  const save = async (draft: NodeStrategy) => {
    if (!config) return;
    setSaving(true);
    const list = config.strategies ?? [];
    const at = list.findIndex((s) => s.id === draft.id);
    const next = at >= 0 ? list.map((s, i) => (i === at ? draft : s)) : [...list, draft];
    const refused = await write({ ...config, strategies: next });
    setSaving(false);
    if (refused) {
      setRefusals(refused);
      return;
    }
    setRefusals([]);
    setNotices((prev) => ({
      ...prev,
      [draft.id]: { tone: 'ok', text: intl.formatMessage(i18n.saved) },
    }));
    closeEditor();
  };

  const act = async (strategy: NodeStrategy, next: NodesConfig) => {
    setBusy(strategy.id);
    const refused = await write(next);
    setBusy(null);
    setNotices((prev) => ({
      ...prev,
      [strategy.id]: refused
        ? { tone: 'err', text: refused.join(' · ') }
        : { tone: 'ok', text: intl.formatMessage(i18n.saved) },
    }));
  };

  const newDraft = () => {
    if (!config) return;
    const name = uniqueName(
      intl.formatMessage(i18n.defaultName),
      strategies.map((s) => s.name)
    );
    const id = nodeIdFor(
      name,
      strategies.map((s) => s.id)
    );
    setRefusals([]);
    setFresh((prev) => ({
      initial: newStrategyDraft(config, nodes, servingNode?.def.id ?? null, id, name),
      seq: (prev?.seq ?? 0) + 1,
    }));
  };

  const onAction = (strategy: NodeStrategy, action: StrategyCardAction) => {
    if (!config) return;
    switch (action.kind) {
      case 'edit':
        setRefusals([]);
        openStored(strategy.id);
        return;
      case 'useForChats':
        void act(strategy, { ...config, forNewChats: { kind: 'strategy', id: strategy.id } });
        return;
      case 'useForBuilds':
        void act(strategy, { ...config, forBuilds: { kind: 'strategy', id: strategy.id } });
        return;
      case 'duplicate': {
        const name = uniqueName(
          strategy.name,
          strategies.map((s) => s.name)
        );
        const id = nodeIdFor(
          name,
          strategies.map((s) => s.id)
        );
        void act(strategy, {
          ...config,
          strategies: [...strategies, { ...strategy, id, name }],
        });
        return;
      }
      case 'remove':
        setRemoving({
          strategy,
          andNewChatsAuto: false,
          andBuildsPool: false,
          refusals: [],
          busy: false,
        });
        return;
    }
  };

  const remove = async (state: RemoveState) => {
    setRemoving({ ...state, busy: true });
    try {
      const response = await nodesRemoveStrategy(state.strategy.id, {
        andNewChatsAuto: state.andNewChatsAuto,
        andBuildsPool: state.andBuildsPool,
      });
      if (response.written) {
        setRemoving(null);
        if (linked === state.strategy.id) clearLink();
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

  const storedEditing =
    editing?.kind === 'stored' ? (strategies.find((s) => s.id === editing.id) ?? null) : null;

  return (
    <div className="flex flex-col gap-5" data-testid="strategies-list">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <p className={cx('max-w-[70ch] break-words', TYPE.bodyMuted)}>
          {intl.formatMessage(i18n.subtitle)}
        </p>
        <Button
          variant="primary"
          icon={<Plus />}
          disabled={!config}
          onClick={newDraft}
          data-testid="strategies-new"
        >
          {intl.formatMessage(i18n.new)}
        </Button>
      </div>

      {store.kind === 'failed' && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.readFailed)}
          text={store.error}
          testId="strategies-read-failed"
          action={
            <Button variant="secondary" size="sm" icon={<RefreshCw />} onClick={refreshGlanceNodes}>
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      )}
      {store.kind === 'unread' && (
        <p className={TYPE.bodyMuted} data-testid="strategies-reading">
          {intl.formatMessage(i18n.reading)}
        </p>
      )}
      {missingLink && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.linkLabel)}
          text={intl.formatMessage(i18n.noSuchStrategy, { id: linked })}
          testId="strategies-missing-link"
          action={
            <Button variant="secondary" size="sm" onClick={clearLink}>
              {intl.formatMessage(i18n.dismiss)}
            </Button>
          }
        />
      )}

      {read && strategies.length === 0 && (
        <div className={cx('py-6', SURFACE.card)} data-testid="strategies-empty">
          <EmptyState
            icon={<RouteIcon />}
            title={intl.formatMessage(i18n.emptyTitle)}
            body={intl.formatMessage(i18n.emptyBody)}
            action={
              <Button variant="primary" icon={<Plus />} onClick={newDraft}>
                {intl.formatMessage(i18n.new)}
              </Button>
            }
          />
        </div>
      )}

      {strategies.length > 0 && config && (
        <div className="grid grid-cols-1 gap-3 min-[1400px]:grid-cols-2">
          {strategies.map((strategy) => (
            <div
              key={strategy.id}
              ref={strategy.id === linked ? highlighted : undefined}
              className="min-w-0"
            >
              <StrategyCard
                strategy={strategy}
                nodes={nodes}
                fit={strategyFit(strategy, nodes, measured)}
                builds={eligibility[strategy.id]}
                macs={macs}
                forNewChats={
                  config.forNewChats?.kind === 'strategy' && config.forNewChats.id === strategy.id
                }
                forBuilds={
                  config.forBuilds?.kind === 'strategy' && config.forBuilds.id === strategy.id
                }
                highlighted={strategy.id === linked}
                busy={busy === strategy.id}
                notice={notices[strategy.id] ?? null}
                onAction={(action) => onAction(strategy, action)}
              />
            </div>
          ))}
        </div>
      )}

      {editing && (editing.kind === 'new' || storedEditing) && (
        <StrategyEditor
          key={editing.kind === 'new' ? `new-${editing.seq}` : `stored-${editing.id}`}
          stored={storedEditing}
          initial={editing.kind === 'new' ? editing.initial : (storedEditing as NodeStrategy)}
          nodes={nodes}
          glanceOf={glanceOf}
          measured={measured}
          macs={macs}
          builds={storedEditing ? eligibility[storedEditing.id] : undefined}
          busy={saving}
          refusals={refusals}
          onSave={(draft) => void save(draft)}
          onClose={closeEditor}
        />
      )}

      {removing && (
        <OverlayDialog
          open
          onClose={() => setRemoving(null)}
          panelClassName={cx('flex w-[30rem] flex-col gap-4 p-5', SURFACE.overlay)}
        >
          <div data-testid="strategy-remove-dialog" className="flex flex-col gap-2">
            <OverlayDialogTitle asChild>
              <h2 className={TYPE.h2}>
                {intl.formatMessage(i18n.removeTitle, { name: removing.strategy.name })}
              </h2>
            </OverlayDialogTitle>
            <p className={TYPE.body}>{intl.formatMessage(i18n.removeBody)}</p>
          </div>
          {removing.refusals.length > 0 && (
            <div
              role="alert"
              className="flex flex-col gap-2"
              data-testid="strategy-remove-refusals"
            >
              <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
                {intl.formatMessage(i18n.notRemoved)}
              </span>
              {removing.refusals.map((r) => (
                <p key={r.code + r.message} className={cx('break-words', TYPE.body)}>
                  {r.message}
                </p>
              ))}
              {removing.refusals.some((r) => r.code === 'strategyIsForNewChats') && (
                <Checkbox
                  checked={removing.andNewChatsAuto}
                  onChange={(v) => setRemoving({ ...removing, andNewChatsAuto: v })}
                  label={intl.formatMessage(i18n.removeAndAuto)}
                  testId="strategy-remove-and-auto"
                />
              )}
              {removing.refusals.some((r) => r.code === 'strategyIsForBuilds') && (
                <Checkbox
                  checked={removing.andBuildsPool}
                  onChange={(v) => setRemoving({ ...removing, andBuildsPool: v })}
                  label={intl.formatMessage(i18n.removeAndPool)}
                  testId="strategy-remove-and-pool"
                />
              )}
            </div>
          )}
          <div className="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              {intl.formatMessage(i18n.cancel)}
            </Button>
            <Button
              variant="destructive"
              disabled={removing.busy}
              onClick={() => void remove(removing)}
              data-testid="strategy-remove-confirm"
            >
              {intl.formatMessage(i18n.removeConfirm)}
            </Button>
          </div>
        </OverlayDialog>
      )}
    </div>
  );
}

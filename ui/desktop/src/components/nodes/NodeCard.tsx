import { useState } from 'react';
import type { IntlShape } from 'react-intl';
import * as Menu from '@radix-ui/react-dropdown-menu';
import {
  Check,
  Copy,
  ExternalLink,
  Loader2,
  MoreHorizontal,
  Pencil,
  Pin,
  Play,
  Settings2,
  Square,
  Trash2,
} from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Disclosure,
  FOCUS,
  MOTION,
  RADIUS,
  ROW,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { figureText, stageWord } from '../engineGlance/EngineGlanceCard';
import { loadPhaseWord } from './loadPhaseWord';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { PERMISSION_LABEL } from '../leanzero-swarm/useMacs';
import { CandidateFigures, outcomeText } from '../leanzero-swarm/PlacementCandidates';
import { KindChip, OtherWayChip, RoleChip, StateChip, WhereChip, ROLE_WORD } from './NodeChips';
import type { GlanceLine, MemoryRow, NodeGlance, UsedBy } from './nodeGlance';
import type { ResolvedNodeDef } from './model';

/**
 * ONE NODE AT A GLANCE (DESIGN-NODES-AND-STRATEGIES.md §8.2): its kind, where it runs, the model,
 * its state in one colour and the line that says why, the figures (the engine's own while it serves,
 * the planner's for its way otherwise), memory against budget per Mac, who uses it, what starting it
 * stops, and the one action that state offers. Everything it says comes from `nodeGlance` — this
 * component only words it. It is never greyed: a node that cannot run keeps full strength and says
 * why in red.
 */

const i18n = defineMessages({
  fromPool: { id: 'nodes.fromPool', defaultMessage: 'from your swarm pool' },
  editInPool: { id: 'nodes.editInPool', defaultMessage: 'Edit in your swarm pool' },
  leftPool: { id: 'nodes.leftPool', defaultMessage: 'No longer in your swarm pool' },
  startsIn: {
    id: 'nodes.startsIn',
    defaultMessage:
      'Starts in about {duration} · median of {count, plural, one {# load} other {# loads}}',
  },
  firstStart: { id: 'nodes.firstStart', defaultMessage: 'First start not measured yet' },
  loadsUnread: { id: 'nodes.loadsUnread', defaultMessage: 'Load times could not be read: {error}' },
  displaces: { id: 'nodes.displaces', defaultMessage: 'Starting it stops {names}' },
  heldByBuildLine: {
    id: 'nodes.heldByBuildLine',
    defaultMessage: 'A swarm build is using {node}; it frees when the build ends',
  },
  follows: {
    id: 'nodes.follows',
    defaultMessage: 'Serves whatever this Mac’s engine runs: {model}',
  },
  followsSplit: {
    id: 'nodes.followsSplit',
    defaultMessage: 'Serves whatever this Mac’s engine runs: {model} · split across {count} Macs',
  },
  followsNothing: {
    id: 'nodes.followsNothing',
    defaultMessage: 'Serves whatever this Mac’s engine runs; nothing runs there now',
  },
  cloudAlways: {
    id: 'nodes.cloudAlways',
    defaultMessage: 'Always available · billed by {provider}',
  },
  endpointAlways: {
    id: 'nodes.endpointAlways',
    defaultMessage: 'Always available · served by {provider}',
  },
  keyMissingLine: {
    id: 'nodes.keyMissingLine',
    defaultMessage: 'Set up {provider} under Providers › Cloud Providers',
  },
  checking: { id: 'nodes.checking', defaultMessage: 'Checking {provider}…' },
  readingState: { id: 'nodes.readingState', defaultMessage: 'Reading what serves now…' },
  planning: { id: 'nodes.planning', defaultMessage: 'Measuring your Macs for this way…' },
  planFailed: { id: 'nodes.planFailed', defaultMessage: 'No plan for this way: {error}' },
  noPlan: { id: 'nodes.noPlan', defaultMessage: 'goose has no plan for this model yet' },
  noSuchWay: {
    id: 'nodes.noSuchWay',
    defaultMessage: 'goose’s plan for this model has no such way',
  },
  notConnected: {
    id: 'nodes.notConnected',
    defaultMessage: '{mac} is not connected to LeanZero Link',
  },
  copyFirst: { id: 'nodes.copyFirst', defaultMessage: 'Copy the model to {mac} first' },
  copyFirstThisMac: {
    id: 'nodes.copyFirstThisMac',
    defaultMessage: 'Get the model on this Mac first',
  },
  permissionOff: {
    id: 'nodes.permissionOff',
    defaultMessage: 'Turn on “{permission}” on {mac} first (LeanZero MLX › My Macs)',
  },
  liveLine: { id: 'nodes.liveLine', defaultMessage: '{stage} · {chat}' },
  loadProgress: { id: 'nodes.loadProgress', defaultMessage: '{phase} · {done} of {total} GB' },
  memNeed: { id: 'nodes.memNeed', defaultMessage: 'needs {need} of {budget} GB' },
  memPeak: { id: 'nodes.memPeak', defaultMessage: '{peak} of {budget} GB' },
  memPeakOnly: { id: 'nodes.memPeakOnly', defaultMessage: 'peak {peak} GB' },
  memNoPeak: { id: 'nodes.memNoPeak', defaultMessage: 'no peak yet' },
  memBar: { id: 'nodes.memBar', defaultMessage: 'Memory on {mac} against its budget' },
  usedBy: { id: 'nodes.usedBy', defaultMessage: 'Used by' },
  roleIn: {
    id: 'nodes.roleIn',
    defaultMessage:
      '{role} {rank, selectordinal, one {#st} two {#nd} few {#rd} other {#th}} · {strategy}',
  },
  actionStart: { id: 'nodes.actionStart', defaultMessage: 'Start' },
  actionStop: { id: 'nodes.actionStop', defaultMessage: 'Stop' },
  actionEdit: { id: 'nodes.actionEdit', defaultMessage: 'Edit' },
  actionDetails: { id: 'nodes.actionDetails', defaultMessage: 'Details' },
  actionSetUp: { id: 'nodes.actionSetUp', defaultMessage: 'Set up' },
  actionOpenRunIt: { id: 'nodes.actionOpenRunIt', defaultMessage: 'Open Run it' },
  pinWay: { id: 'nodes.pinWay', defaultMessage: 'Pin a way' },
  more: { id: 'nodes.more', defaultMessage: 'More for {name}' },
  keepLoaded: { id: 'nodes.keepLoaded', defaultMessage: 'Keep loaded' },
  duplicate: { id: 'nodes.duplicate', defaultMessage: 'Duplicate' },
  showInRunIt: { id: 'nodes.showInRunIt', defaultMessage: 'Show in Run it' },
  remove: { id: 'nodes.remove', defaultMessage: 'Remove' },
});

export type NodeCardAction =
  | { kind: 'start' }
  | { kind: 'stop' }
  | { kind: 'openRunIt' }
  | { kind: 'setUp' }
  | { kind: 'pinWay' }
  | { kind: 'edit' }
  | { kind: 'editInPool' }
  | { kind: 'duplicate' }
  | { kind: 'remove' }
  | { kind: 'keepLoaded'; value: boolean }
  | { kind: 'openStrategy'; id: string };

export interface NodeCardProps {
  node: ResolvedNodeDef;
  glance: NodeGlance;
  usedBy: UsedBy[];
  /** Opened by `#/nodes?tab=nodes&node=<id>`: a solid ring, never a tint. */
  highlighted?: boolean;
  /** An action on this card is in flight. */
  busy?: boolean;
  /** The last action's outcome on this card, in goose's words. */
  notice?: { tone: 'ok' | 'err'; text: string } | null;
  onAction: (action: NodeCardAction) => void;
}

const gb = (intl: IntlShape, value: number) =>
  intl.formatNumber(value, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

/** The line under the name, in the person's words; every fact comes from `nodeGlance`. */
export function lineText(intl: IntlShape, line: GlanceLine): string {
  switch (line.kind) {
    case 'live': {
      const stage = stageWord(intl, line.stage);
      return line.chat ? intl.formatMessage(i18n.liveLine, { stage, chat: line.chat }) : stage;
    }
    case 'loadPhase': {
      const phase = loadPhaseWord(intl, line.phase);
      if (!line.progress) return phase;
      const GIB = 1024 * 1024 * 1024;
      return intl.formatMessage(i18n.loadProgress, {
        phase,
        done: gb(intl, line.progress.done / GIB),
        total: gb(intl, line.progress.total / GIB),
      });
    }
    case 'words':
      return line.text;
    case 'startsIn':
      return intl.formatMessage(i18n.startsIn, {
        duration: formatElapsed(line.medianMs / 1000),
        count: line.count,
      });
    case 'firstStart':
      return intl.formatMessage(i18n.firstStart);
    case 'loadsUnread':
      return intl.formatMessage(i18n.loadsUnread, { error: line.error });
    case 'planning':
      return intl.formatMessage(i18n.planning);
    case 'readingState':
      return intl.formatMessage(i18n.readingState);
    case 'planFailed':
      return intl.formatMessage(i18n.planFailed, { error: line.error });
    case 'noPlan':
      return line.error
        ? intl.formatMessage(i18n.planFailed, { error: line.error })
        : intl.formatMessage(i18n.noPlan);
    case 'noSuchWay':
      return intl.formatMessage(i18n.noSuchWay);
    case 'notConnected':
      return intl.formatMessage(i18n.notConnected, { mac: line.mac });
    case 'copyFirst':
      return line.mac == null
        ? intl.formatMessage(i18n.copyFirstThisMac)
        : intl.formatMessage(i18n.copyFirst, { mac: line.mac });
    case 'permissionOff':
      return intl.formatMessage(i18n.permissionOff, {
        permission: intl.formatMessage(PERMISSION_LABEL[line.permission]),
        mac: line.mac,
      });
    case 'outcome':
      return outcomeText(intl, line.candidate) ?? line.candidate.fit.detail;
    case 'heldByBuild':
      return intl.formatMessage(i18n.heldByBuildLine, { node: line.way });
    case 'follows': {
      const way = line.serving;
      if (!way) return intl.formatMessage(i18n.followsNothing);
      const model = way.modelId.split('/').filter(Boolean).pop() ?? way.modelId;
      return way.kind === 'split'
        ? intl.formatMessage(i18n.followsSplit, { model, count: way.macNames.length })
        : intl.formatMessage(i18n.follows, { model });
    }
    case 'leftPool':
      return intl.formatMessage(i18n.leftPool);
    case 'cloudAlways':
      return intl.formatMessage(line.endpoint ? i18n.endpointAlways : i18n.cloudAlways, {
        provider: line.provider,
      });
    case 'keyMissing':
      return intl.formatMessage(i18n.keyMissingLine, { provider: line.provider });
    case 'checking':
      return intl.formatMessage(i18n.checking, { provider: line.provider });
  }
}

/** The line's colour: red for what cannot run, orange-ink for a step, the page's ink otherwise. */
function lineTone(glance: NodeGlance): string {
  switch (glance.state) {
    case 'cantRun':
    case 'heldByBuild':
    case 'keyMissing':
    case 'failing':
      return cx('text-lz-body', TONE_TEXT.err, WEIGHT.semibold);
    case 'needsStep':
      return cx('text-lz-body', TONE_TEXT.warn, WEIGHT.semibold);
    default:
      return TYPE.body;
  }
}

function MemoryBars({ rows }: { rows: MemoryRow[] }) {
  const intl = useIntl();
  if (rows.length === 0) return null;
  return (
    <div data-testid="node-memory" className="flex flex-col gap-2">
      {rows.map((row) => {
        const over = row.usedGb != null && row.budgetGb != null && row.usedGb > row.budgetGb;
        const text =
          row.usedGb == null
            ? intl.formatMessage(i18n.memNoPeak)
            : row.budgetGb == null
              ? intl.formatMessage(i18n.memPeakOnly, { peak: gb(intl, row.usedGb) })
              : intl.formatMessage(row.kind === 'need' ? i18n.memNeed : i18n.memPeak, {
                  need: gb(intl, row.usedGb),
                  peak: gb(intl, row.usedGb),
                  budget: gb(intl, row.budgetGb),
                });
        const pct =
          row.usedGb != null && row.budgetGb != null && row.budgetGb > 0
            ? Math.round(Math.min(1, row.usedGb / row.budgetGb) * 100)
            : null;
        return (
          <div key={row.mac} className="flex flex-col gap-1" data-testid="node-memory-row">
            <div className="flex min-w-0 items-baseline justify-between gap-2">
              <span className={cx('min-w-0 truncate', TYPE.meta)}>{row.mac}</span>
              <span
                className={cx(
                  'shrink-0 text-lz-meta',
                  TNUM,
                  WEIGHT.semibold,
                  over ? TONE_TEXT.err : 'text-lz-ink'
                )}
              >
                {text}
              </span>
            </div>
            {pct != null && (
              <div
                role="progressbar"
                aria-label={intl.formatMessage(i18n.memBar, { mac: row.mac })}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct}
                className={cx(
                  'h-2 w-full overflow-hidden border border-current',
                  RADIUS.pill,
                  over ? TONE_TEXT.err : 'text-lz-ink'
                )}
              >
                <div className="h-full bg-current" style={{ width: `${pct}%` }} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function LiveFigures({ glance }: { glance: NodeGlance }) {
  const intl = useIntl();
  const figures = glance.figures;
  if (!figures) return null;
  if (figures.kind === 'plan') {
    return <CandidateFigures candidate={figures.candidate} goal={figures.goal} />;
  }
  const pairs = [figures.hero, figures.second].filter((f) => f != null);
  if (pairs.length === 0) return null;
  return (
    <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1" data-testid="node-figures">
      {pairs.map((fig) => {
        const { value, label } = figureText(intl, fig);
        return (
          <span key={fig.kind} className="inline-flex items-baseline gap-1">
            <span className={cx('text-lz-body', WEIGHT.semibold, TNUM, TONE_TEXT.accent)}>
              {value}
            </span>
            <span className={TYPE.meta}>{label}</span>
          </span>
        );
      })}
    </div>
  );
}

const MENU_ITEM = cx(
  'flex w-full cursor-default select-none items-center gap-2 px-2 text-lz-body text-lz-ink outline-none [&_svg]:size-4 [&_svg]:shrink-0',
  ROW.dense,
  RADIUS.control,
  'data-[highlighted]:bg-lz-surface-2'
);

function MoreMenu({
  node,
  onAction,
}: {
  node: ResolvedNodeDef;
  onAction: (action: NodeCardAction) => void;
}) {
  const intl = useIntl();
  const def = node.def;
  const mlxOwn = def.kind === 'mlx' && def.poolDevice == null;
  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button
          variant="ghost"
          size="sm"
          iconOnly
          icon={<MoreHorizontal />}
          aria-label={intl.formatMessage(i18n.more, { name: def.name })}
          data-testid="node-more"
        />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content
          align="end"
          sideOffset={4}
          className={cx('z-50 min-w-[12rem] p-1', SURFACE.overlay)}
          data-testid="node-more-menu"
        >
          {mlxOwn && (
            <Menu.CheckboxItem
              className={MENU_ITEM}
              checked={def.keepLoaded ?? false}
              onCheckedChange={(value) => onAction({ kind: 'keepLoaded', value })}
              data-testid="node-keep-loaded"
            >
              <span
                aria-hidden
                className={cx(
                  'inline-flex size-4 items-center justify-center border-2 border-current',
                  RADIUS.control
                )}
              >
                <Menu.ItemIndicator>
                  <Check className="size-3" />
                </Menu.ItemIndicator>
              </span>
              {intl.formatMessage(i18n.keepLoaded)}
            </Menu.CheckboxItem>
          )}
          {def.poolDevice == null && (
            <Menu.Item className={MENU_ITEM} onSelect={() => onAction({ kind: 'duplicate' })}>
              <Copy />
              {intl.formatMessage(i18n.duplicate)}
            </Menu.Item>
          )}
          {def.kind === 'mlx' && (
            <Menu.Item className={MENU_ITEM} onSelect={() => onAction({ kind: 'openRunIt' })}>
              <ExternalLink />
              {intl.formatMessage(i18n.showInRunIt)}
            </Menu.Item>
          )}
          <Menu.Item
            className={cx(MENU_ITEM, TONE_TEXT.err)}
            onSelect={() => onAction({ kind: 'remove' })}
            data-testid="node-remove"
          >
            <Trash2 />
            {intl.formatMessage(i18n.remove)}
          </Menu.Item>
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}

export function NodeCard({
  node,
  glance,
  usedBy,
  highlighted = false,
  busy = false,
  notice = null,
  onAction,
}: NodeCardProps) {
  const intl = useIntl();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const def = node.def;
  const fromPool = node.modelFrom.kind === 'pool';
  const spinner = <Loader2 className="animate-spin" />;

  const primary = (() => {
    switch (glance.action) {
      case 'start':
        return (
          <Button
            variant="primary"
            size="sm"
            icon={busy ? spinner : <Play />}
            disabled={busy}
            onClick={() => onAction({ kind: 'start' })}
            data-testid="node-start"
          >
            {intl.formatMessage(i18n.actionStart)}
          </Button>
        );
      case 'stop':
        return (
          <Button
            variant="secondary"
            size="sm"
            icon={busy ? spinner : <Square />}
            disabled={busy}
            onClick={() => onAction({ kind: 'stop' })}
            data-testid="node-stop"
          >
            {intl.formatMessage(i18n.actionStop)}
          </Button>
        );
      case 'openRunIt':
        return (
          <Button
            variant="primary"
            size="sm"
            icon={<ExternalLink />}
            onClick={() => onAction({ kind: 'openRunIt' })}
            data-testid="node-open-run-it"
          >
            {intl.formatMessage(i18n.actionOpenRunIt)}
          </Button>
        );
      case 'setUp':
        return (
          <Button
            variant="primary"
            size="sm"
            icon={<Settings2 />}
            onClick={() => onAction({ kind: 'setUp' })}
            data-testid="node-set-up"
          >
            {intl.formatMessage(i18n.actionSetUp)}
          </Button>
        );
      case 'pinWay':
        return (
          <Button
            variant="secondary"
            size="sm"
            icon={<Pin />}
            onClick={() => onAction({ kind: 'pinWay' })}
            data-testid="node-pin-way"
          >
            {intl.formatMessage(i18n.pinWay)}
          </Button>
        );
      case 'details':
        return def.kind === 'mlx' ? null : (
          <Button
            variant="secondary"
            size="sm"
            icon={<Settings2 />}
            onClick={() => onAction({ kind: 'setUp' })}
            data-testid="node-details"
          >
            {intl.formatMessage(i18n.actionDetails)}
          </Button>
        );
      case null:
        return null;
    }
  })();

  return (
    <article
      data-testid="node-card"
      data-node={def.id}
      data-state={glance.state}
      aria-label={def.name}
      className={cx(
        'flex min-w-0 flex-col gap-3 p-4',
        SURFACE.card,
        highlighted && SURFACE.selectedRing
      )}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <KindChip kind={def.kind} />
        {glance.where && <WhereChip where={glance.where} />}
        <span className="ml-auto inline-flex">
          <StateChip state={glance.state} />
        </span>
      </div>

      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <h3 className={cx('min-w-0 break-words', TYPE.h2, WEIGHT.semibold)}>{def.name}</h3>
          {glance.state === 'displaced' && <OtherWayChip />}
        </div>
        {node.model && (
          <p className={cx('min-w-0 break-all', TYPE.mono)} data-testid="node-model">
            {node.model}
            {fromPool && (
              <span className={cx('ml-2 font-sans', TYPE.meta)}>
                {intl.formatMessage(i18n.fromPool)}
              </span>
            )}
          </p>
        )}
        <p className={cx('break-words', lineTone(glance))} data-testid="node-line">
          {lineText(intl, glance.line)}
        </p>
      </div>

      <LiveFigures glance={glance} />
      <MemoryBars rows={glance.memory} />

      {usedBy.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5" data-testid="node-used-by">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.usedBy)}</span>
          {usedBy.map((u) => (
            <button
              key={`${u.strategyId}:${u.role}`}
              type="button"
              onClick={() => onAction({ kind: 'openStrategy', id: u.strategyId })}
              className={cx('inline-flex', RADIUS.control, FOCUS, MOTION, 'hover:underline')}
            >
              <RoleChip role={u.role}>
                {intl.formatMessage(i18n.roleIn, {
                  role: intl.formatMessage(ROLE_WORD[u.role]),
                  rank: u.rank,
                  strategy: u.strategyName,
                })}
              </RoleChip>
            </button>
          ))}
        </div>
      )}

      {glance.displaces && (
        <p
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
          data-testid="node-displaces"
        >
          {intl.formatMessage(i18n.displaces, { names: glance.displaces })}
        </p>
      )}

      {notice && (
        <p
          className={cx(
            'break-words text-lz-meta',
            WEIGHT.semibold,
            notice.tone === 'err' ? TONE_TEXT.err : TONE_TEXT.ok
          )}
          data-testid="node-notice"
        >
          {notice.text}
        </p>
      )}

      {glance.action === 'details' && def.kind === 'mlx' && glance.detail && (
        <Disclosure
          variant="plain"
          title={intl.formatMessage(i18n.actionDetails)}
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
          testId="node-details"
        >
          <p className={cx('whitespace-pre-wrap break-words', TYPE.meta)}>{glance.detail}</p>
        </Disclosure>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {primary}
        {fromPool || node.modelFrom.kind === 'leftPool' ? (
          <Button
            variant="ghost"
            size="sm"
            icon={<Pencil />}
            onClick={() => onAction({ kind: 'editInPool' })}
            data-testid="node-edit-in-pool"
          >
            {intl.formatMessage(i18n.editInPool)}
          </Button>
        ) : (
          <Button
            variant="ghost"
            size="sm"
            icon={<Pencil />}
            onClick={() => onAction({ kind: 'edit' })}
            data-testid="node-edit"
          >
            {intl.formatMessage(i18n.actionEdit)}
          </Button>
        )}
        <span className="ml-auto inline-flex">
          <MoreMenu node={node} onAction={onAction} />
        </span>
      </div>
    </article>
  );
}

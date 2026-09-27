import type { IntlShape } from 'react-intl';
import * as Menu from '@radix-ui/react-dropdown-menu';
import {
  AlertTriangle,
  Copy,
  MessageSquare,
  MoreHorizontal,
  Pencil,
  Trash2,
  Wrench,
} from 'lucide-react';
import type { BuildEligibility } from '../../acp/nodes';
import { defineMessages, useIntl } from '../../i18n';
import { Button, RADIUS, ROW, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { macForPlacementNode, type Mac } from '../leanzero-swarm/macs';
import { RoleChip, ROLE_WORD } from './NodeChips';
import {
  ROLES,
  effectiveRole,
  type NodeRole,
  type NodeRoleEntry,
  type NodeStrategy,
  type ResolvedNodeDef,
} from './model';
import type { Read } from './nodeGlance';
import type { FitWay, StrategyFit } from './strategyFit';

/**
 * One strategy on the Strategies tab (DESIGN-NODES-AND-STRATEGIES.md §8.4): its name and the
 * owner's note, each set role's chain, the roles that follow another ("same as Build"), what it asks
 * of your Macs (strategyFit — one MLX way at a time, so two ways swap), whether swarm builds can use
 * it (goosed's `nodes/buildEligibility`, never guessed) and its actions. The words helpers here are
 * shared with the editor so a card and its editor never say the same fact two ways.
 */

const i18n = defineMessages({
  badgeChats: { id: 'strategies.badgeChats', defaultMessage: 'New chats start here' },
  badgeBuilds: { id: 'strategies.badgeBuilds', defaultMessage: 'Swarm builds use this' },
  sameAs: { id: 'strategies.sameAs', defaultMessage: 'Same as {role}' },
  tagOverflow: { id: 'strategies.tagOverflow', defaultMessage: '{chain} (the next when busy)' },
  tagShare: { id: 'strategies.tagShare', defaultMessage: '{chain} (share {ratio})' },
  onYourMacs: { id: 'strategies.onYourMacs', defaultMessage: 'On your Macs' },
  noWays: {
    id: 'strategies.noWays',
    defaultMessage: 'nothing loads: every node here is in the cloud or follows this Mac’s engine',
  },
  noSwaps: { id: 'strategies.noSwaps', defaultMessage: 'one way ({node}), no swaps' },
  swapsSummary: {
    id: 'strategies.swapsSummary',
    defaultMessage:
      '{count, plural, other {# ways}}, one at a time: each switch stops one and loads the other',
  },
  swapPair: { id: 'strategies.swapPair', defaultMessage: '{a} ⇄ {b}' },
  wayLoad: { id: 'strategies.wayLoad', defaultMessage: '{node} ({load})' },
  loadAbout: {
    id: 'strategies.loadAbout',
    defaultMessage: 'about {duration}, {count, plural, one {# load} other {# loads}} measured',
  },
  loadUnmeasured: { id: 'strategies.loadUnmeasured', defaultMessage: 'load not measured yet' },
  delegateSwaps: {
    id: 'strategies.delegateSwaps',
    defaultMessage: 'Each delegate call swaps twice: to {build} and back to {chat}',
  },
  shareTwoWays: {
    id: 'strategies.shareTwoWays',
    defaultMessage:
      '{role}: sharing between {a} and {b} would stop one to load the other on every turn',
  },
  overflowTwoWays: {
    id: 'strategies.overflowTwoWays',
    defaultMessage:
      '{role}: overflowing from {a} to {b} would stop one to load the other whenever {a} is busy',
  },
  unknownNode: {
    id: 'strategies.unknownNode',
    defaultMessage: '{id} is not a node any more',
  },
  buildsCan: { id: 'strategies.buildsCan', defaultMessage: 'Swarm builds can use this strategy.' },
  buildsCantUse: {
    id: 'strategies.buildsCantUse',
    defaultMessage: 'Swarm builds can’t use this strategy:',
  },
  buildsChecking: {
    id: 'strategies.buildsChecking',
    defaultMessage: 'Checking whether swarm builds can use it…',
  },
  buildsUnread: {
    id: 'strategies.buildsUnread',
    defaultMessage: 'Whether swarm builds can use it could not be read: {error}',
  },
  buildsSplit: {
    id: 'strategies.buildsSplit',
    defaultMessage:
      '{node} is a split; swarm builds reach LeanZero MLX only through this Mac’s single engine',
  },
  buildsRemote: {
    id: 'strategies.buildsRemote',
    defaultMessage:
      '{node} runs on {mac}; swarm builds reach LeanZero MLX only through this Mac’s single engine',
  },
  buildsOtherModel: {
    id: 'strategies.buildsOtherModel',
    defaultMessage:
      'Swarm builds on this Mac’s engine run {model}; choose {node}’s model in Run it first',
  },
  buildsCloudPlanner: {
    id: 'strategies.buildsCloudPlanner',
    defaultMessage:
      'Planning on {node}: the engine replaces a cloud planner with a model LM Studio has loaded',
  },
  buildsLmStudioJoin: {
    id: 'strategies.buildsLmStudioJoin',
    defaultMessage: 'LM Studio models loaded on your fleet also join this build',
  },
  edit: { id: 'strategies.edit', defaultMessage: 'Edit' },
  useForChats: { id: 'strategies.useForChats', defaultMessage: 'Use for new chats' },
  useForBuilds: { id: 'strategies.useForBuilds', defaultMessage: 'Use for swarm builds' },
  more: { id: 'strategies.more', defaultMessage: 'More for {name}' },
  duplicate: { id: 'strategies.duplicate', defaultMessage: 'Duplicate' },
  remove: { id: 'strategies.remove', defaultMessage: 'Remove' },
});

// ---------------------------------------------------------------------------------------------
// Words shared with the editor.
// ---------------------------------------------------------------------------------------------

export function loadWords(intl: IntlShape, way: FitWay): string {
  return way.load
    ? intl.formatMessage(i18n.loadAbout, {
        duration: formatElapsed(way.load.medianMs / 1000),
        count: way.load.count,
      })
    : intl.formatMessage(i18n.loadUnmeasured);
}

export function wayName(way: FitWay): string {
  return way.nodes.map((n) => n.def.name).join(' · ');
}

export function wayWithLoad(intl: IntlShape, way: FitWay): string {
  return intl.formatMessage(i18n.wayLoad, { node: wayName(way), load: loadWords(intl, way) });
}

/** What the strategy asks of your Macs, in one line (the card) — the editor lists the parts. */
export function fitSummary(intl: IntlShape, fit: StrategyFit): string {
  if (fit.ways.length === 0) return intl.formatMessage(i18n.noWays);
  if (fit.ways.length === 1)
    return intl.formatMessage(i18n.noSwaps, { node: wayName(fit.ways[0]) });
  return intl.formatMessage(i18n.swapsSummary, { count: fit.ways.length });
}

export function delegateWords(intl: IntlShape, fit: StrategyFit): string | null {
  return fit.delegate
    ? intl.formatMessage(i18n.delegateSwaps, {
        build: wayName(fit.delegate.build),
        chat: wayName(fit.delegate.chat),
      })
    : null;
}

export function shareTwoWaysWords(intl: IntlShape, fit: StrategyFit): string[] {
  return fit.shareTwoWays.map((s) =>
    intl.formatMessage(s.when === 'share' ? i18n.shareTwoWays : i18n.overflowTwoWays, {
      role: intl.formatMessage(ROLE_WORD[s.role]),
      a: s.a,
      b: s.b,
    })
  );
}

export function unknownNodeWords(intl: IntlShape, fit: StrategyFit): string[] {
  return fit.unknown.map((id) => intl.formatMessage(i18n.unknownNode, { id }));
}

/**
 * A Tier A refusal in the person's words where the design names them; every other reason is
 * goosed's own sentence, verbatim (never paraphrased, never dropped).
 */
export function buildReasonWords(
  intl: IntlShape,
  reason: NonNullable<BuildEligibility['reasons']>[number],
  names: Record<string, string>,
  macs: readonly Mac[]
): string {
  const r = reason.reason;
  const name = (id: string) => names[id] ?? id;
  switch (r.kind) {
    case 'split':
      return intl.formatMessage(i18n.buildsSplit, { node: name(r.node) });
    case 'remote':
      return intl.formatMessage(i18n.buildsRemote, {
        node: name(r.node),
        mac: macForPlacementNode(macs, r.mac)?.name ?? r.mac,
      });
    case 'otherModel':
      return r.model
        ? intl.formatMessage(i18n.buildsOtherModel, { model: r.model, node: name(r.node) })
        : reason.message;
    case 'cloudPlanner':
      return intl.formatMessage(i18n.buildsCloudPlanner, { node: name(r.node) });
    default:
      return reason.message;
  }
}

/** The swarm-builds lines: whether builds can use the STORED strategy, and every reason or note. */
export function BuildsLine({
  answer,
  names,
  macs,
}: {
  answer: Read<BuildEligibility> | undefined;
  names: Record<string, string>;
  macs: readonly Mac[];
}) {
  const intl = useIntl();
  if (!answer || answer.kind === 'reading') {
    return (
      <p className={TYPE.meta} data-testid="strategy-builds" data-builds="checking">
        {intl.formatMessage(i18n.buildsChecking)}
      </p>
    );
  }
  if (answer.kind === 'failed') {
    return (
      <p
        className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
        data-testid="strategy-builds"
        data-builds="failed"
      >
        {intl.formatMessage(i18n.buildsUnread, { error: answer.error })}
      </p>
    );
  }
  if (!answer.value.eligible) {
    return (
      <div className="flex flex-col gap-0.5" data-testid="strategy-builds" data-builds="refused">
        <p className={cx('text-lz-meta', WEIGHT.semibold, 'text-lz-ink-2')}>
          {intl.formatMessage(i18n.buildsCantUse)}
        </p>
        {(answer.value.reasons ?? []).map((r) => (
          <p key={r.message} className={cx('break-words', TYPE.meta)}>
            {buildReasonWords(intl, r, names, macs)}
          </p>
        ))}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-0.5" data-testid="strategy-builds" data-builds="eligible">
      <p className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.ok)}>
        {intl.formatMessage(i18n.buildsCan)}
      </p>
      <p className={cx('break-words', TYPE.meta)}>{intl.formatMessage(i18n.buildsLmStudioJoin)}</p>
      {(answer.value.notes ?? []).map((note) => (
        <p key={note} className={cx('break-words', TYPE.meta)}>
          {note}
        </p>
      ))}
    </div>
  );
}

/** A set role's chain in one line: names in order and, when it is not failover, how it hands on. */
export function chainWords(
  intl: IntlShape,
  entry: NodeRoleEntry,
  names: Record<string, string>
): string {
  const list = entry.chain.map((l) => names[l.node] ?? l.node).join(' → ');
  const when = entry.when ?? 'failover';
  if (when === 'overflow' && entry.chain.length > 1)
    return intl.formatMessage(i18n.tagOverflow, { chain: list });
  if (when === 'share' && entry.chain.length > 1)
    return intl.formatMessage(i18n.tagShare, {
      chain: list,
      ratio: entry.chain.map((l) => l.weight).join(':'),
    });
  return list;
}

export function SameAs({ role, source }: { role: NodeRole; source: NodeRole }) {
  const intl = useIntl();
  return (
    <span
      className="inline-flex items-center gap-1.5"
      data-testid="strategy-same-as"
      data-role={role}
    >
      <RoleChip role={role} />
      <span className={cx(TYPE.body)}>
        {intl.formatMessage(i18n.sameAs, { role: intl.formatMessage(ROLE_WORD[source]) })}
      </span>
    </span>
  );
}

// ---------------------------------------------------------------------------------------------
// The card.
// ---------------------------------------------------------------------------------------------

export type StrategyCardAction =
  | { kind: 'edit' }
  | { kind: 'useForChats' }
  | { kind: 'useForBuilds' }
  | { kind: 'duplicate' }
  | { kind: 'remove' };

export interface StrategyCardProps {
  strategy: NodeStrategy;
  nodes: readonly ResolvedNodeDef[];
  fit: StrategyFit;
  builds: Read<BuildEligibility> | undefined;
  macs: readonly Mac[];
  forNewChats: boolean;
  forBuilds: boolean;
  highlighted?: boolean;
  busy?: boolean;
  notice?: { tone: 'ok' | 'err'; text: string } | null;
  onAction: (action: StrategyCardAction) => void;
}

const MENU_ITEM = cx(
  'flex w-full cursor-default select-none items-center gap-2 px-2 text-lz-body text-lz-ink outline-none [&_svg]:size-4 [&_svg]:shrink-0',
  ROW.dense,
  RADIUS.control,
  'data-[highlighted]:bg-lz-surface-2'
);

const BADGE = cx(
  'inline-flex h-5 shrink-0 items-center whitespace-nowrap px-1.5 text-lz-meta font-lz-semibold',
  RADIUS.control,
  SURFACE.selected
);

export function StrategyCard({
  strategy,
  nodes,
  fit,
  builds,
  macs,
  forNewChats,
  forBuilds,
  highlighted = false,
  busy = false,
  notice = null,
  onAction,
}: StrategyCardProps) {
  const intl = useIntl();
  const names = Object.fromEntries(nodes.map((n) => [n.def.id, n.def.name]));
  const roles = strategy.roles ?? {};
  const setRoles = ROLES.filter((r) => roles[r]);
  const inherited = ROLES.filter((r) => !roles[r]);
  const delegate = delegateWords(intl, fit);
  const warnings = [...shareTwoWaysWords(intl, fit), ...unknownNodeWords(intl, fit)];
  const eligible = builds?.kind === 'read' && builds.value.eligible;

  return (
    <article
      data-testid="strategy-card"
      data-strategy={strategy.id}
      aria-label={strategy.name}
      className={cx(
        'flex min-w-0 flex-col gap-3 p-4',
        SURFACE.card,
        highlighted && 'ring-2 ring-lz-accent'
      )}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <h3 className={cx('min-w-0 break-words', TYPE.h2, WEIGHT.semibold)}>{strategy.name}</h3>
        {forNewChats && (
          <span className={BADGE} data-testid="strategy-badge-chats">
            {intl.formatMessage(i18n.badgeChats)}
          </span>
        )}
        {forBuilds && (
          <span className={BADGE} data-testid="strategy-badge-builds">
            {intl.formatMessage(i18n.badgeBuilds)}
          </span>
        )}
      </div>
      {strategy.note && (
        <p className={cx('break-words italic', TYPE.bodyMuted)} data-testid="strategy-note">
          “{strategy.note}”
        </p>
      )}

      <ul className="flex flex-col gap-1.5" data-testid="strategy-roles">
        {setRoles.map((role) => (
          <li key={role} className="flex min-w-0 flex-wrap items-center gap-2" data-role={role}>
            <RoleChip role={role} />
            <span className={cx('min-w-0 break-words', TYPE.body)}>
              {chainWords(intl, roles[role] as NodeRoleEntry, names)}
            </span>
          </li>
        ))}
        {inherited.length > 0 && (
          <li className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
            {inherited.map((role) => {
              const source = effectiveRole(roles, role);
              return source ? <SameAs key={role} role={role} source={source} /> : null;
            })}
          </li>
        )}
      </ul>

      <p className={cx('break-words', TYPE.body)} data-testid="strategy-fit">
        <span className={WEIGHT.semibold}>{intl.formatMessage(i18n.onYourMacs)}: </span>
        {fitSummary(intl, fit)}
      </p>
      {fit.swaps.map(([a, b]) => (
        <p
          key={`${a.key}|${b.key}`}
          className={cx('break-words', TYPE.meta)}
          data-testid="strategy-swap"
        >
          {intl.formatMessage(i18n.swapPair, { a: wayWithLoad(intl, a), b: wayWithLoad(intl, b) })}
        </p>
      ))}
      {delegate && (
        <p
          className={cx(
            'flex items-start gap-1.5 break-words text-lz-meta',
            WEIGHT.semibold,
            TONE_TEXT.warn
          )}
          data-testid="strategy-delegate-warning"
        >
          <AlertTriangle aria-hidden className="mt-0.5 size-3.5 shrink-0" />
          {delegate}
        </p>
      )}
      {warnings.map((w) => (
        <p
          key={w}
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
          data-testid="strategy-warning"
        >
          {w}
        </p>
      ))}

      <BuildsLine answer={builds} names={names} macs={macs} />

      {notice && (
        <p
          className={cx(
            'break-words text-lz-meta',
            WEIGHT.semibold,
            notice.tone === 'err' ? TONE_TEXT.err : TONE_TEXT.ok
          )}
          data-testid="strategy-notice"
        >
          {notice.text}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="secondary"
          size="sm"
          icon={<Pencil />}
          onClick={() => onAction({ kind: 'edit' })}
          data-testid="strategy-edit"
        >
          {intl.formatMessage(i18n.edit)}
        </Button>
        {!forNewChats && (
          <Button
            variant="ghost"
            size="sm"
            icon={<MessageSquare />}
            disabled={busy}
            onClick={() => onAction({ kind: 'useForChats' })}
            data-testid="strategy-use-chats"
          >
            {intl.formatMessage(i18n.useForChats)}
          </Button>
        )}
        {eligible && !forBuilds && (
          <Button
            variant="ghost"
            size="sm"
            icon={<Wrench />}
            disabled={busy}
            onClick={() => onAction({ kind: 'useForBuilds' })}
            data-testid="strategy-use-builds"
          >
            {intl.formatMessage(i18n.useForBuilds)}
          </Button>
        )}
        <span className="ml-auto inline-flex">
          <Menu.Root>
            <Menu.Trigger asChild>
              <Button
                variant="ghost"
                size="sm"
                iconOnly
                icon={<MoreHorizontal />}
                aria-label={intl.formatMessage(i18n.more, { name: strategy.name })}
                data-testid="strategy-more"
              />
            </Menu.Trigger>
            <Menu.Portal>
              <Menu.Content
                align="end"
                sideOffset={4}
                className={cx('z-50 min-w-[12rem] p-1', SURFACE.overlay)}
              >
                <Menu.Item className={MENU_ITEM} onSelect={() => onAction({ kind: 'duplicate' })}>
                  <Copy />
                  {intl.formatMessage(i18n.duplicate)}
                </Menu.Item>
                <Menu.Item
                  className={cx(MENU_ITEM, TONE_TEXT.err)}
                  onSelect={() => onAction({ kind: 'remove' })}
                  data-testid="strategy-remove"
                >
                  <Trash2 />
                  {intl.formatMessage(i18n.remove)}
                </Menu.Item>
              </Menu.Content>
            </Menu.Portal>
          </Menu.Root>
        </span>
      </div>
    </article>
  );
}

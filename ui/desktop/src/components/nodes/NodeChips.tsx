import type { ReactNode } from 'react';
import { Cloud, Cpu, Laptop, Network, Plug } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { RADIUS, TNUM, cx } from '../lz';
import { KIND_HUE, OUTLINE, ROLE_HUES, STATE_HUES } from './hues';
import { STATE_CHIP, type GlanceWhere, type NodeState } from './nodeGlance';
import type { NodeDefKind, NodeRole } from './model';

/**
 * The node chips, one register each (hues.ts): the KIND chip is solid ink with its icon, the WHERE
 * chip an ink outline, the STATE chip its state's one colour, a ROLE chip its role's hue. S6's
 * strategy editor and pickers use these too, so a state or a role looks the same everywhere.
 */

const i18n = defineMessages({
  kindMlx: { id: 'nodes.kindMlx', defaultMessage: 'MLX' },
  kindCloud: { id: 'nodes.kindCloud', defaultMessage: 'Cloud' },
  kindEndpoint: { id: 'nodes.kindEndpoint', defaultMessage: 'Endpoint' },
  whereThisMac: { id: 'nodes.whereThisMac', defaultMessage: 'This Mac' },
  whereSplit: {
    id: 'nodes.whereSplit',
    defaultMessage: 'Split · {count, plural, one {# Mac} other {# Macs}}',
  },
  stateServing: { id: 'nodes.stateServing', defaultMessage: 'Serving' },
  stateLoading: { id: 'nodes.stateLoading', defaultMessage: 'Loading' },
  stateWaiting: { id: 'nodes.stateWaiting', defaultMessage: 'Waiting to load' },
  stateNotLoaded: { id: 'nodes.stateNotLoaded', defaultMessage: 'Not loaded' },
  stateOtherWay: { id: 'nodes.stateOtherWay', defaultMessage: 'Another way is running' },
  stateNeedsStep: { id: 'nodes.stateNeedsStep', defaultMessage: 'Needs a step' },
  stateCantRun: { id: 'nodes.stateCantRun', defaultMessage: 'Can’t run' },
  stateHeldByBuild: { id: 'nodes.stateHeldByBuild', defaultMessage: 'Held by a build' },
  stateFollows: { id: 'nodes.stateFollows', defaultMessage: 'Follows this Mac' },
  stateUnknown: { id: 'nodes.stateUnknown', defaultMessage: 'State unknown' },
  stateReady: { id: 'nodes.stateReady', defaultMessage: 'Ready' },
  stateKeyMissing: { id: 'nodes.stateKeyMissing', defaultMessage: 'Key missing' },
  stateLastCallFailed: { id: 'nodes.stateLastCallFailed', defaultMessage: 'Last call failed' },
  roleChat: { id: 'strategies.roleChat', defaultMessage: 'Chat' },
  rolePlanning: { id: 'strategies.rolePlanning', defaultMessage: 'Planning' },
  roleBuild: { id: 'strategies.roleBuild', defaultMessage: 'Build' },
  roleTesting: { id: 'strategies.roleTesting', defaultMessage: 'Testing' },
  roleFrontend: { id: 'strategies.roleFrontend', defaultMessage: 'Frontend' },
  roleBackend: { id: 'strategies.roleBackend', defaultMessage: 'Backend' },
});

const STATE_WORD: Record<NodeState, (typeof i18n)['stateServing']> = {
  serving: i18n.stateServing,
  loading: i18n.stateLoading,
  waiting: i18n.stateWaiting,
  ready: i18n.stateNotLoaded,
  displaced: i18n.stateNotLoaded,
  needsStep: i18n.stateNeedsStep,
  cantRun: i18n.stateCantRun,
  heldByBuild: i18n.stateHeldByBuild,
  follows: i18n.stateFollows,
  unknown: i18n.stateUnknown,
  cloudReady: i18n.stateReady,
  keyMissing: i18n.stateKeyMissing,
  failing: i18n.stateLastCallFailed,
};

export const ROLE_WORD: Record<NodeRole, (typeof i18n)['roleChat']> = {
  chat: i18n.roleChat,
  planning: i18n.rolePlanning,
  build: i18n.roleBuild,
  testing: i18n.roleTesting,
  frontend: i18n.roleFrontend,
  backend: i18n.roleBackend,
};

const CHIP =
  'inline-flex h-5 shrink-0 items-center gap-1 whitespace-nowrap px-1.5 text-lz-meta font-lz-semibold [&_svg]:size-3';

function NodeChip({
  className,
  icon,
  children,
  testId,
  data,
}: {
  className: string;
  icon?: ReactNode;
  children: ReactNode;
  testId: string;
  data?: Record<string, string>;
}) {
  return (
    <span
      data-testid={testId}
      {...Object.fromEntries(Object.entries(data ?? {}).map(([k, v]) => [`data-${k}`, v]))}
      className={cx(CHIP, TNUM, RADIUS.control, className)}
    >
      {icon != null && <span aria-hidden>{icon}</span>}
      {children}
    </span>
  );
}

export function KindChip({ kind }: { kind: NodeDefKind }) {
  const intl = useIntl();
  const [icon, word] =
    kind === 'mlx'
      ? [<Cpu key="i" />, i18n.kindMlx]
      : kind === 'cloud'
        ? [<Cloud key="i" />, i18n.kindCloud]
        : [<Plug key="i" />, i18n.kindEndpoint];
  return (
    <NodeChip className={KIND_HUE.className} icon={icon} testId="node-kind" data={{ kind }}>
      {intl.formatMessage(word)}
    </NodeChip>
  );
}

export function WhereChip({ where }: { where: GlanceWhere }) {
  const intl = useIntl();
  const icon =
    where.kind === 'split' ? <Network /> : where.kind === 'provider' ? <Cloud /> : <Laptop />;
  const word =
    where.kind === 'thisMac'
      ? intl.formatMessage(i18n.whereThisMac)
      : where.kind === 'split'
        ? intl.formatMessage(i18n.whereSplit, { count: where.count })
        : where.name;
  return (
    <NodeChip className={OUTLINE.ink} icon={icon} testId="node-where">
      {word}
    </NodeChip>
  );
}

export function StateChip({ state }: { state: NodeState }) {
  const intl = useIntl();
  const look = STATE_CHIP[state];
  const className = 'hue' in look ? STATE_HUES[look.hue].className : OUTLINE[look.outline];
  return (
    <NodeChip className={className} testId="node-state" data={{ state }}>
      {intl.formatMessage(STATE_WORD[state])}
    </NodeChip>
  );
}

/** "Another way is running": the slate outline beside a displaced node's name. */
export function OtherWayChip() {
  const intl = useIntl();
  return (
    <NodeChip className={OUTLINE.slate} testId="node-other-way">
      {intl.formatMessage(i18n.stateOtherWay)}
    </NodeChip>
  );
}

export function RoleChip({ role, children }: { role: NodeRole; children?: ReactNode }) {
  const intl = useIntl();
  return (
    <NodeChip className={ROLE_HUES[role].className} testId="node-role" data={{ role }}>
      {children ?? intl.formatMessage(ROLE_WORD[role])}
    </NodeChip>
  );
}

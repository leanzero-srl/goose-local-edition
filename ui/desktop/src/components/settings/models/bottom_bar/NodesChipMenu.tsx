import type { ReactNode } from 'react';
import { Check, Cpu, Network, Sliders } from 'lucide-react';
import type {
  NodeResidency,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
} from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../../../i18n';
import {
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from '../../../ui/dropdown-menu';
import { TYPE, cx } from '../../../lz';
import { StateChip } from '../../../nodes/NodeChips';
import { effectiveEntry, nodeNamesById } from '../../../nodes/model';
import type { NodeState } from '../../../nodes/nodeGlance';
import { formatElapsed } from '../../../leanzero-swarm/mlxLiveStats';
import { measuredLoadOf } from '../../../../utils/nodeSwap';

const i18n = defineMessages({
  title: { id: 'nodesChipMenu.title', defaultMessage: 'Run this chat on' },
  strategies: { id: 'nodesChipMenu.strategies', defaultMessage: 'Strategies' },
  nodes: { id: 'nodesChipMenu.nodes', defaultMessage: 'Nodes' },
  chatOn: { id: 'nodesChipMenu.chatOn', defaultMessage: 'Chat → {node}' },
  chatOnNone: { id: 'nodesChipMenu.chatOnNone', defaultMessage: 'Chat → no node set' },
  startsIn: { id: 'nodesChipMenu.startsIn', defaultMessage: 'starts in about {duration}' },
  auto: { id: 'nodes.auto', defaultMessage: 'Any node (Auto)' },
  manageNodes: { id: 'nodesChipMenu.manageNodes', defaultMessage: 'Manage nodes…' },
  engine: { id: 'nodesChipMenu.engine', defaultMessage: 'LeanZero MLX engine…' },
  otherModels: { id: 'nodesChipMenu.otherModels', defaultMessage: 'Other models and providers…' },
  otherModelsHint: {
    id: 'nodesChipMenu.otherModelsHint',
    defaultMessage: 'Cloud models and your endpoints',
  },
});

/**
 * A node's residency as the chip menu's state chip (§8.5: its states come from `nodes/residency`,
 * read when the menu opens) — the Nodes page's own chips and words (NodeChips' StateChip).
 */
export function residencyChipState(residency: NodeResidency | undefined): NodeState {
  switch (residency?.kind) {
    case 'serving':
      return 'serving';
    case 'loading':
      return 'loading';
    case 'waiting':
      return 'waiting';
    case 'notRunning':
      return 'ready';
    case 'alwaysReady':
      return 'cloudReady';
    case 'refusedLastTime':
      return residency.facts?.kind === 'heldByBuild' ? 'heldByBuild' : 'cantRun';
    case 'unknown':
    case undefined:
      return 'unknown';
  }
}

/** The base of a session's route model (`strategy:<id>@<role>` is its strategy's). */
function routeBase(model: string | null | undefined): string | null {
  if (!model) return null;
  return model.startsWith('strategy:') ? model.split('@')[0] : model;
}

/**
 * §8.5's chip menu: "Run this chat on" — the strategies, each with the node its Chat role goes to
 * and that node's state; the nodes, each with its state and, when it is not loaded and a load is
 * measured, how long it takes to start; and Any node (Auto). Picking one sets THIS session's model
 * (`onPick`); the current one is checked. The foot opens the Nodes page, the engine and the other
 * models and providers.
 */
export function NodesChipMenu({
  read,
  residency,
  currentModel,
  onPick,
  onManageNodes,
  onEngine,
  onOtherModels,
}: {
  read: NodesReadResponse_unstable;
  residency: NodesResidencyResponse_unstable;
  /** The session's model when it rides the swarm provider; null otherwise (nothing is checked). */
  currentModel: string | null;
  onPick: (model: string, label: string) => void;
  onManageNodes: () => void;
  onEngine: () => void;
  onOtherModels: () => void;
}) {
  const intl = useIntl();
  const names = nodeNamesById(read.nodes);
  const stateOf = (id: string) =>
    residencyChipState(residency.nodes.find((r) => r.node === id)?.residency);
  const current = routeBase(currentModel);
  const strategies = read.config.strategies ?? [];

  const row = (
    model: string,
    label: string,
    testId: string,
    extra: { note?: string | null; state?: NodeState | null; icon?: ReactNode }
  ) => (
    <DropdownMenuItem
      key={model}
      data-testid={testId}
      data-model={model}
      data-current={current === model ? 'yes' : undefined}
      onClick={() => onPick(model, label)}
    >
      <span className="flex w-4 shrink-0 justify-center" aria-hidden>
        {current === model ? <Check className="h-4 w-4" /> : extra.icon}
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate" title={label}>
          {label}
        </span>
        {extra.note && <span className={cx(TYPE.meta, 'truncate')}>{extra.note}</span>}
      </span>
      {extra.state && <StateChip state={extra.state} />}
    </DropdownMenuItem>
  );

  return (
    <div data-testid="nodes-chip-menu">
      <DropdownMenuLabel className={TYPE.meta}>{intl.formatMessage(i18n.title)}</DropdownMenuLabel>
      {strategies.length > 0 && (
        <>
          <p className={cx('mx-2 mt-1 uppercase tracking-wide', TYPE.meta)}>
            {intl.formatMessage(i18n.strategies)}
          </p>
          {strategies.map((s) => {
            const chatNode = effectiveEntry(s, 'chat')?.chain[0]?.node ?? null;
            return row(`strategy:${s.id}`, s.name, `nodes-chip-strategy-${s.id}`, {
              note:
                chatNode != null
                  ? intl.formatMessage(i18n.chatOn, { node: names[chatNode] ?? chatNode })
                  : intl.formatMessage(i18n.chatOnNone),
              state: chatNode != null ? stateOf(chatNode) : null,
            });
          })}
        </>
      )}
      <p className={cx('mx-2 mt-1 uppercase tracking-wide', TYPE.meta)}>
        {intl.formatMessage(i18n.nodes)}
      </p>
      {read.nodes.map((node) => {
        const state = stateOf(node.def.id);
        const load = measuredLoadOf(residency, node.def.id);
        const note =
          node.def.kind === 'mlx' && state !== 'serving' && load
            ? intl.formatMessage(i18n.startsIn, { duration: formatElapsed(load.medianMs / 1000) })
            : null;
        return row(`node:${node.def.id}`, node.def.name, `nodes-chip-node-${node.def.id}`, {
          note,
          state,
        });
      })}
      {row('swarm', intl.formatMessage(i18n.auto), 'nodes-chip-auto', {
        icon: <Network className="h-4 w-4" />,
      })}
      <DropdownMenuSeparator />
      <div className="grid grid-cols-2">
        <DropdownMenuItem data-testid="nodes-chip-manage" onClick={onManageNodes}>
          <span>{intl.formatMessage(i18n.manageNodes)}</span>
        </DropdownMenuItem>
        <DropdownMenuItem data-testid="nodes-chip-engine" onClick={onEngine}>
          <span>{intl.formatMessage(i18n.engine)}</span>
          <Cpu className="ml-auto h-4 w-4 shrink-0" />
        </DropdownMenuItem>
      </div>
      <DropdownMenuItem data-testid="nodes-chip-other" onClick={onOtherModels}>
        <span className="flex min-w-0 flex-col">
          <span>{intl.formatMessage(i18n.otherModels)}</span>
          <span className={TYPE.meta}>{intl.formatMessage(i18n.otherModelsHint)}</span>
        </span>
        <Sliders className="ml-auto h-4 w-4 shrink-0 rotate-90" />
      </DropdownMenuItem>
    </div>
  );
}

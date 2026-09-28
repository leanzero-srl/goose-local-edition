import type { ReactNode } from 'react';
import { Check, Cpu, Network, Plus, Save, Sliders, X } from 'lucide-react';
import type {
  NodeResidency,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
} from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../../../i18n';
import {
  DropdownMenuCheckboxItem,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from '../../../ui/dropdown-menu';
import { RADIUS, SURFACE, TYPE, WEIGHT, cx } from '../../../lz';
import { StateChip } from '../../../nodes/NodeChips';
import { effectiveEntry, namedStrategies, nodeNamesById } from '../../../nodes/model';
import { chatNodeIds, type ChatNodesNow } from '../../../nodes/chatNodeAvailability';
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
  chatNodes: { id: 'nodesChipMenu.chatNodes', defaultMessage: 'This chat’s nodes' },
  answers: { id: 'nodesChipMenu.answers', defaultMessage: 'answers' },
  takeOut: { id: 'nodesChipMenu.takeOut', defaultMessage: 'Take {node} out of this chat' },
  setLine: {
    id: 'nodesChipMenu.setLine',
    defaultMessage: 'Delegates share these nodes. The chat answers on {lead}.',
  },
  oneLine: {
    id: 'nodesChipMenu.oneLine',
    defaultMessage: 'The chat and its delegates run on {lead}.',
  },
  strategyLine: {
    id: 'nodesChipMenu.strategyLine',
    defaultMessage:
      'On {strategy}: the chat answers on {lead}. Adding a node gives this chat its own nodes; {strategy} stays as it is.',
  },
  noneLine: {
    id: 'nodesChipMenu.noneLine',
    defaultMessage: 'No node answers this chat by name. The first node you add answers it.',
  },
  answerOnNext: {
    id: 'nodesChipMenu.answerOnNext',
    defaultMessage: 'If {lead} can’t run, answer on the next node',
  },
  answerOnNextWhy: {
    id: 'nodesChipMenu.answerOnNextWhy',
    defaultMessage: 'Off: when {lead} can’t run, the turn ends and says why.',
  },
  addNode: { id: 'nodesChipMenu.addNode', defaultMessage: 'Add a node to this chat…' },
  saveAsStrategy: { id: 'nodesChipMenu.saveAsStrategy', defaultMessage: 'Save as a strategy…' },
});

/** The chip's door to this chat's own nodes (Q-359). */
export interface ChatNodesControl {
  now: ChatNodesNow;
  /** Store the chat's set (`nodes/setChatNodes`): lead first. */
  onSetNodes: (nodes: string[], answerOnNext: boolean) => void;
  onAddNode: () => void;
  onSaveAsStrategy: (strategyId: string) => void;
}

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

/**
 * "THIS CHAT'S NODES" (Q-359, DESIGN-Q359-CHAT-NODES.md "Control"): the chat's nodes as solid chips —
 * the one that answers marked, every other with its × — the line that says who answers and who the
 * delegates share, the failover switch (off by default), and the two doors: add a node, save the set
 * as a strategy. A chip's state is the menu's own (`nodes/residency`, read when it opened).
 */
function ChatNodesSection({
  control,
  names,
  stateOf,
}: {
  control: ChatNodesControl;
  names: Record<string, string>;
  stateOf: (id: string) => NodeState;
}) {
  const intl = useIntl();
  const { now } = control;
  const ids = chatNodeIds(now);
  const nameOf = (id: string) => names[id] ?? id;
  const lead = ids[0] != null ? nameOf(ids[0]) : null;
  const line =
    now.kind === 'strategy' && lead
      ? intl.formatMessage(i18n.strategyLine, { strategy: now.strategy.name, lead })
      : lead && ids.length > 1
        ? intl.formatMessage(i18n.setLine, { lead })
        : lead
          ? intl.formatMessage(i18n.oneLine, { lead })
          : intl.formatMessage(i18n.noneLine);
  return (
    <div className="mx-2 mb-1 flex flex-col gap-1.5" data-testid="chat-nodes-section">
      <p className={cx('mt-1 uppercase tracking-wide', TYPE.meta)}>
        {intl.formatMessage(i18n.chatNodes)}
      </p>
      {ids.length > 0 && (
        <div className="flex flex-wrap gap-1.5" data-testid="chat-nodes-chips">
          {ids.map((id, index) => (
            <span
              key={id}
              data-testid={`chat-node-${id}`}
              data-lead={index === 0 ? 'yes' : undefined}
              className={cx(
                'inline-flex max-w-full items-center gap-1.5 py-0.5 pl-2 pr-1',
                RADIUS.control,
                SURFACE.outline,
                'bg-lz-surface'
              )}
            >
              <span className={cx('truncate text-lz-meta text-lz-ink', WEIGHT.semibold)}>
                {nameOf(id)}
              </span>
              <StateChip state={stateOf(id)} />
              {index === 0 && (
                <span
                  className={cx(
                    'inline-flex h-5 items-center px-1.5 text-lz-meta',
                    WEIGHT.semibold,
                    RADIUS.control,
                    'bg-lz-accent text-lz-accent-ink'
                  )}
                  data-testid="chat-node-answers"
                >
                  {intl.formatMessage(i18n.answers)}
                </span>
              )}
              {index > 0 && now.kind === 'set' && (
                // A menu item (Q-380): the menu's arrow keys reach it and Enter/Space take the node
                // out. The menu stays open, so the chip leaving is seen.
                <DropdownMenuItem
                  className={cx(
                    'size-5 justify-center p-0 text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink focus:bg-lz-ink focus:text-lz-surface [&_svg]:size-3.5',
                    RADIUS.control
                  )}
                  aria-label={intl.formatMessage(i18n.takeOut, { node: nameOf(id) })}
                  data-testid={`chat-node-remove-${id}`}
                  onSelect={(e) => {
                    e.preventDefault();
                    control.onSetNodes(
                      ids.filter((other) => other !== id),
                      now.answerOnNext
                    );
                  }}
                >
                  <X />
                </DropdownMenuItem>
              )}
            </span>
          ))}
        </div>
      )}
      <p className={cx('break-words', TYPE.meta)} data-testid="chat-nodes-line">
        {line}
      </p>
      {now.kind === 'set' && ids.length > 1 && lead && (
        // A menu checkbox item (Q-380): reached by the arrow keys, toggled by Enter/Space, and the
        // menu stays open so the switch is seen to flip.
        <DropdownMenuCheckboxItem
          checked={now.answerOnNext}
          onCheckedChange={(next) => control.onSetNodes(ids, next)}
          onSelect={(e) => e.preventDefault()}
          data-testid="chat-nodes-answer-on-next"
        >
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className={cx('break-words text-lz-body text-lz-ink', WEIGHT.medium)}>
              {intl.formatMessage(i18n.answerOnNext, { lead })}
            </span>
            <span className={cx('break-words', TYPE.meta)}>
              {intl.formatMessage(i18n.answerOnNextWhy, { lead })}
            </span>
          </span>
        </DropdownMenuCheckboxItem>
      )}
      <div className="flex flex-wrap">
        <DropdownMenuItem data-testid="chat-nodes-add" onClick={control.onAddNode}>
          <Plus className="h-4 w-4 shrink-0" />
          <span>{intl.formatMessage(i18n.addNode)}</span>
        </DropdownMenuItem>
        {now.kind === 'set' && (
          <DropdownMenuItem
            data-testid="chat-nodes-save"
            onClick={() => control.onSaveAsStrategy(now.strategyId)}
          >
            <Save className="h-4 w-4 shrink-0" />
            <span>{intl.formatMessage(i18n.saveAsStrategy)}</span>
          </DropdownMenuItem>
        )}
      </div>
      <DropdownMenuSeparator />
    </div>
  );
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
  chatNodes = null,
}: {
  read: NodesReadResponse_unstable;
  residency: NodesResidencyResponse_unstable;
  /** The session's model when it rides the swarm provider; null otherwise (nothing is checked). */
  currentModel: string | null;
  onPick: (model: string, label: string) => void;
  onManageNodes: () => void;
  onEngine: () => void;
  onOtherModels: () => void;
  /** This chat's nodes; null when the menu is not a chat's (no session). */
  chatNodes?: ChatNodesControl | null;
}) {
  const intl = useIntl();
  const names = nodeNamesById(read.nodes);
  const stateOf = (id: string) =>
    residencyChipState(residency.nodes.find((r) => r.node === id)?.residency);
  const current = routeBase(currentModel);
  // A chat's own node set is listed by no one: its chat shows it in the section above.
  const strategies = namedStrategies(read.config);

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
      {chatNodes && <ChatNodesSection control={chatNodes} names={names} stateOf={stateOf} />}
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

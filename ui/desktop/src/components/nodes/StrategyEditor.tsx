import { useId, useState } from 'react';
import { ArrowUp, Plus, X } from 'lucide-react';
import type { BuildEligibility } from '../../acp/nodes';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Segmented, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import {
  INPUT,
  StudioSelect,
  ToneBanner,
  WeightStepper,
  type StudioSelectOption,
} from '../leanzero-swarm/studio';
import type { Mac } from '../leanzero-swarm/macs';
import { RoleChip, ROLE_WORD, StateChip } from './NodeChips';
import {
  ROLES,
  effectiveEntry,
  effectiveRole,
  ifServingOtherForNewChain,
  ifServingOtherOf,
  inheritsFrom,
  nodeNamesById,
  type NodeIfNotLoaded,
  type NodeIfServingOther,
  type NodeRole,
  type NodeRoleEntry,
  type NodeStrategy,
  type NodeStrategyRoles,
  type NodeWhen,
  type NodesConfig,
  type ResolvedNodeDef,
} from './model';
import type { NodeGlance, Read } from './nodeGlance';
import { sentenceFacts } from './resolve';
import { sentenceFor } from './strategySentence';
import { strategyFit, wayKeyOf, type MeasuredLoad, type StrategyFit } from './strategyFit';
import { useDraftBuildEligibility } from './useBuildEligibility';
import {
  BuildsLine,
  SameAs,
  delegateWords,
  loadWords,
  shareTwoWaysWords,
  unknownNodeWords,
  wayName,
} from './StrategyCard';

/**
 * THE STRATEGY EDITOR (DESIGN-NODES-AND-STRATEGIES.md §8.4): one row per role — what it is in plain
 * words, its nodes in order (each picker option wears the node's state chip from the cards' own
 * derivation), when the next one takes work, what happens when an MLX node is not loaded, and the
 * rule read back as a sentence. Beside it, "On your Macs": what the strategy asks of your Macs under
 * the v1 rule (one MLX way serves this Mac's goose at a time, across all your Macs — strategyFit),
 * and whether swarm builds can use it — goosed's answer for the strategy as it is held NOW (the
 * stored one's answer while nothing changed, a draft check while it is edited — Q-311), never guessed
 * here.
 *
 * The editor validates nothing itself: Save goes through `nodes/write` and every refusal is shown
 * verbatim, the draft kept as it was.
 */

const i18n = defineMessages({
  newTitle: { id: 'strategies.newTitle', defaultMessage: 'New strategy' },
  editTitle: { id: 'strategies.editTitle', defaultMessage: 'Edit {name}' },
  name: { id: 'strategies.name', defaultMessage: 'Strategy' },
  note: { id: 'strategies.note', defaultMessage: 'Note (your words)' },
  colNodes: { id: 'strategies.colNodes', defaultMessage: 'Nodes in order' },
  colWhen: { id: 'strategies.colWhen', defaultMessage: 'When to use the next' },
  colIfNotLoaded: { id: 'strategies.colIfNotLoaded', defaultMessage: 'If not loaded' },
  whenFailover: { id: 'strategies.whenFailover', defaultMessage: 'can’t run' },
  whenOverflow: { id: 'strategies.whenOverflow', defaultMessage: 'busy' },
  whenShare: { id: 'strategies.whenShare', defaultMessage: 'share' },
  loadWait: { id: 'strategies.loadWait', defaultMessage: 'Load it and wait' },
  useNext: { id: 'strategies.useNext', defaultMessage: 'Use the next meanwhile' },
  // Q-428, the owner: "the strategy should have the option hopefully to avoid interrupting a node
  // doing its thing".
  colIfServingOther: {
    id: 'strategies.colIfServingOther',
    defaultMessage: 'If its Mac is serving another node',
  },
  servingOtherUseNext: {
    id: 'strategies.servingOtherUseNext',
    defaultMessage: 'Use the next node',
  },
  servingOtherWait: { id: 'strategies.servingOtherWait', defaultMessage: 'Wait' },
  servingOtherTakeOver: { id: 'strategies.servingOtherTakeOver', defaultMessage: 'Take it over' },
  servingOtherUseNextSays: {
    id: 'strategies.servingOtherUseNextSays',
    defaultMessage:
      'While its Mac answers another chat, or keeps another chat’s node between its messages, the next node takes the turn. Nothing is stopped.',
  },
  servingOtherWaitSays: {
    id: 'strategies.servingOtherWaitSays',
    defaultMessage:
      'The turn waits until the other chats are done with their node — their replies end, and each chat is closed or moves to another node — then this one loads.',
  },
  servingOtherTakeOverSays: {
    id: 'strategies.servingOtherTakeOverSays',
    defaultMessage:
      'This one loads as soon as the other node’s running replies end. A chat resting between messages loses its node.',
  },
  servingOtherNeedsNext: {
    id: 'strategies.servingOtherNeedsNext',
    defaultMessage: 'Add a second node to use the next one.',
  },
  setOwn: { id: 'strategies.setOwn', defaultMessage: 'Set its own nodes' },
  useSameAs: { id: 'strategies.useSameAs', defaultMessage: 'Same as {role} instead' },
  usedByBuilds: { id: 'strategies.usedByBuilds', defaultMessage: 'Used by swarm builds.' },
  // Q-433: Build also runs the delegates of a chat on this strategy (summon's `@build`).
  usedByBuildRole: {
    id: 'strategies.usedByBuildRole',
    defaultMessage: 'Delegates of chats on this strategy, and swarm builds.',
  },
  addNode: { id: 'strategies.addNode', defaultMessage: 'Add a node' },
  addNodeOnlyOne: {
    id: 'strategies.addNodeOnlyOne',
    defaultMessage: 'Only one node exists — make another under Nodes to add it here',
  },
  addNodeAllIn: {
    id: 'strategies.addNodeAllIn',
    defaultMessage: 'Every node you have is already in this list',
  },
  pickNode: { id: 'strategies.pickNode', defaultMessage: 'Pick a node' },
  nodeAt: { id: 'strategies.nodeAt', defaultMessage: '{role}: node {rank}' },
  roleField: { id: 'strategies.roleField', defaultMessage: '{role}: {field}' },
  weightOf: { id: 'strategies.weightOf', defaultMessage: 'share of {node}' },
  earlier: { id: 'strategies.earlier', defaultMessage: 'Try {node} earlier' },
  removeEntry: { id: 'strategies.removeEntry', defaultMessage: 'Take {node} out of {role}' },
  noNodes: {
    id: 'strategies.noNodes',
    defaultMessage: 'There are no nodes yet. Make one on the Nodes tab first.',
  },
  onYourMacs: { id: 'strategies.onYourMacsTitle', defaultMessage: 'On your Macs' },
  oneWayRule: {
    id: 'strategies.oneWayRule',
    defaultMessage:
      'Your Macs run one model at a time for goose on this Mac — on one Mac or split across them. Nodes in this strategy that run differently take turns: each switch stops one and starts the other.',
  },
  oneWay: {
    id: 'strategies.oneWay',
    defaultMessage: 'Only {node} runs on your Macs for this strategy, so nothing switches.',
  },
  noWaysPanel: {
    id: 'strategies.noWaysPanel',
    defaultMessage: 'Nothing loads on your Macs for this strategy.',
  },
  wayRow: { id: 'strategies.wayRow', defaultMessage: '{node}: {load}' },
  swapPairWarn: {
    id: 'strategies.swapPairWarn',
    defaultMessage: '{a} ⇄ {b}: each switch between them stops one and loads the other.',
  },
  cloudRow: { id: 'strategies.cloudRow', defaultMessage: '{node}: always available' },
  followsRow: {
    id: 'strategies.followsRow',
    defaultMessage: '{node}: uses whatever this Mac is running; it never starts a model itself',
  },
  buildsTitle: { id: 'strategies.buildsTitle', defaultMessage: 'Swarm builds' },
  buildsUnsaved: {
    id: 'strategies.buildsUnsaved',
    defaultMessage: 'Checked as you edit; nothing is saved yet.',
  },
  refused: { id: 'strategies.refused', defaultMessage: 'Not saved' },
  cancel: { id: 'strategies.cancel', defaultMessage: 'Cancel' },
  save: { id: 'strategies.save', defaultMessage: 'Save strategy' },
  whatChat: {
    id: 'strategies.whatChat',
    defaultMessage: 'Your turns in a chat: answers, edits, tool calls',
  },
  whatPlanning: {
    id: 'strategies.whatPlanning',
    defaultMessage: 'Reading the request, asking questions, researching and writing the plan',
  },
  whatBuild: { id: 'strategies.whatBuild', defaultMessage: 'Writing the code for each task' },
  whatTesting: {
    id: 'strategies.whatTesting',
    defaultMessage: 'Running and checking the result, and fixing what the check finds',
  },
  whatFrontend: {
    id: 'strategies.whatFrontend',
    defaultMessage: 'Build tasks that write the user interface (pages, components, styles)',
  },
  whatBackend: {
    id: 'strategies.whatBackend',
    defaultMessage: 'Build tasks that write the server side (APIs, services, data)',
  },
});

const ROLE_WHAT: Record<NodeRole, (typeof i18n)['whatChat']> = {
  chat: i18n.whatChat,
  planning: i18n.whatPlanning,
  build: i18n.whatBuild,
  testing: i18n.whatTesting,
  frontend: i18n.whatFrontend,
  backend: i18n.whatBackend,
};

/**
 * A new strategy (§8.4): ONE Chat row set to what new chats start on now — that node, that
 * strategy's Chat chain, or (on Auto) the node serving now, else the first node — and every other
 * role unset, so Build reads "Same as Chat" and a new strategy never swaps by default. With no
 * node at all the chain is empty and Save shows goosed's refusal.
 */
export function newStrategyDraft(
  config: NodesConfig,
  nodes: readonly ResolvedNodeDef[],
  servingNodeId: string | null,
  id: string,
  name: string
): NodeStrategy {
  const use = config.forNewChats ?? { kind: 'auto' };
  let chat: NodeRoleEntry | null = null;
  if (use.kind === 'node' && nodes.some((n) => n.def.id === use.id)) {
    chat = { chain: [{ node: use.id, weight: 1 }], when: 'failover', ifNotLoaded: 'load' };
  } else if (use.kind === 'strategy') {
    const from = (config.strategies ?? []).find((s) => s.id === use.id);
    const entry = from ? effectiveEntry(from, 'chat') : null;
    if (entry) chat = { ...entry, chain: [...entry.chain] };
  }
  if (!chat) {
    const first = servingNodeId ?? nodes[0]?.def.id ?? null;
    chat = {
      chain: first ? [{ node: first, weight: 1 }] : [],
      when: 'failover',
      ifNotLoaded: 'load',
    };
  }
  // Q-428: a new strategy never displaces another chat's node by default — the next node when
  // the chain has one, otherwise wait. A setting the copied chain chose itself is kept.
  chat.ifServingOther = chat.ifServingOther ?? ifServingOtherForNewChain(chat.chain.length);
  return { id, name, roles: { chat } };
}

/** Whether unsetting `role` leaves every role resolvable (Chat and Build may not both be unset). */
function canUnset(roles: NodeStrategyRoles, role: NodeRole): boolean {
  const next = { ...roles, [role]: null };
  return ROLES.some((r) => next[r]) && ROLES.every((r) => effectiveRole(next, r) !== null);
}

interface NodeOption extends StudioSelectOption {
  node: ResolvedNodeDef;
}

export interface StrategyEditorProps {
  /** The strategy as stored; null for a new one. */
  stored: NodeStrategy | null;
  initial: NodeStrategy;
  nodes: readonly ResolvedNodeDef[];
  glanceOf: (node: ResolvedNodeDef) => NodeGlance;
  measured: MeasuredLoad;
  macs: readonly Mac[];
  /** goosed's answer for the STORED strategy. */
  builds: Read<BuildEligibility> | undefined;
  busy: boolean;
  refusals: string[];
  onSave: (draft: NodeStrategy) => void;
  onClose: () => void;
}

export function StrategyEditor({
  stored,
  initial,
  nodes,
  glanceOf,
  measured,
  macs,
  builds,
  busy,
  refusals,
  onSave,
  onClose,
}: StrategyEditorProps) {
  const intl = useIntl();
  // The host keys the editor by what it edits, so `initial` seeds the draft once.
  const [draft, setDraft] = useState<NodeStrategy>(initial);

  const roles = draft.roles ?? {};
  const byId = new Map(nodes.map((n) => [n.def.id, n]));
  const names = nodeNamesById(nodes);
  const fit = strategyFit(draft, nodes, measured);
  const dirty = stored === null || JSON.stringify(stored) !== JSON.stringify(draft);
  const draftBuilds = useDraftBuildEligibility(draft, dirty);
  const addWhyId = useId();

  const setRole = (role: NodeRole, entry: NodeRoleEntry | null) =>
    setDraft((d) => ({ ...d, roles: { ...(d.roles ?? {}), [role]: entry } }));

  const about = (id: string) => {
    const node = byId.get(id);
    return {
      loads: node ? wayKeyOf(node) !== null : false,
      loadMs: node ? (measured(node)?.medianMs ?? null) : null,
    };
  };

  const options: NodeOption[] = nodes.map((node) => ({
    value: node.def.id,
    label: node.def.name,
    node,
  }));
  const renderNode = (o: NodeOption) => (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span className="min-w-0 flex-1 truncate">{o.label}</span>
      <StateChip state={glanceOf(o.node).state} />
    </span>
  );

  const roleRow = (role: NodeRole) => {
    const entry = roles[role] ?? null;
    const roleWord = intl.formatMessage(ROLE_WORD[role]);
    const header = (
      <div className="flex min-w-0 flex-col items-start gap-1">
        <RoleChip role={role} />
        <p className={cx('break-words', TYPE.bodyMuted)}>{intl.formatMessage(ROLE_WHAT[role])}</p>
        {role !== 'chat' && (
          <p className={cx('break-words', TYPE.meta)}>
            {intl.formatMessage(role === 'build' ? i18n.usedByBuildRole : i18n.usedByBuilds)}
          </p>
        )}
      </div>
    );
    if (!entry) {
      const source = effectiveRole(roles, role);
      const inherited = effectiveEntry(draft, role);
      return (
        <section
          key={role}
          data-testid="strategy-role"
          data-role={role}
          data-set="false"
          className={cx('grid gap-3 p-3 min-[900px]:grid-cols-[14rem_minmax(0,1fr)]', SURFACE.card)}
        >
          {header}
          <div className="flex min-w-0 flex-wrap items-center gap-3">
            {source && <SameAs role={role} source={source} />}
            <Button
              variant="secondary"
              size="sm"
              icon={<Plus />}
              onClick={() =>
                setRole(
                  role,
                  inherited
                    ? { ...inherited, chain: [...inherited.chain] }
                    : {
                        chain: nodes[0] ? [{ node: nodes[0].def.id, weight: 1 }] : [],
                        when: 'failover',
                        ifNotLoaded: 'load',
                        ifServingOther: ifServingOtherForNewChain(nodes[0] ? 1 : 0),
                      }
                )
              }
              data-testid="strategy-set-own"
            >
              {intl.formatMessage(i18n.setOwn)}
            </Button>
          </div>
        </section>
      );
    }

    const when: NodeWhen = entry.when ?? 'failover';
    const ifNotLoaded: NodeIfNotLoaded = entry.ifNotLoaded ?? 'load';
    const facts = sentenceFacts(draft, role, names);
    const sentence = facts ? sentenceFor(intl, facts, about) : '';
    const anyMlx = entry.chain.some((l) => about(l.node).loads);
    const inChain = new Set(entry.chain.map((l) => l.node));
    const nextFree = nodes.find((n) => !inChain.has(n.def.id));
    // A disabled "Add a node" says why beside it (Q-311); with no node at all, `noNodes` says it.
    const addBlocked =
      nextFree || nodes.length === 0
        ? null
        : intl.formatMessage(nodes.length === 1 ? i18n.addNodeOnlyOne : i18n.addNodeAllIn);
    const ifServingOther: NodeIfServingOther = ifServingOtherOf(entry);
    const change = (next: Partial<NodeRoleEntry>) => setRole(role, { ...entry, ...next });
    // A chain down to one node has no next node to use: the setting says so by waiting instead.
    const setChain = (chain: NodeRoleEntry['chain']) =>
      change(
        chain.length < 2 && ifServingOther === 'useNext'
          ? { chain, ifServingOther: 'wait' }
          : { chain }
      );
    const servingOtherSays = {
      useNext: i18n.servingOtherUseNextSays,
      wait: i18n.servingOtherWaitSays,
      takeOver: i18n.servingOtherTakeOverSays,
    }[ifServingOther];

    return (
      <section
        key={role}
        data-testid="strategy-role"
        data-role={role}
        data-set="true"
        className={cx('grid gap-3 p-3 min-[900px]:grid-cols-[14rem_minmax(0,1fr)]', SURFACE.card)}
      >
        {header}
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex flex-col gap-2">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.colNodes)}</span>
            <ol className="flex flex-col gap-2" data-testid="strategy-chain">
              {entry.chain.map((link, index) => {
                const node = byId.get(link.node);
                const nodeName = node?.def.name ?? link.node;
                const value = options.find((o) => o.value === link.node) ?? null;
                return (
                  <li
                    key={`${link.node}-${index}`}
                    className="flex min-w-0 flex-wrap items-center gap-2"
                    data-testid="strategy-chain-entry"
                  >
                    <span className={cx('w-5 text-right', TYPE.body, WEIGHT.semibold, TNUM)}>
                      {index + 1}
                    </span>
                    <StudioSelect<NodeOption>
                      className="min-w-[12rem] flex-1"
                      aria-label={intl.formatMessage(i18n.nodeAt, {
                        role: roleWord,
                        rank: index + 1,
                      })}
                      options={options.map((o) => ({
                        ...o,
                        disabled: o.value !== link.node && inChain.has(o.value),
                      }))}
                      value={value}
                      placeholder={value ? nodeName : intl.formatMessage(i18n.pickNode)}
                      renderOption={renderNode}
                      optionTestId={(o) => `strategy-pick-${o.value}`}
                      onChange={(o) =>
                        o &&
                        setChain(
                          entry.chain.map((l, i) => (i === index ? { ...l, node: o.value } : l))
                        )
                      }
                    />
                    {when === 'share' && (
                      <WeightStepper
                        value={link.weight}
                        label={intl.formatMessage(i18n.weightOf, { node: nodeName })}
                        onChange={(weight) =>
                          setChain(entry.chain.map((l, i) => (i === index ? { ...l, weight } : l)))
                        }
                      />
                    )}
                    {index > 0 && (
                      <Button
                        variant="ghost"
                        size="sm"
                        iconOnly
                        icon={<ArrowUp />}
                        aria-label={intl.formatMessage(i18n.earlier, { node: nodeName })}
                        onClick={() => {
                          const next = [...entry.chain];
                          [next[index - 1], next[index]] = [next[index], next[index - 1]];
                          setChain(next);
                        }}
                      />
                    )}
                    {entry.chain.length > 1 && (
                      <Button
                        variant="ghost"
                        size="sm"
                        iconOnly
                        icon={<X />}
                        aria-label={intl.formatMessage(i18n.removeEntry, {
                          node: nodeName,
                          role: roleWord,
                        })}
                        onClick={() => setChain(entry.chain.filter((_, i) => i !== index))}
                      />
                    )}
                  </li>
                );
              })}
            </ol>
            {nodes.length === 0 && (
              <p className={cx('break-words', TYPE.meta)}>{intl.formatMessage(i18n.noNodes)}</p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                icon={<Plus />}
                disabled={!nextFree}
                aria-describedby={addBlocked ? `${addWhyId}-${role}` : undefined}
                onClick={() =>
                  nextFree && setChain([...entry.chain, { node: nextFree.def.id, weight: 1 }])
                }
                data-testid="strategy-add-node"
              >
                {intl.formatMessage(i18n.addNode)}
              </Button>
              {addBlocked && (
                <span
                  id={`${addWhyId}-${role}`}
                  className={cx('break-words', TYPE.meta, WEIGHT.semibold)}
                  data-testid="strategy-add-node-why"
                >
                  {addBlocked}
                </span>
              )}
              {canUnset(roles, role) && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setRole(role, null)}
                  data-testid="strategy-unset"
                >
                  {intl.formatMessage(i18n.useSameAs, {
                    role: intl.formatMessage(ROLE_WORD[inheritsFrom(role)]),
                  })}
                </Button>
              )}
            </div>
          </div>

          {(entry.chain.length > 1 || anyMlx) && (
            <div className="flex flex-wrap gap-x-6 gap-y-3">
              {entry.chain.length > 1 && (
                <div className="flex flex-col gap-1">
                  <span className={TYPE.meta}>{intl.formatMessage(i18n.colWhen)}</span>
                  <Segmented<NodeWhen>
                    aria-label={intl.formatMessage(i18n.roleField, {
                      role: roleWord,
                      field: intl.formatMessage(i18n.colWhen),
                    })}
                    value={when}
                    onChange={(next) => change({ when: next })}
                    options={[
                      { value: 'failover', label: intl.formatMessage(i18n.whenFailover) },
                      { value: 'overflow', label: intl.formatMessage(i18n.whenOverflow) },
                      { value: 'share', label: intl.formatMessage(i18n.whenShare) },
                    ]}
                  />
                </div>
              )}
              {anyMlx && (
                <div className="flex flex-col gap-1">
                  <span className={TYPE.meta}>{intl.formatMessage(i18n.colIfNotLoaded)}</span>
                  <Segmented<NodeIfNotLoaded>
                    aria-label={intl.formatMessage(i18n.roleField, {
                      role: roleWord,
                      field: intl.formatMessage(i18n.colIfNotLoaded),
                    })}
                    value={ifNotLoaded}
                    onChange={(next) => change({ ifNotLoaded: next })}
                    options={[
                      { value: 'load', label: intl.formatMessage(i18n.loadWait) },
                      { value: 'useNext', label: intl.formatMessage(i18n.useNext) },
                    ]}
                  />
                </div>
              )}
              {anyMlx && ifNotLoaded === 'load' && (
                <div
                  className="flex min-w-0 max-w-[32rem] flex-col gap-1"
                  data-testid="strategy-serving-other"
                >
                  <span className={TYPE.meta}>{intl.formatMessage(i18n.colIfServingOther)}</span>
                  <Segmented<NodeIfServingOther>
                    aria-label={intl.formatMessage(i18n.roleField, {
                      role: roleWord,
                      field: intl.formatMessage(i18n.colIfServingOther),
                    })}
                    value={ifServingOther}
                    onChange={(next) => change({ ifServingOther: next })}
                    options={[
                      {
                        value: 'useNext',
                        label: intl.formatMessage(i18n.servingOtherUseNext),
                        disabled: entry.chain.length < 2,
                        describedBy:
                          entry.chain.length < 2 ? `${addWhyId}-${role}-next` : undefined,
                        testId: 'strategy-serving-other-useNext',
                      },
                      {
                        value: 'wait',
                        label: intl.formatMessage(i18n.servingOtherWait),
                        testId: 'strategy-serving-other-wait',
                      },
                      {
                        value: 'takeOver',
                        label: intl.formatMessage(i18n.servingOtherTakeOver),
                        testId: 'strategy-serving-other-takeOver',
                      },
                    ]}
                  />
                  <p
                    className={cx('break-words', TYPE.meta)}
                    data-testid="strategy-serving-other-says"
                  >
                    {intl.formatMessage(servingOtherSays)}
                  </p>
                  {entry.chain.length < 2 && (
                    <p
                      id={`${addWhyId}-${role}-next`}
                      className={cx('break-words', TYPE.meta, WEIGHT.semibold)}
                    >
                      {intl.formatMessage(i18n.servingOtherNeedsNext)}
                    </p>
                  )}
                </div>
              )}
            </div>
          )}

          {sentence && (
            <p
              className={cx('break-words', TYPE.body, WEIGHT.medium)}
              data-testid="strategy-sentence"
            >
              {sentence}
            </p>
          )}
        </div>
      </section>
    );
  };

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx(
        'flex max-h-[calc(100vh-2rem)] w-[80rem] flex-col gap-4 overflow-y-auto p-5',
        'max-[640px]:h-screen max-[640px]:max-h-screen max-[640px]:w-screen max-[640px]:max-w-none max-[640px]:rounded-none max-[640px]:p-4',
        SURFACE.overlay
      )}
    >
      <div className="flex flex-col gap-4" data-testid="strategy-editor">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>
            {stored
              ? intl.formatMessage(i18n.editTitle, { name: stored.name })
              : intl.formatMessage(i18n.newTitle)}
          </h2>
        </OverlayDialogTitle>

        <div className="grid grid-cols-1 gap-3 min-[720px]:grid-cols-[minmax(0,20rem)_minmax(0,1fr)]">
          <label className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.name)}</span>
            <input
              className={cx(INPUT, 'w-full')}
              value={draft.name}
              onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
              data-testid="strategy-name"
            />
          </label>
          <label className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.note)}</span>
            <input
              className={cx(INPUT, 'w-full')}
              value={draft.note ?? ''}
              onChange={(e) =>
                setDraft((d) => ({ ...d, note: e.target.value === '' ? null : e.target.value }))
              }
              data-testid="strategy-note-input"
            />
          </label>
        </div>

        <div className="grid grid-cols-1 gap-4 min-[1400px]:grid-cols-[minmax(0,1fr)_22rem]">
          <div className="flex min-w-0 flex-col gap-3">{ROLES.map(roleRow)}</div>
          <FitPanel
            fit={fit}
            builds={dirty ? (draftBuilds ?? undefined) : builds}
            dirty={dirty}
            names={names}
            macs={macs}
          />
        </div>

        {refusals.length > 0 && (
          <div className="flex flex-col gap-2" data-testid="strategy-refusals">
            {refusals.map((r) => (
              <ToneBanner key={r} tone="err" label={intl.formatMessage(i18n.refused)} text={r} />
            ))}
          </div>
        )}

        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            {intl.formatMessage(i18n.cancel)}
          </Button>
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => onSave(draft)}
            data-testid="strategy-save"
          >
            {intl.formatMessage(i18n.save)}
          </Button>
        </div>
      </div>
    </OverlayDialog>
  );
}

/** "On your Macs": the ways the strategy names, the swaps between them, and what never loads. */
function FitPanel({
  fit,
  builds,
  dirty,
  names,
  macs,
}: {
  fit: StrategyFit;
  builds: Read<BuildEligibility> | undefined;
  dirty: boolean;
  names: Record<string, string>;
  macs: readonly Mac[];
}) {
  const intl = useIntl();
  const delegate = delegateWords(intl, fit);
  const refusals = [...shareTwoWaysWords(intl, fit), ...unknownNodeWords(intl, fit)];
  return (
    <aside
      className={cx('flex min-w-0 flex-col gap-2 self-start p-4', SURFACE.card)}
      data-testid="strategy-fit-panel"
    >
      <span className={TYPE.zone}>{intl.formatMessage(i18n.onYourMacs)}</span>
      <p className={cx('break-words', TYPE.meta)}>{intl.formatMessage(i18n.oneWayRule)}</p>
      {fit.ways.length === 0 && (
        <p className={cx('break-words', TYPE.body)} data-testid="strategy-fit-none">
          {intl.formatMessage(i18n.noWaysPanel)}
        </p>
      )}
      {fit.ways.length === 1 && (
        <p className={cx('break-words', TYPE.body)} data-testid="strategy-fit-one">
          {intl.formatMessage(i18n.oneWay, { node: wayName(fit.ways[0]) })}
        </p>
      )}
      {fit.ways.map((way) => (
        <div key={way.key} className="flex min-w-0 flex-col gap-1" data-testid="strategy-fit-way">
          <p className={cx('break-words', TYPE.body)}>
            {intl.formatMessage(i18n.wayRow, { node: wayName(way), load: loadWords(intl, way) })}
          </p>
          <div className="flex flex-wrap gap-1">
            {way.uses.map((u) => (
              <RoleChip key={u.role} role={u.role} />
            ))}
          </div>
        </div>
      ))}
      {fit.swaps.map(([a, b]) => (
        <p
          key={`${a.key}|${b.key}`}
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
          data-testid="strategy-fit-swap"
        >
          {intl.formatMessage(i18n.swapPairWarn, { a: wayName(a), b: wayName(b) })}
        </p>
      ))}
      {delegate && (
        <p
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
          data-testid="strategy-fit-delegate"
        >
          {delegate}
        </p>
      )}
      {refusals.map((r) => (
        <p
          key={r}
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
          data-testid="strategy-fit-refusal"
        >
          {r}
        </p>
      ))}
      {fit.cloud.map((n) => (
        <p key={n.def.id} className={cx('break-words', TYPE.body)} data-testid="strategy-fit-cloud">
          {intl.formatMessage(i18n.cloudRow, { node: n.def.name })}
        </p>
      ))}
      {fit.follows.map((n) => (
        <p
          key={n.def.id}
          className={cx('break-words', TYPE.body)}
          data-testid="strategy-fit-follows"
        >
          {intl.formatMessage(i18n.followsRow, { node: n.def.name })}
        </p>
      ))}
      <span className={cx('mt-2', TYPE.zone)}>{intl.formatMessage(i18n.buildsTitle)}</span>
      <BuildsLine answer={builds} names={names} macs={macs} />
      {dirty && (
        <p className={cx('break-words', TYPE.meta)} data-testid="strategy-builds-unsaved">
          {intl.formatMessage(i18n.buildsUnsaved)}
        </p>
      )}
    </aside>
  );
}

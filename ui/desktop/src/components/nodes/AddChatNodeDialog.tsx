import { useState } from 'react';
import type { IntlShape } from 'react-intl';
import { Loader2, Plus, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { WithMacs } from '../leanzero-swarm/useMacs';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { useEngineGlance } from '../engineGlance/glanceStore';
import { KindChip, StateChip, WhereChip } from './NodeChips';
import { lineText } from './NodeCard';
import { loadPhaseWord } from './loadPhaseWord';
import { useNodeFacts } from './useNodeFacts';
import {
  chatNodeAvailability,
  chatNodeIds,
  type AddLine,
  type ChatNodeAvailability,
  type ChatNodesNow,
  type GlancedNode,
  type MacRef,
  type NotAddable,
  type ServingNow,
} from './chatNodeAvailability';
import type { ChatNodesWrite } from './useChatNodes';

/**
 * "+ Add a node to this chat…" (Q-359, DESIGN-Q359-CHAT-NODES.md "Control"): every node the chat
 * does not have yet, each with what it is doing now — the Nodes page's own derivation
 * (`useNodeFacts` + `nodeGlance` under `WithMacs`, mounted here as the Nodes page mounts them; the
 * chip's dropdown cannot mount `useMacs`, design §13 item 9) — and whether it can join. A node that
 * can't is shown at full strength with its reason and no Add button: never greyed, never hidden.
 */

const i18n = defineMessages({
  title: { id: 'chatNodes.addTitle', defaultMessage: 'Add a node to this chat' },
  close: { id: 'chatNodes.close', defaultMessage: 'Close' },
  contextSet: {
    id: 'chatNodes.addContextSet',
    defaultMessage: '{lead} answers this chat. Its delegates share every node you add.',
  },
  contextStrategy: {
    id: 'chatNodes.addContextStrategy',
    defaultMessage:
      'Adds to this chat only; {strategy} stays as it is. {lead} keeps answering this chat.',
  },
  contextNone: {
    id: 'chatNodes.addContextNone',
    defaultMessage: 'The first node you add answers this chat.',
  },
  add: { id: 'chatNodes.add', defaultMessage: 'Add' },
  addNamed: { id: 'chatNodes.addNamed', defaultMessage: 'Add {node} to this chat' },
  refused: { id: 'chatNodes.refused', defaultMessage: 'Not added' },
  reading: { id: 'chatNodes.reading', defaultMessage: 'Reading your nodes…' },
  readFailed: { id: 'chatNodes.readFailed', defaultMessage: 'Your nodes could not be read: {error}' },
  noOthers: {
    id: 'chatNodes.noOthers',
    defaultMessage: 'Every node you have already runs this chat.',
  },
  serving: { id: 'chatNodes.lineServing', defaultMessage: 'Serving' },
  startsIn: {
    id: 'chatNodes.lineStartsIn',
    defaultMessage:
      'Not loaded · starts in about {duration} · median of {count, plural, one {# load} other {# loads}}',
  },
  firstStart: { id: 'chatNodes.lineFirstStart', defaultMessage: 'First start not measured yet' },
  loading: { id: 'chatNodes.lineLoading', defaultMessage: 'Loading · {phase}' },
  waiting: { id: 'chatNodes.lineWaiting', defaultMessage: 'Waiting to load · {words}' },
  stopsOtherChat: {
    id: 'chatNodes.lineStopsOtherChat',
    defaultMessage:
      '{mac} is answering chat “{chat}” on {node}. Adding it stops that after its answer',
  },
  sameMac: {
    id: 'chatNodes.notSameMac',
    defaultMessage:
      'Runs on {mac}, where {lead} answers this chat. A Mac runs one model at a time for goose',
  },
  leadSplit: {
    id: 'chatNodes.notLeadSplit',
    defaultMessage: '{lead} runs across both Macs, so no Mac is free for another model',
  },
  beforeC3: {
    id: 'chatNodes.notBeforeC3',
    defaultMessage: 'Can’t run beside {lead} yet: your Macs serve goose one model at a time',
  },
  thisMacStart: { id: 'chatNodes.thisMacStart', defaultMessage: 'This Mac' },
  thisMacMid: { id: 'chatNodes.thisMacMid', defaultMessage: 'this Mac' },
});

function macText(intl: IntlShape, mac: MacRef, startOfSentence: boolean): string {
  if (mac.kind === 'mac') return mac.name;
  return intl.formatMessage(startOfSentence ? i18n.thisMacStart : i18n.thisMacMid);
}

/** An addable node's line, in the person's words. */
export function addLineText(intl: IntlShape, line: AddLine): string {
  switch (line.kind) {
    case 'serving':
      return intl.formatMessage(i18n.serving);
    case 'notLoaded':
      return line.start
        ? intl.formatMessage(i18n.startsIn, {
            duration: formatElapsed(line.start.medianMs / 1000),
            count: line.start.count,
          })
        : intl.formatMessage(i18n.firstStart);
    case 'loading':
      return intl.formatMessage(i18n.loading, { phase: loadPhaseWord(intl, line.phase) });
    case 'waiting':
      return intl.formatMessage(i18n.waiting, { words: line.words });
    case 'stopsOtherChat':
      return intl.formatMessage(i18n.stopsOtherChat, {
        mac: macText(intl, line.mac, true),
        chat: line.chat,
        node: line.node,
      });
    case 'glance':
      return lineText(intl, line.line);
  }
}

/** Why a node can't join, in the person's words. */
export function notAddableText(intl: IntlShape, reason: NotAddable): string {
  switch (reason.kind) {
    case 'sameMac':
      return intl.formatMessage(i18n.sameMac, {
        mac: macText(intl, reason.mac, false),
        lead: reason.lead,
      });
    case 'leadSplit':
      return intl.formatMessage(i18n.leadSplit, { lead: reason.lead });
    case 'beforeC3':
      return intl.formatMessage(i18n.beforeC3, { lead: reason.lead });
    case 'glance':
      return lineText(intl, reason.line);
  }
}

export interface AddChatNodeDialogProps {
  /** This chat's session. */
  sessionId: string;
  /** What the chat runs on now (`chatNodesNow`). */
  now: ChatNodesNow;
  /** Stores the chat's set through goosed's one door (`useChatNodes`). */
  setChatNodes: (nodes: string[], answerOnNext: boolean) => Promise<ChatNodesWrite>;
  onClose: () => void;
}

export function AddChatNodeDialog(props: AddChatNodeDialogProps) {
  return (
    <WithMacs>
      <AddChatNodeBody {...props} />
    </WithMacs>
  );
}

function AddChatNodeBody({ sessionId, now, setChatNodes, onClose }: AddChatNodeDialogProps) {
  const intl = useIntl();
  const { store, nodes, servingNode, glanceOf } = useNodeFacts();
  const push = useEngineGlance();
  const [busy, setBusy] = useState<string | null>(null);
  const [refusals, setRefusals] = useState<string[]>([]);

  const ids = chatNodeIds(now);
  const set: GlancedNode[] = ids
    .map((id) => nodes.find((n) => n.def.id === id))
    .filter((n): n is NonNullable<typeof n> => n != null)
    .map((node) => ({ node, glance: glanceOf(node) }));
  const lead = set[0]?.node.def.name ?? null;
  const residency = store.kind === 'read' ? store.residency : null;
  const servingWay = residency?.serving ?? null;
  const engineChat = push?.engine?.chat ?? null;
  const serving: ServingNow = {
    node: servingNode?.def.name ?? null,
    mac:
      servingWay == null
        ? null
        : servingWay.kind === 'single'
          ? { kind: 'thisMac' }
          : { kind: 'mac', name: intl.formatList(servingWay.macNames, { type: 'conjunction' }) },
    otherChat: engineChat && engineChat.sessionId !== sessionId ? engineChat.name : null,
  };
  const candidates = nodes
    .filter((n) => !ids.includes(n.def.id))
    .map((node) => {
      const glanced = { node, glance: glanceOf(node) };
      return { ...glanced, availability: chatNodeAvailability(glanced, set, serving) };
    });

  const add = async (id: string) => {
    setBusy(id);
    setRefusals([]);
    const answerOnNext = now.kind === 'set' || now.kind === 'strategy' ? now.answerOnNext : false;
    const result = await setChatNodes([...ids, id], answerOnNext);
    setBusy(null);
    if (result.applied) onClose();
    else setRefusals(result.refusals);
  };

  const context =
    now.kind === 'strategy' && lead
      ? intl.formatMessage(i18n.contextStrategy, { strategy: now.strategy.name, lead })
      : lead
        ? intl.formatMessage(i18n.contextSet, { lead })
        : intl.formatMessage(i18n.contextNone);

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx(
        'flex max-h-[calc(100vh-2rem)] w-[40rem] flex-col gap-4 overflow-y-auto p-5',
        SURFACE.overlay
      )}
    >
      <div className="flex items-start justify-between gap-3" data-testid="add-chat-node-dialog">
        <div className="flex min-w-0 flex-col gap-1">
          <OverlayDialogTitle asChild>
            <h2 className={TYPE.h2}>{intl.formatMessage(i18n.title)}</h2>
          </OverlayDialogTitle>
          <p className={cx('break-words', TYPE.bodyMuted)} data-testid="add-chat-node-context">
            {context}
          </p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          iconOnly
          icon={<X />}
          aria-label={intl.formatMessage(i18n.close)}
          onClick={onClose}
        />
      </div>

      {store.kind === 'failed' && (
        <p className={cx('break-words', TYPE.body, TONE_TEXT.err)} role="alert">
          {intl.formatMessage(i18n.readFailed, { error: store.error })}
        </p>
      )}
      {store.kind === 'unread' && (
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.reading)}</p>
      )}
      {store.kind === 'read' && candidates.length === 0 && (
        <p className={TYPE.bodyMuted} data-testid="add-chat-node-none">
          {intl.formatMessage(i18n.noOthers)}
        </p>
      )}

      {candidates.length > 0 && (
        <ul className="flex flex-col gap-2" data-testid="add-chat-node-list">
          {candidates.map(({ node, glance, availability }) => (
            <CandidateRow
              key={node.def.id}
              id={node.def.id}
              name={node.def.name}
              kind={node.def.kind}
              glance={glance}
              availability={availability}
              busy={busy === node.def.id}
              disabled={busy != null}
              onAdd={() => void add(node.def.id)}
            />
          ))}
        </ul>
      )}

      {refusals.length > 0 && (
        <div role="alert" className="flex flex-col gap-1" data-testid="add-chat-node-refusals">
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.refused)}
          </span>
          {refusals.map((r) => (
            <p key={r} className={cx('break-words', TYPE.body)}>
              {r}
            </p>
          ))}
        </div>
      )}
    </OverlayDialog>
  );
}

function CandidateRow({
  id,
  name,
  kind,
  glance,
  availability,
  busy,
  disabled,
  onAdd,
}: {
  id: string;
  name: string;
  kind: GlancedNode['node']['def']['kind'];
  glance: GlancedNode['glance'];
  availability: ChatNodeAvailability;
  busy: boolean;
  disabled: boolean;
  onAdd: () => void;
}) {
  const intl = useIntl();
  const words = availability.addable
    ? addLineText(intl, availability.line)
    : notAddableText(intl, availability.reason);
  return (
    <li
      className={cx('flex items-start gap-3 p-3', SURFACE.card)}
      data-testid={`add-chat-node-${id}`}
      data-addable={availability.addable ? 'yes' : 'no'}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <KindChip kind={kind} />
          {glance.where && <WhereChip where={glance.where} />}
          <StateChip state={glance.state} />
        </div>
        <p className={cx('break-words', TYPE.body, WEIGHT.semibold)}>{name}</p>
        <p
          className={cx(
            'break-words',
            availability.addable ? TYPE.body : cx('text-lz-body', WEIGHT.semibold, TONE_TEXT.err)
          )}
          data-testid="add-chat-node-line"
        >
          {words}
        </p>
      </div>
      {availability.addable && (
        <Button
          variant="primary"
          size="sm"
          icon={busy ? <Loader2 className="animate-spin" /> : <Plus />}
          disabled={disabled}
          aria-label={intl.formatMessage(i18n.addNamed, { node: name })}
          onClick={onAdd}
          data-testid="add-chat-node-add"
        >
          {intl.formatMessage(i18n.add)}
        </Button>
      )}
    </li>
  );
}

/**
 * What the composer shows and does about the chat's loop (DESIGN-SESSION-LOOPS §7.1, §7.2, §8.1,
 * §5.3), as pure functions: the Loop slot (button or status chip), which turn in flight is a tick,
 * the refusal for a swarm-build chat, and the reply line of a `/loop` control sent straight to
 * goosed while a turn runs. The chip's words are the rail pill's (`pillView`) — one derivation, so
 * the composer and the rail never name a loop's state two ways.
 */
import type { IntlShape } from 'react-intl';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import { SWARM_PROVIDER_ID } from '../../branding';
import { parseRouteModel } from '../nodes/model';
import { lastTick, pillView, viewerOffsetMinutes, type PillView } from './loopView';
import { loopWords, sentenceMessage, sentenceValues } from './loopWords';
import {
  goalFirstLine,
  parseTickId,
  parseTime,
  clockTime,
  statusSentence,
  type LoopCommand,
  type LoopRecord,
  type LoopStatus,
  type LoopStatusReason,
} from './model';
import { composerWords as cw } from './startLoopWords';
import type { LoopControlNow } from './tickDoor';
import type { SessionLoop } from './useSessionLoop';

/**
 * The chat builds with the swarm (`swarm-build`, or a build strategy): every tick would start a
 * full build. The same test goosed's `chat_facts` makes (session_loops/acp.rs), so the button
 * refuses exactly the chats the start would refuse.
 */
export function isSwarmBuildChat(
  provider: string | null | undefined,
  model: string | null | undefined
): boolean {
  if (provider !== SWARM_PROVIDER_ID || !model) return false;
  const route = parseRouteModel(model);
  return route?.kind === 'build' || route?.kind === 'buildStrategy';
}

export type ComposerLoopSlot =
  /** The loop is still being read: nothing yet, rather than a button that may be wrong. */
  | { kind: 'none' }
  /** No loop (or an ended one the person has seen): the Loop button; a swarm-build chat refuses. */
  | { kind: 'button'; swarmBuild: boolean }
  /** A loop exists: its status as a solid chip that opens the rail on Loop. */
  | { kind: 'chip'; view: PillView };

export function composerLoopSlot(
  state: SessionLoop,
  facts: { swarmBuild: boolean; endedSeen: boolean; nowMs: number }
): ComposerLoopSlot {
  switch (state.kind) {
    case 'loading':
      return { kind: 'none' };
    case 'none':
      return { kind: 'button', swarmBuild: facts.swarmBuild };
    case 'unreadable':
      return { kind: 'chip', view: { tone: 'err', label: { message: loopWords.pillUnreadable } } };
    case 'loop':
      if (state.status === 'ended' && facts.endedSeen) {
        return { kind: 'button', swarmBuild: facts.swarmBuild };
      }
      return {
        kind: 'chip',
        view: pillView(state.loop, { status: state.status, reason: state.reason }, facts.nowMs),
      };
  }
}

/**
 * The turn in flight in this window is loop tick `n` — read from the transcript, the same place
 * the turn's own marker lives: the last message that opened a turn (a user message that is not a
 * steer and carries text) is the tick's marker. null = no turn runs here, or the person's own does.
 */
export function tickRunningHere(messages: readonly Message[], chatState: ChatState): number | null {
  if (chatState === ChatState.Idle || chatState === ChatState.LoadingConversation) return null;
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message.role !== 'user' || message.metadata?.steer) continue;
    const tick = message.metadata?.loopTick ?? (message.id ? parseTickId(message.id) : null);
    if (tick) return tick.n;
    if (message.content.some((content) => content.type === 'text')) return null;
  }
  return null;
}

/**
 * A tick goosed says is running while this window runs no turn, and this window never ran it —
 * the tick finishing on a connection this window lost (§5.1, a reload mid-tick). `ranHere` is the
 * last tick this window saw running, so the moment between a tick ending here and goosed's record
 * catching up is not mistaken for one.
 */
export function tickFinishingElsewhere(
  state: SessionLoop,
  chatState: ChatState,
  ranHere: number | null
): number | null {
  if (state.kind !== 'loop' || state.status !== 'running' || chatState !== ChatState.Idle) {
    return null;
  }
  const last = lastTick(state.loop);
  if (!last || last.endedAt || last.n === ranHere) return null;
  return last.n;
}

/** The reply line of a `/loop` control goosed answered directly — the words L3's `/loop` says. */
export interface LoopReplyLine {
  text: string;
  refused: boolean;
}

function nowSentence(
  intl: IntlShape,
  record: LoopRecord,
  status: LoopStatus,
  reason: LoopStatusReason | null | undefined,
  nowMs: number
): string {
  const got = statusSentence(record, status, reason, nowMs, viewerOffsetMinutes(nowMs));
  if (!got.ok) return intl.formatMessage(cw.replyStatusUnreadable, { error: got.error });
  const message = sentenceMessage(got.value);
  return message ? intl.formatMessage(message, sentenceValues(got.value)) : got.value.text;
}

function afterTick(intl: IntlShape, record: LoopRecord, verb: 'paused' | 'stopped'): string {
  const last = lastTick(record);
  if (verb === 'paused') {
    return last
      ? intl.formatMessage(cw.replyPausedAfter, { n: last.n })
      : intl.formatMessage(cw.replyPausedBefore);
  }
  return last
    ? intl.formatMessage(cw.replyStoppedAfter, { n: last.n })
    : intl.formatMessage(cw.replyStoppedBefore);
}

/**
 * Mirrors `execute_commands.rs`'s `loop_status_reply` / `loop_change_reply`, so a `/loop` control
 * answers the same words whether it went through the prompt (between turns) or straight to
 * `loops/*` (during one).
 */
export function loopReplyLine(
  intl: IntlShape,
  command: LoopCommand,
  answer: LoopControlNow,
  nowMs: number
): LoopReplyLine {
  if (answer.kind === 'status') {
    const got = answer.result;
    if (got.error) return { text: got.error, refused: true };
    const record = got.loop;
    if (!record) {
      return {
        text: intl.formatMessage(cw.replyNoLoop, { command: '/loop <goal>' }),
        refused: false,
      };
    }
    const status = got.effectiveStatus ?? record.status;
    const reason = got.effectiveStatus ? got.effectiveReason : record.statusReason;
    const goal = goalFirstLine(record.goal);
    const sentence = nowSentence(intl, record, status, reason, nowMs);
    const last = lastTick(record);
    return {
      text: last
        ? intl.formatMessage(cw.replyStatusTick, { goal, sentence, n: last.n })
        : intl.formatMessage(cw.replyStatus, { goal, sentence }),
      refused: false,
    };
  }
  const changed = answer.result;
  if (changed.refusal) return { text: changed.refusal.reason, refused: true };
  const record = changed.loop;
  if (!record) return { text: intl.formatMessage(cw.replyNeither), refused: true };
  const sentence = () => nowSentence(intl, record, record.status, record.statusReason, nowMs);
  switch (command.kind) {
    case 'now':
      return {
        text: record.offer
          ? intl.formatMessage(cw.replyTickNow, { n: record.offer.n })
          : sentence(),
        refused: false,
      };
    case 'pause':
      return { text: afterTick(intl, record, 'paused'), refused: false };
    case 'stop':
      return { text: afterTick(intl, record, 'stopped'), refused: false };
    case 'resume': {
      if (record.status !== 'waiting' || !record.nextTick) {
        return {
          text: intl.formatMessage(cw.replyResumedSentence, { sentence: sentence() }),
          refused: false,
        };
      }
      const at = parseTime(record.nextTick.at);
      const time = at.ok ? clockTime(at.value, viewerOffsetMinutes(at.value)) : at;
      return {
        text: time.ok
          ? intl.formatMessage(cw.replyResumedAt, { time: time.value })
          : intl.formatMessage(cw.replyResumedUnreadable, { error: time.error }),
        refused: false,
      };
    }
    default:
      return { text: sentence(), refused: false };
  }
}

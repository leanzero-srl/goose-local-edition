import type { LoopRefuseReason, LoopsTickDueNotification_unstable } from '@aaif/goose-sdk';
import type { AcpSubmitMessageOptions, AcpSubmitStatus } from '../../acp/chatSessionController';
import type { AcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import type { LoopsChange, LoopsGet } from '../../acp/loops';
import { ChatState } from '../../types/chatState';
import type { Message, MessageMetadata } from '../../types/message';
import { controlAction, isControlCommand, parseLoopCommand, parseTickId } from './model';

/**
 * The renderer half of the tick door (DESIGN-SESSION-LOOPS §5.1, §9 L4r). goosed's runner OFFERS a
 * tick (`loops/tickDue`); this window either submits it through `submitMessage` — the door a typed
 * message uses, carrying `_meta.goose.loopTick` so goosed can match it to the open offer — or
 * answers `loops/tickRefused{reason}`. The runner owns every decision; this only fires or refuses,
 * and never drops an offer without saying which.
 */

export type TickOffer = LoopsTickDueNotification_unstable;

/** What clears a refusal, so the window can send `loops/ready`. `null`: only the runner knows. */
export type RefusalWaitsOn = 'turn' | 'session' | null;

export type TickDoorOutcome =
  | { kind: 'submitted' }
  | { kind: 'duplicate' }
  | { kind: 'refused'; reason: LoopRefuseReason; waitsOn: RefusalWaitsOn }
  /** The tick's turn started here and then failed; goosed recorded it, nothing is refused. */
  | { kind: 'failed'; error: string };

export interface TickDoorDeps {
  getSnapshot(sessionId: string): AcpChatSessionSnapshot | undefined;
  loadSession(sessionId: string): Promise<void>;
  submitMessage(
    sessionId: string,
    message: Message,
    options: AcpSubmitMessageOptions
  ): Promise<AcpSubmitStatus>;
  setMessages(sessionId: string, messages: Message[]): void;
  pendingUserInput(sessionId: string): number;
  tickRefused(
    sessionId: string,
    loopId: string,
    n: number,
    reason: LoopRefuseReason
  ): Promise<void>;
}

export interface TickRef {
  loopId: string;
  n: number;
  messageId: string;
}

/** The tick's marker: its prompt as a user message with the runner's id and `metadata.loopTick`. */
export function tickMarker(offer: TickOffer): Message & { id: string } {
  const loopTick: TickRef = { loopId: offer.loopId, n: offer.n, messageId: offer.messageId };
  const metadata: MessageMetadata & { loopTick: TickRef } = {
    userVisible: true,
    agentVisible: true,
    loopTick,
  };
  return {
    id: offer.messageId,
    role: 'user',
    created: Math.floor(Date.now() / 1000),
    content: [{ type: 'text', text: offer.prompt }],
    metadata,
  };
}

export function tickMeta(offer: TickOffer): Record<string, unknown> {
  return {
    goose: { loopTick: { loopId: offer.loopId, n: offer.n, messageId: offer.messageId } },
  };
}

function busyReason(snapshot: AcpChatSessionSnapshot): LoopRefuseReason | null {
  if (snapshot.pendingCancelPromptAttemptId) return { kind: 'pending_cancel' };
  if (snapshot.activePromptAttemptId || snapshot.chatState !== ChatState.Idle) {
    return { kind: 'turn_running' };
  }
  return null;
}

/** Whether a refusal that waits on `waitsOn` has cleared for this chat. */
export function refusalCleared(
  waitsOn: Exclude<RefusalWaitsOn, null>,
  snapshot: AcpChatSessionSnapshot | undefined,
  queued: number
): boolean {
  if (waitsOn === 'session') return Boolean(snapshot?.session) && !snapshot?.sessionLoadError;
  if (queued > 0) return false;
  return !snapshot || busyReason(snapshot) === null;
}

export interface TickDoor {
  offer(offer: TickOffer): Promise<TickDoorOutcome>;
}

export function createTickDoor(deps: TickDoorDeps): TickDoor {
  // Offers this window is submitting or has submitted, by the runner's message id: a repeated
  // `tickDue` for the same (loopId, n, messageId) submits once. A refused or failed offer leaves
  // the set, so the runner's re-offer of it is taken.
  const taken = new Set<string>();

  async function refuse(
    offer: TickOffer,
    reason: LoopRefuseReason,
    waitsOn: RefusalWaitsOn
  ): Promise<TickDoorOutcome> {
    taken.delete(offer.messageId);
    try {
      await deps.tickRefused(offer.sessionId, offer.loopId, offer.n, reason);
    } catch (error) {
      console.error(
        `Could not tell goose that loop tick ${offer.n} of ${offer.sessionId} was refused (${reason.kind}):`,
        error
      );
    }
    return { kind: 'refused', reason, waitsOn };
  }

  function withdrawMarkerIfAlone(offer: TickOffer): boolean {
    const messages = deps.getSnapshot(offer.sessionId)?.messages ?? [];
    if (messages[messages.length - 1]?.id !== offer.messageId) return false;
    deps.setMessages(offer.sessionId, messages.slice(0, -1));
    return true;
  }

  async function offer(tick: TickOffer): Promise<TickDoorOutcome> {
    if (taken.has(tick.messageId)) return { kind: 'duplicate' };
    taken.add(tick.messageId);

    const parsed = parseTickId(tick.messageId);
    if (!parsed || parsed.loopId !== tick.loopId || parsed.n !== tick.n) {
      console.error(
        `loops/tickDue carries message id ${tick.messageId}, which is not tick ${tick.n} of ${tick.loopId}`
      );
    }

    let snapshot = deps.getSnapshot(tick.sessionId);
    if (!snapshot?.session) {
      await deps.loadSession(tick.sessionId);
      snapshot = deps.getSnapshot(tick.sessionId);
      if (!snapshot?.session) {
        const error = snapshot?.sessionLoadError ?? 'the chat did not load into this window';
        return refuse(tick, { kind: 'load_failed', error }, 'session');
      }
    }

    const busy = busyReason(snapshot);
    if (busy) return refuse(tick, busy, 'turn');
    if (deps.pendingUserInput(tick.sessionId) > 0) {
      return refuse(tick, { kind: 'queued_message' }, 'turn');
    }

    let failure: string | undefined;
    let status: AcpSubmitStatus;
    try {
      status = await deps.submitMessage(tick.sessionId, tickMarker(tick), {
        getCurrentSnapshot: () => deps.getSnapshot(tick.sessionId),
        onFinish: (error) => {
          failure = error;
        },
        meta: tickMeta(tick),
        preAppend: true,
      });
    } catch (error) {
      status = 'submitted';
      failure = error instanceof Error ? error.message : String(error);
    }

    if (status === 'busy') return refuse(tick, { kind: 'turn_running' }, 'turn');
    if (failure === undefined) return { kind: 'submitted' };

    // Nothing followed the marker: the prompt never ran here (goosed refused it, or the socket
    // dropped before any reply), so the marker is withdrawn and the refusal carries goosed's words.
    // Anything after it means the tick's turn ran, and goosed records how it ended.
    if (withdrawMarkerIfAlone(tick)) {
      return refuse(tick, { kind: 'submit_failed', error: failure }, null);
    }
    taken.delete(tick.messageId);
    return { kind: 'failed', error: failure };
  }

  return { offer };
}

export type LoopControlNow =
  | { kind: 'status'; result: LoopsGet }
  | { kind: 'control'; result: LoopsChange };

export interface LoopControlApi {
  loopsGet(sessionId: string): Promise<LoopsGet>;
  loopsControl(
    sessionId: string,
    action: NonNullable<ReturnType<typeof controlAction>>
  ): Promise<LoopsChange>;
}

/**
 * A `/loop` CONTROL form (`/loop`, `/loop now|pause|resume|stop`) goes to goosed at once — it never
 * waits in the composer's queue behind the very tick it means to stop (§7.2, §13 item 4). Any
 * other line answers `null` and takes the ordinary path (queued while a turn runs).
 */
export async function sendLoopControlNow(
  sessionId: string,
  line: string,
  api: LoopControlApi
): Promise<LoopControlNow | null> {
  const command = parseLoopCommand(line);
  if (!command || !isControlCommand(command)) return null;
  const action = controlAction(command);
  if (action === null) return { kind: 'status', result: await api.loopsGet(sessionId) };
  return { kind: 'control', result: await api.loopsControl(sessionId, action) };
}

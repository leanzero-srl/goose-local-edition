import type { AcpSubmitMessageOptions, AcpSubmitStatus } from '../../acp/chatSessionController';
import type { AcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import { crossNoteMeta } from '../../acp/notes';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';

/**
 * A note's own turn (Q-358): the window submits the note's exact words as a user message under the
 * note's message id, marked `_meta.goose.crossNote`, through the door a typed message uses — so the
 * reply streams where the person sees it. goosed offers a due note (`notes/deliverDue`) only to the
 * windows that show its chat; one that is not shown here is left alone, and one that is busy here
 * (still loading, a turn running, input queued) is left for goosed to offer again — when the
 * chat's turn ends, or when this window, the chat idle at last, says again that it shows it (Q-488).
 * "Give it to goose now" on the inbox tray is the same door, opened by the person's click.
 */

export interface NoteTurn {
  sessionId: string;
  noteId: string;
  messageId: string;
  prompt: string;
}

export type NoteDoorOutcome =
  | { kind: 'submitted' }
  | { kind: 'duplicate' }
  /** The window does not show the chat: another window, or a later showing, takes it. */
  | { kind: 'not_here' }
  /**
   * Not loaded yet, a turn runs, or the person's own input waits: the driver asks goosed to offer
   * it again the moment that clears (a turn's end alone would miss a chat that was only loading).
   */
  | { kind: 'busy' }
  /** goosed refused it (taken already, no longer waiting): nothing ran, the marker is withdrawn. */
  | { kind: 'refused'; error: string }
  /** Its turn started and then failed; goosed recorded how. */
  | { kind: 'failed'; error: string };

export interface NoteDoorDeps {
  getSnapshot(sessionId: string): AcpChatSessionSnapshot | undefined;
  submitMessage(
    sessionId: string,
    message: Message,
    options: AcpSubmitMessageOptions
  ): Promise<AcpSubmitStatus>;
  setMessages(sessionId: string, messages: Message[]): void;
  pendingUserInput(sessionId: string): number;
  isShownHere(sessionId: string): boolean;
}

/** The note's marker: its words as a user message under the note's own id. */
export function noteTurnMessage(turn: NoteTurn): Message & { id: string } {
  return {
    id: turn.messageId,
    role: 'user',
    created: Math.floor(Date.now() / 1000),
    content: [{ type: 'text', text: turn.prompt }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

/**
 * The chat cannot take a note's turn here now: not loaded yet (a window that has just opened the
 * chat is still reading it), a turn runs, or the person's own input waits.
 */
export function noteDoorBusy(
  snapshot: AcpChatSessionSnapshot | undefined,
  queued: number
): boolean {
  if (!snapshot?.session) return true;
  if (queued > 0) return true;
  return Boolean(
    snapshot.pendingCancelPromptAttemptId ||
    snapshot.activePromptAttemptId ||
    snapshot.chatState !== ChatState.Idle
  );
}

export interface NoteDoor {
  /** `fromOffer`: goosed's offer, taken only by a window that shows the chat. */
  open(turn: NoteTurn, fromOffer: boolean): Promise<NoteDoorOutcome>;
}

export function createNoteDoor(deps: NoteDoorDeps): NoteDoor {
  // Turns this window is submitting, by message id: a repeated offer submits once. A refused or
  // failed turn leaves the set, so goosed's next offer of it is taken.
  const taken = new Set<string>();

  function withdrawIfAlone(turn: NoteTurn): boolean {
    const messages = deps.getSnapshot(turn.sessionId)?.messages ?? [];
    if (messages[messages.length - 1]?.id !== turn.messageId) return false;
    deps.setMessages(turn.sessionId, messages.slice(0, -1));
    return true;
  }

  async function open(turn: NoteTurn, fromOffer: boolean): Promise<NoteDoorOutcome> {
    if (fromOffer && !deps.isShownHere(turn.sessionId)) return { kind: 'not_here' };
    if (taken.has(turn.messageId)) return { kind: 'duplicate' };
    if (noteDoorBusy(deps.getSnapshot(turn.sessionId), deps.pendingUserInput(turn.sessionId))) {
      return { kind: 'busy' };
    }
    taken.add(turn.messageId);

    let failure: string | undefined;
    let status: AcpSubmitStatus;
    try {
      status = await deps.submitMessage(turn.sessionId, noteTurnMessage(turn), {
        getCurrentSnapshot: () => deps.getSnapshot(turn.sessionId),
        onFinish: (error) => {
          failure = error;
        },
        meta: crossNoteMeta(turn.noteId, turn.messageId),
        preAppend: true,
      });
    } catch (error) {
      status = 'submitted';
      failure = error instanceof Error ? error.message : String(error);
    }

    if (status === 'busy') {
      taken.delete(turn.messageId);
      return { kind: 'busy' };
    }
    if (failure === undefined) return { kind: 'submitted' };
    taken.delete(turn.messageId);
    // Nothing followed the marker: goosed refused the turn, so the marker goes and goosed's words
    // say why. Anything after it means the turn ran, and goosed recorded how it ended.
    if (withdrawIfAlone(turn)) return { kind: 'refused', error: failure };
    return { kind: 'failed', error: failure };
  }

  return { open };
}

import { useCallback, useEffect, useState } from 'react';
import { onNotesDeliverDue } from '../../acp/acpConnection';
import { acpChatSessionController } from '../../acp/chatSessionController';
import {
  acpChatSessionActions,
  acpChatSessionStore,
  useAcpChatSessionSnapshot,
} from '../../acp/chatSessionStore';
import { getPendingUserInput, usePendingUserInput } from '../loops/pendingUserInput';
import {
  createNoteDoor,
  noteDoorBusy,
  type NoteDoor,
  type NoteDoorDeps,
  type NoteTurn,
} from './noteDoor';
import { askForDueNotes, isShownHere } from './notesStore';

const liveDeps: NoteDoorDeps = {
  getSnapshot: (sessionId) => acpChatSessionStore.getSnapshot(sessionId),
  submitMessage: (sessionId, message, options) =>
    acpChatSessionController.submitMessage(sessionId, message, options),
  setMessages: (sessionId, messages) => {
    acpChatSessionActions.setMessages(sessionId, messages);
  },
  pendingUserInput: getPendingUserInput,
  isShownHere,
};

/** One door per window, shared by goosed's offers and the tray's "Give it to goose now". */
const liveDoor: NoteDoor = createNoteDoor(liveDeps);

/** The person's click on "Give it to goose now": the note's own turn, in this chat, now. */
export function giveNoteToGoose(turn: NoteTurn) {
  return liveDoor.open(turn, false);
}

/** Watches one chat whose offer found it busy here; says so the moment it can take a turn. */
function IdleWatcher({
  sessionId,
  onIdle,
}: {
  sessionId: string;
  onIdle(sessionId: string): void;
}) {
  const snapshot = useAcpChatSessionSnapshot(sessionId);
  const queued = usePendingUserInput(sessionId);
  const idle = !noteDoorBusy(snapshot, queued);

  useEffect(() => {
    if (idle) onIdle(sessionId);
  }, [idle, onIdle, sessionId]);

  return null;
}

/**
 * The window's hands for notes to another chat (Q-358), mounted once per window beside the loop
 * driver: goosed's `notes/deliverDue` offers are opened here when this window shows the chat and
 * the chat is idle. An offer that finds the chat busy here — still loading in a window that has
 * just opened it, a turn running, the person's input queued — is watched, and the moment the chat
 * turns idle this window asks goosed to offer it again (Q-488: a turn's end is not the only way a
 * chat turns idle, and a chat that was only loading never ends a turn). A store event, never a timer.
 */
export function NotesDriver({
  door = liveDoor,
  offerAgain = askForDueNotes,
}: {
  door?: NoteDoor;
  offerAgain?: (sessionId: string) => void;
}) {
  const [waitingForIdle, setWaitingForIdle] = useState<ReadonlySet<string>>(() => new Set());

  useEffect(
    () =>
      onNotesDeliverDue((due) => {
        void door.open(due, true).then((outcome) => {
          if (outcome.kind === 'busy') {
            setWaitingForIdle((current) =>
              current.has(due.sessionId) ? current : new Set(current).add(due.sessionId)
            );
            return;
          }
          if (outcome.kind === 'refused' || outcome.kind === 'failed') {
            console.error(
              `The note ${due.noteId} offered for ${due.sessionId} did not run here: ${outcome.error}`
            );
          }
        });
      }),
    [door]
  );

  const onIdle = useCallback(
    (sessionId: string) => {
      setWaitingForIdle((current) => {
        if (!current.has(sessionId)) return current;
        const next = new Set(current);
        next.delete(sessionId);
        return next;
      });
      offerAgain(sessionId);
    },
    [offerAgain]
  );

  return (
    <>
      {[...waitingForIdle].map((sessionId) => (
        <IdleWatcher key={sessionId} sessionId={sessionId} onIdle={onIdle} />
      ))}
    </>
  );
}

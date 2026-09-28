import { useEffect } from 'react';
import { onNotesDeliverDue } from '../../acp/acpConnection';
import { acpChatSessionController } from '../../acp/chatSessionController';
import { acpChatSessionActions, acpChatSessionStore } from '../../acp/chatSessionStore';
import { getPendingUserInput } from '../loops/pendingUserInput';
import { createNoteDoor, type NoteDoor, type NoteDoorDeps, type NoteTurn } from './noteDoor';
import { isShownHere } from './notesStore';

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

/**
 * The window's hands for notes to another chat (Q-358), mounted once per window beside the loop
 * driver: goosed's `notes/deliverDue` offers are opened here when this window shows the chat and
 * the chat is idle. Anything else leaves the offer for goosed to make again — when the chat's turn
 * ends, or a window says it shows the chat — so nothing is dropped and nothing is guessed.
 */
export function NotesDriver({ door = liveDoor }: { door?: NoteDoor }) {
  useEffect(
    () =>
      onNotesDeliverDue((due) => {
        void door.open(due, true).then((outcome) => {
          if (outcome.kind === 'refused' || outcome.kind === 'failed') {
            console.error(
              `The note ${due.noteId} offered for ${due.sessionId} did not run here: ${outcome.error}`
            );
          }
        });
      }),
    [door]
  );
  return null;
}

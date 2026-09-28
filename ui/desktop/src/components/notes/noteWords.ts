import { defineMessages } from '../../i18n';

/** Every word the notes surfaces show (Q-358, DESIGN-Q358-TRANSCRIPTS §3), in one place. */
export const noteWords = defineMessages({
  draftBand: { id: 'notes.draft.band', defaultMessage: 'Note to another chat' },
  liveWorking: { id: 'notes.draft.liveWorking', defaultMessage: 'goose is working there' },
  liveIdle: { id: 'notes.draft.liveIdle', defaultMessage: 'idle since {time}' },
  liveNotOpen: { id: 'notes.draft.liveNotOpen', defaultMessage: 'not open in any window' },
  steerNow: { id: 'notes.draft.steerNow', defaultMessage: 'Steer it now' },
  leaveThere: { id: 'notes.draft.leaveThere', defaultMessage: 'Leave it there' },
  notThisChat: { id: 'notes.draft.notThisChat', defaultMessage: 'Not this chat' },
  cancel: { id: 'notes.draft.cancel', defaultMessage: 'Cancel' },
  textLabel: { id: 'notes.draft.textLabel', defaultMessage: 'The note' },
  ambiguous: {
    id: 'notes.draft.ambiguous',
    defaultMessage: 'Several chats match "{query}". Which one?',
  },
  noMatch: {
    id: 'notes.draft.noMatch',
    defaultMessage: 'No chat matches "{query}". Pick the chat this note is for.',
  },
  pickTitle: { id: 'notes.draft.pickTitle', defaultMessage: 'Pick the chat' },
  pickFilter: {
    id: 'notes.draft.pickFilter',
    defaultMessage: 'Find a chat by its title or folder',
  },
  pickNone: { id: 'notes.draft.pickNone', defaultMessage: 'No chat matches these words.' },
  pickLoading: { id: 'notes.draft.pickLoading', defaultMessage: 'Reading your chats…' },
  pickBack: { id: 'notes.draft.pickBack', defaultMessage: 'Back' },
  sentWaiting: {
    id: 'notes.sent.waiting',
    defaultMessage: 'Note sent to "{name}" · waiting there',
  },
  sentRead: {
    id: 'notes.sent.read',
    defaultMessage: 'Note sent to "{name}" · read in its turn at {time}',
  },
  sentDismissed: {
    id: 'notes.sent.dismissed',
    defaultMessage: 'Note sent to "{name}" · dismissed there',
  },
  sentGone: {
    id: 'notes.sent.gone',
    defaultMessage: 'Note sent to "{name}" · {reason}',
  },
  closeSent: { id: 'notes.sent.close', defaultMessage: 'Close this line' },
  inboxBand: { id: 'notes.inbox.band', defaultMessage: 'Note from "{name}"' },
  inboxUnread: {
    id: 'notes.inbox.unread',
    defaultMessage: 'Sent from your other chat. goose has not read it yet.',
  },
  giveNow: { id: 'notes.inbox.giveNow', defaultMessage: 'Give it to goose now' },
  addToNext: { id: 'notes.inbox.addToNext', defaultMessage: 'Add to my next message' },
  dismiss: { id: 'notes.inbox.dismiss', defaultMessage: 'Dismiss' },
  steerThisTurn: { id: 'notes.inbox.steerThisTurn', defaultMessage: 'Steer this turn' },
  afterThisTurn: { id: 'notes.inbox.afterThisTurn', defaultMessage: 'After this turn' },
  steering: {
    id: 'notes.inbox.steering',
    defaultMessage: 'Steered into this turn: goose reads it between tool calls.',
  },
  withNextMessage: {
    id: 'notes.inbox.withNextMessage',
    defaultMessage: 'Goes with your next message.',
  },
  afterTurnWaiting: {
    id: 'notes.inbox.afterTurnWaiting',
    defaultMessage: 'goose reads it when this turn ends.',
  },
  failed: { id: 'notes.failed', defaultMessage: 'Could not do that: {error}' },
  unreadable: {
    id: 'notes.unreadable',
    defaultMessage: "This chat's notes could not be read: {error}",
  },
  markerTitle: { id: 'notes.marker.title', defaultMessage: 'Note from "{name}" · {time}' },
  markerShow: { id: 'notes.marker.show', defaultMessage: 'Show note' },
  markerHide: { id: 'notes.marker.hide', defaultMessage: 'Hide note' },
  chip: { id: 'notes.chip', defaultMessage: 'Note' },
  chipLabel: {
    id: 'notes.chipLabel',
    defaultMessage:
      '{count, plural, one {A note from "{from}" is waiting in this chat} other {# notes are waiting in this chat}}',
  },
  waiting: {
    id: 'notes.waiting',
    defaultMessage: '{count, plural, one {# note waiting} other {# notes waiting}}',
  },
  waitingMenu: { id: 'notes.waitingMenu', defaultMessage: 'Notes waiting for you' },
  waitingFrom: { id: 'notes.waitingFrom', defaultMessage: 'from "{from}"' },
});

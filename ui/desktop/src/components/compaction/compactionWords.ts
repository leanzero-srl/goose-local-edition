import { defineMessages } from '../../i18n';

/**
 * The words of a chat's compaction (Q-357), in one place: the card, the meter menu and the Context
 * tab say the same thing the same way.
 */
export const compactionWords = defineMessages({
  noteLabel: {
    id: 'compaction.noteLabel',
    defaultMessage: 'Note for the next compaction',
  },
  notePlaceholder: {
    id: 'compaction.notePlaceholder',
    defaultMessage: 'What must survive — a rule, a number, a decision',
  },
  standing: {
    id: 'compaction.standing',
    defaultMessage: 'Use this note for every compaction in this chat',
  },
  compactNow: { id: 'compaction.compactNow', defaultMessage: 'Compact now' },
  seeWhatItKeeps: { id: 'compaction.seeWhatItKeeps', defaultMessage: 'See what it keeps' },
  noteSaveFailed: {
    id: 'compaction.noteSaveFailed',
    defaultMessage: 'Couldn’t save the note: {error}',
  },
  noteReadFailed: {
    id: 'compaction.noteReadFailed',
    defaultMessage: 'Couldn’t read the note saved for this chat: {error}',
  },
  saving: { id: 'compaction.saving', defaultMessage: 'Saving…' },
  cancel: { id: 'compaction.cancel', defaultMessage: 'Cancel' },
});

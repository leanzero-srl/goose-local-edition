import { useEffect, useId, useState } from 'react';
import { useIntl } from '../../i18n';
import { Button, Checkbox, FOCUS, RADIUS, TYPE, TONE_TEXT, WEIGHT, cx, SURFACE } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { compactionWords } from './compactionWords';

export const NOTE_FIELD = cx(
  'w-full resize-y border border-lz-border-strong bg-lz-surface px-2.5 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
  RADIUS.control,
  FOCUS
);

/**
 * The note for the next compaction and whether it stands for every compaction of the chat — one
 * editor for the meter menu and the card's dialog. It holds the person's words until they save;
 * saving is the caller's (`useCompactionSteer`).
 */
export function NoteFields({
  note,
  standing,
  onNote,
  onStanding,
  rows = 3,
  autoFocus = false,
  testIdPrefix,
}: {
  note: string;
  standing: boolean;
  onNote: (note: string) => void;
  onStanding: (standing: boolean) => void;
  rows?: number;
  autoFocus?: boolean;
  testIdPrefix: string;
}) {
  const intl = useIntl();
  const id = useId();
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={id} className={cx(TYPE.meta, WEIGHT.semibold, 'text-lz-ink-2')}>
        {intl.formatMessage(compactionWords.noteLabel)}
      </label>
      <textarea
        id={id}
        data-testid={`${testIdPrefix}-note`}
        value={note}
        rows={rows}
        autoFocus={autoFocus}
        placeholder={intl.formatMessage(compactionWords.notePlaceholder)}
        onChange={(e) => onNote(e.target.value)}
        className={NOTE_FIELD}
      />
      <Checkbox
        checked={standing}
        onChange={onStanding}
        label={intl.formatMessage(compactionWords.standing)}
        testId={`${testIdPrefix}-standing`}
      />
    </div>
  );
}

/**
 * The note in a dialog (the card's "Edit note"): the person edits it, then compacts under it.
 */
export function NoteDialog({
  open,
  title,
  initialNote,
  initialStanding,
  saveError,
  confirmLabel,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: string;
  initialNote: string;
  initialStanding: boolean;
  saveError: string | null;
  confirmLabel: string;
  onConfirm: (note: string, standing: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const intl = useIntl();
  const [note, setNote] = useState(initialNote);
  const [standing, setStanding] = useState(initialStanding);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) {
      setNote(initialNote);
      setStanding(initialStanding);
    }
  }, [open, initialNote, initialStanding]);
  return (
    <OverlayDialog
      open={open}
      onClose={onClose}
      panelClassName={cx('flex w-[30rem] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <OverlayDialogTitle className={cx(TYPE.body, WEIGHT.semibold)}>{title}</OverlayDialogTitle>
      <NoteFields
        note={note}
        standing={standing}
        onNote={setNote}
        onStanding={setStanding}
        rows={4}
        autoFocus
        testIdPrefix="compaction-note-dialog"
      />
      {saveError && (
        <p className={cx(TYPE.meta, TONE_TEXT.err)} data-testid="compaction-note-dialog-error">
          {intl.formatMessage(compactionWords.noteSaveFailed, { error: saveError })}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onClose}>
          {intl.formatMessage(compactionWords.cancel)}
        </Button>
        <Button
          variant="primary"
          disabled={busy}
          data-testid="compaction-note-dialog-confirm"
          onClick={async () => {
            setBusy(true);
            try {
              await onConfirm(note, standing);
            } finally {
              setBusy(false);
            }
          }}
        >
          {confirmLabel}
        </Button>
      </div>
    </OverlayDialog>
  );
}

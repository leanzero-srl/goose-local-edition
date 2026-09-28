import { useEffect, useState } from 'react';
import { Eye, ScrollText } from 'lucide-react';
import { useIntl } from '../../i18n';
import { Button, TONE_TEXT, TYPE, cx } from '../lz';
import { requestOpenContextRail } from '../contextRail/contextRailRequest';
import { compactionWords } from './compactionWords';
import { NoteFields } from './NoteEditor';
import { useCompactionSteer } from './useCompactionSteer';

/**
 * The meter menu's compaction part (Q-357 STEER): the note for the next compaction, whether it
 * stands for every compaction of the chat, Compact now, and the way into what a compaction keeps.
 * The note is saved when the person leaves the field or ticks the box, and before Compact now — a
 * note typed and then compacted is never lost.
 */
export function CompactionMenu({
  sessionId,
  compactDisabled,
  onCompact,
}: {
  sessionId: string;
  compactDisabled: boolean;
  onCompact: () => void;
}) {
  const intl = useIntl();
  const { state, save, saveError } = useCompactionSteer(sessionId);
  const saved = state.kind === 'ready' ? state.steer : null;
  const [draft, setDraft] = useState<string | null>(null);
  const note = draft ?? saved?.note ?? '';

  useEffect(() => {
    setDraft(null);
  }, [saved?.note]);

  const saveNote = async () => {
    if (draft === null || draft === (saved?.note ?? '')) return true;
    return save({ note: draft, followAsWritten: false });
  };

  return (
    <div
      data-testid="compaction-menu"
      className="flex flex-col gap-3 bg-lz-surface px-3 py-3 text-lz-ink"
      onMouseDown={(e) => e.stopPropagation()}
    >
      {state.kind === 'unreadable' ? (
        <p className={cx(TYPE.meta, TONE_TEXT.err)} data-testid="compaction-menu-unreadable">
          {intl.formatMessage(compactionWords.noteReadFailed, { error: state.error })}
        </p>
      ) : (
        <div onBlur={() => void saveNote()}>
          <NoteFields
            note={note}
            standing={saved?.standing ?? false}
            onNote={setDraft}
            onStanding={(standing) => {
              void save({ note, standing });
            }}
            testIdPrefix="compaction-menu"
          />
        </div>
      )}
      {state.kind === 'ready' && state.readError && (
        <p className={cx(TYPE.meta, TONE_TEXT.warn)}>
          {intl.formatMessage(compactionWords.noteReadFailed, { error: state.readError })}
        </p>
      )}
      {saveError && (
        <p className={cx(TYPE.meta, TONE_TEXT.err)} data-testid="compaction-menu-save-error">
          {intl.formatMessage(compactionWords.noteSaveFailed, { error: saveError })}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="primary"
          icon={<ScrollText />}
          disabled={compactDisabled}
          data-testid="compaction-menu-compact"
          onClick={async (e) => {
            e.stopPropagation();
            if (await saveNote()) onCompact();
          }}
        >
          {intl.formatMessage(compactionWords.compactNow)}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          icon={<Eye />}
          data-testid="compaction-menu-see-kept"
          onClick={async (e) => {
            e.stopPropagation();
            await saveNote();
            requestOpenContextRail(sessionId);
          }}
        >
          {intl.formatMessage(compactionWords.seeWhatItKeeps)}
        </Button>
      </div>
    </div>
  );
}

import { useState } from 'react';
import { Mail, X } from 'lucide-react';
import { useIntl } from '../../i18n';
import { notesInbox, type InboxNoteDto, type NoteInboxAction } from '../../acp/notes';
import { ChatState } from '../../types/chatState';
import { Button, RADIUS, TNUM, TYPE, WEIGHT, cx } from '../lz';
import { NOTE_BAND, NOTE_CARD, clockOf } from './NoteDraftCard';
import { noteWords as w } from './noteWords';
import { giveNoteToGoose } from './NotesDriver';
import { refreshAfter, useChatNotes } from './notesStore';
import type { NoteDoorOutcome } from './noteDoor';

/** The note's own turn in `sessionId`, opened by the person's click. */
export type Give = (sessionId: string, note: InboxNoteDto) => Promise<NoteDoorOutcome>;

const giveLive: Give = (sessionId, note) =>
  giveNoteToGoose({
    sessionId,
    noteId: note.id,
    messageId: note.messageId,
    prompt: note.prompt,
  });

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function NoteInboxCard({
  sessionId,
  note,
  busy,
  give,
}: {
  sessionId: string;
  note: InboxNoteDto;
  busy: boolean;
  give: Give;
}) {
  const intl = useIntl();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const act = async (run: () => Promise<unknown>) => {
    setSaving(true);
    setError(null);
    try {
      await run();
      refreshAfter([sessionId, note.fromSessionId]);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };
  const inbox = (action: NoteInboxAction) => void act(() => notesInbox(sessionId, note.id, action));
  const giveNow = () =>
    void act(async () => {
      const outcome = await give(sessionId, note);
      if (outcome.kind === 'refused' || outcome.kind === 'failed') throw new Error(outcome.error);
    });

  const status =
    note.status === 'steering'
      ? intl.formatMessage(w.steering)
      : note.status === 'with_next_message'
        ? intl.formatMessage(w.withNextMessage)
        : busy && note.offerWhenIdle
          ? intl.formatMessage(w.afterTurnWaiting)
          : null;
  const open = note.status === 'waiting';

  return (
    <section
      data-testid="note-inbox-card"
      data-note-id={note.id}
      data-status={note.status}
      aria-label={intl.formatMessage(w.inboxBand, { name: note.fromName })}
      className={cx(NOTE_CARD, RADIUS.card, 'shrink-0')}
    >
      <div className={NOTE_BAND}>
        <Mail aria-hidden />
        <span className={cx('min-w-0 truncate text-lz-body', WEIGHT.semibold)}>
          {intl.formatMessage(w.inboxBand, { name: note.fromName })}
        </span>
        <span className={cx('shrink-0 text-lz-meta', TNUM)}>· {clockOf(note.sentAt)}</span>
      </div>
      <div className="flex flex-col gap-2 px-4 pb-3 pt-3">
        <p className={cx(TYPE.bodyMuted, 'italic')}>{intl.formatMessage(w.inboxUnread)}</p>
        <p
          data-testid="note-inbox-text"
          className="whitespace-pre-wrap break-words text-lz-body text-lz-ink"
        >
          {note.text}
        </p>
        <p className={TYPE.meta}>{note.fromFolder}</p>
        {status && (
          <p
            data-testid="note-inbox-status"
            className={cx('text-lz-body text-lz-ink', WEIGHT.semibold)}
          >
            {status}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2">
          {open && !busy && (
            <>
              <Button
                variant="primary"
                size="sm"
                data-testid="note-give-now"
                disabled={saving}
                onClick={giveNow}
              >
                {intl.formatMessage(w.giveNow)}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                data-testid="note-add-to-next"
                disabled={saving}
                onClick={() => inbox('add_to_next_message')}
              >
                {intl.formatMessage(w.addToNext)}
              </Button>
            </>
          )}
          {open && busy && (
            <>
              <Button
                variant="primary"
                size="sm"
                data-testid="note-steer-this-turn"
                disabled={saving}
                onClick={() => inbox('steer_this_turn')}
              >
                {intl.formatMessage(w.steerThisTurn)}
              </Button>
              {!note.offerWhenIdle && (
                <Button
                  variant="secondary"
                  size="sm"
                  data-testid="note-after-this-turn"
                  disabled={saving}
                  onClick={() => inbox('after_this_turn')}
                >
                  {intl.formatMessage(w.afterThisTurn)}
                </Button>
              )}
            </>
          )}
          {note.status !== 'steering' && (
            <Button
              variant="ghost"
              size="sm"
              icon={<X />}
              data-testid="note-dismiss"
              disabled={saving}
              onClick={() => inbox('dismiss')}
            >
              {intl.formatMessage(w.dismiss)}
            </Button>
          )}
        </div>
        {error && (
          <p role="alert" className="text-lz-meta text-lz-err">
            {intl.formatMessage(w.failed, { error })}
          </p>
        )}
      </div>
    </section>
  );
}

/**
 * Pinned above the composer of the chat notes were SENT to: each note from the person's other chat
 * that goose here has not read, with what the person can do with it — given to goose now or with
 * their next message when goose is idle, steered into this turn or after it when goose works, or
 * dismissed. Nothing reaches the model here without one of those clicks, or the sender's "Steer it
 * now".
 */
export default function NoteInboxTray({
  sessionId,
  chatState,
  sendBlocked = false,
  give = giveLive,
  className,
}: {
  sessionId: string;
  chatState: ChatState;
  sendBlocked?: boolean;
  give?: Give;
  className?: string;
}) {
  const { list } = useChatNotes(sessionId);
  const waiting = (list?.inbox ?? []).filter(
    (note) =>
      note.status === 'waiting' || note.status === 'steering' || note.status === 'with_next_message'
  );
  if (waiting.length === 0) return null;
  const busy = chatState !== ChatState.Idle || sendBlocked;
  return (
    <div data-testid="note-inbox-tray" className={cx('flex flex-col gap-2', className)}>
      {waiting.map((note) => (
        <NoteInboxCard key={note.id} sessionId={sessionId} note={note} busy={busy} give={give} />
      ))}
    </div>
  );
}

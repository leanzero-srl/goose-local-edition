import { useEffect, useMemo, useState } from 'react';
import { Check, CircleX, Clock, Send, X } from 'lucide-react';
import { useIntl } from '../../i18n';
import {
  notesDraft,
  notesSend,
  notesTargets,
  type NoteChatDto,
  type NoteDelivery,
  type NoteDraftDto,
} from '../../acp/notes';
import { Button, FOCUS, MOTION, RADIUS, SURFACE, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { noteWords as w } from './noteWords';
import { refreshAfter, useChatNotes } from './notesStore';

/** Note surfaces are solid violet: a header band and a full border — never a rail, never a wash. */
export const NOTE_CARD = 'overflow-hidden border-2 border-lz-secondary bg-lz-surface';
export const NOTE_BAND = cx(
  'flex w-full min-w-0 items-center gap-2 px-3 py-1.5 [&>svg]:size-4 [&>svg]:shrink-0',
  TONE_FILL.secondary
);

/** "14:02" in the viewer's clock. */
export function clockOf(iso: string | undefined | null): string {
  if (!iso) return '';
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return '';
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function LiveLine({ chat }: { chat: NoteChatDto }) {
  const intl = useIntl();
  const text =
    chat.live === 'working'
      ? intl.formatMessage(w.liveWorking)
      : chat.live === 'idle'
        ? intl.formatMessage(w.liveIdle, { time: clockOf(chat.lastActiveAt) })
        : intl.formatMessage(w.liveNotOpen);
  return (
    <span data-testid="note-target-live" data-live={chat.live} className="italic">
      {text}
    </span>
  );
}

function ChatRow({
  chat,
  onPick,
  disabled,
}: {
  chat: NoteChatDto;
  onPick: (chat: NoteChatDto) => void;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      data-testid="note-candidate"
      data-session-id={chat.sessionId}
      disabled={disabled}
      onClick={() => onPick(chat)}
      className={cx(
        'flex w-full min-w-0 flex-col items-start gap-0.5 border border-lz-border-strong px-3 py-1.5 text-left',
        'disabled:pointer-events-none disabled:text-lz-ink-3',
        SURFACE.hover,
        RADIUS.control,
        FOCUS,
        MOTION
      )}
    >
      <span className={cx('w-full truncate text-lz-body text-lz-ink', WEIGHT.semibold)}>
        {`"${chat.name}"`}
      </span>
      <span className={cx('w-full truncate', TYPE.meta)}>
        {chat.folder} · <LiveLine chat={chat} />
      </span>
    </button>
  );
}

/** Every chat the note could go to, narrowed by the words typed — a list of buttons, never a select. */
function ChatPicker({
  sessionId,
  onPick,
  onBack,
  disabled,
}: {
  sessionId: string;
  onPick: (chat: NoteChatDto) => void;
  onBack?: () => void;
  disabled: boolean;
}) {
  const intl = useIntl();
  const [chats, setChats] = useState<NoteChatDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  useEffect(() => {
    let live = true;
    notesTargets(sessionId).then(
      (found) => live && setChats(found),
      (e) => live && setError(errorText(e))
    );
    return () => {
      live = false;
    };
  }, [sessionId]);
  const shown = useMemo(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    return (chats ?? []).filter((chat) => {
      const haystack = `${chat.name} ${chat.folder}`.toLowerCase();
      return words.every((word) => haystack.includes(word));
    });
  }, [chats, filter]);

  return (
    <div data-testid="note-picker" className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <span className={TYPE.zone}>{intl.formatMessage(w.pickTitle)}</span>
        <span className="flex-1" />
        {onBack && (
          <Button variant="ghost" size="sm" data-testid="note-picker-back" onClick={onBack}>
            {intl.formatMessage(w.pickBack)}
          </Button>
        )}
      </div>
      <input
        type="text"
        data-testid="note-picker-filter"
        value={filter}
        placeholder={intl.formatMessage(w.pickFilter)}
        aria-label={intl.formatMessage(w.pickFilter)}
        onChange={(e) => setFilter(e.target.value)}
        className={cx(
          'w-full border border-lz-border-strong bg-lz-surface px-2 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
          RADIUS.control,
          FOCUS,
          MOTION
        )}
      />
      {error && (
        <p role="alert" className="text-lz-meta text-lz-err">
          {intl.formatMessage(w.failed, { error })}
        </p>
      )}
      {!error && chats === null && <p className={TYPE.meta}>{intl.formatMessage(w.pickLoading)}</p>}
      {chats !== null && shown.length === 0 && (
        <p className={TYPE.meta}>{intl.formatMessage(w.pickNone)}</p>
      )}
      <div className="flex max-h-48 flex-col gap-1 overflow-y-auto">
        {shown.map((chat) => (
          <ChatRow key={chat.sessionId} chat={chat} onPick={onPick} disabled={disabled} />
        ))}
      </div>
    </div>
  );
}

export function NoteDraftCard({ sessionId, draft }: { sessionId: string; draft: NoteDraftDto }) {
  const intl = useIntl();
  const [text, setText] = useState(draft.text);
  const [picking, setPicking] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target = draft.target ?? null;
  const unresolved = target === null;

  const act = async (run: () => Promise<unknown>, touched: string[]) => {
    setSaving(true);
    setError(null);
    try {
      await run();
      refreshAfter(touched);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };
  const send = (delivery: NoteDelivery) => {
    if (!target) return;
    void act(() => notesSend(sessionId, draft.id, text, delivery), [sessionId, target.sessionId]);
  };
  const pick = (chat: NoteChatDto) =>
    void act(async () => {
      await notesDraft(sessionId, draft.id, { kind: 'retarget', toSessionId: chat.sessionId });
      setPicking(false);
    }, [sessionId]);
  const cancel = () =>
    void act(() => notesDraft(sessionId, draft.id, { kind: 'cancel' }), [sessionId]);

  return (
    <section
      data-testid="note-draft-card"
      data-note-id={draft.id}
      aria-label={intl.formatMessage(w.draftBand)}
      className={cx(NOTE_CARD, RADIUS.card, 'shrink-0')}
    >
      <div className={NOTE_BAND}>
        <Send aria-hidden />
        <span className={cx('min-w-0 truncate text-lz-body', WEIGHT.semibold)}>
          {intl.formatMessage(w.draftBand)}
        </span>
      </div>
      <div className="flex flex-col gap-2 px-4 pb-3 pt-3">
        {target && !picking && (
          <p
            data-testid="note-target"
            className="flex flex-wrap items-baseline gap-x-1.5 text-lz-body"
          >
            <span className={cx('text-lz-ink', WEIGHT.semibold)}>{`"${target.name}"`}</span>
            <span className={TYPE.meta}>
              · {target.folder} · <LiveLine chat={target} />
            </span>
          </p>
        )}
        {unresolved && draft.resolution === 'ambiguous' && !picking && (
          <div className="flex flex-col gap-1.5">
            <p data-testid="note-ambiguous" className="text-lz-body text-lz-ink">
              {intl.formatMessage(w.ambiguous, { query: draft.toQuery })}
            </p>
            <div className="flex max-h-48 flex-col gap-1 overflow-y-auto">
              {(draft.candidates ?? []).map((chat) => (
                <ChatRow key={chat.sessionId} chat={chat} onPick={pick} disabled={saving} />
              ))}
            </div>
          </div>
        )}
        {unresolved && draft.resolution !== 'ambiguous' && !picking && (
          <p data-testid="note-no-match" className="text-lz-body text-lz-ink">
            {intl.formatMessage(w.noMatch, { query: draft.toQuery })}
          </p>
        )}
        {(picking || (unresolved && draft.resolution !== 'ambiguous')) && (
          <ChatPicker
            sessionId={sessionId}
            onPick={pick}
            onBack={picking ? () => setPicking(false) : undefined}
            disabled={saving}
          />
        )}

        <label className="flex flex-col gap-1">
          <span className={TYPE.zone}>{intl.formatMessage(w.textLabel)}</span>
          <textarea
            data-testid="note-text"
            rows={3}
            value={text}
            disabled={saving}
            onChange={(e) => setText(e.target.value)}
            className={cx(
              'w-full resize-y border border-lz-border-strong bg-lz-surface px-2 py-1.5 text-lz-body text-lz-ink',
              'disabled:bg-lz-surface-2 disabled:text-lz-ink-3',
              RADIUS.control,
              FOCUS,
              MOTION
            )}
          />
        </label>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            size="sm"
            data-testid="note-steer-now"
            disabled={saving || unresolved || text.trim().length === 0}
            onClick={() => send('steer_now')}
          >
            {intl.formatMessage(w.steerNow)}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            data-testid="note-leave-there"
            disabled={saving || unresolved || text.trim().length === 0}
            onClick={() => send('leave_there')}
          >
            {intl.formatMessage(w.leaveThere)}
          </Button>
          {target && !picking && (
            <Button
              variant="ghost"
              size="sm"
              data-testid="note-not-this-chat"
              disabled={saving}
              onClick={() => setPicking(true)}
            >
              {intl.formatMessage(w.notThisChat)}
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            icon={<X />}
            data-testid="note-cancel"
            disabled={saving}
            onClick={cancel}
          >
            {intl.formatMessage(w.cancel)}
          </Button>
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

/** The one line a sent note leaves in the chat that wrote it, following what became of it. */
export function NoteSentLine({ draft, onClose }: { draft: NoteDraftDto; onClose: () => void }) {
  const intl = useIntl();
  const name = draft.target?.name ?? draft.toQuery;
  const outcome = draft.outcome?.state ?? 'waiting';
  const words =
    outcome === 'delivered'
      ? intl.formatMessage(w.sentRead, { name, time: clockOf(draft.outcome?.at) })
      : outcome === 'dismissed'
        ? intl.formatMessage(w.sentDismissed, { name })
        : outcome === 'gone'
          ? intl.formatMessage(w.sentGone, { name, reason: draft.outcome?.reason ?? '' })
          : intl.formatMessage(w.sentWaiting, { name });
  const icon =
    outcome === 'delivered' ? (
      <Check aria-hidden />
    ) : outcome === 'waiting' || outcome === 'steering' || outcome === 'with_next_message' ? (
      <Clock aria-hidden />
    ) : (
      <CircleX aria-hidden />
    );
  return (
    <div
      data-testid="note-sent-line"
      data-note-id={draft.id}
      data-outcome={outcome}
      className={cx(
        'flex min-w-0 shrink-0 items-center gap-2 border-2 border-lz-secondary bg-lz-surface px-3 py-1.5 text-lz-body text-lz-ink [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-lz-secondary',
        RADIUS.card,
        TNUM
      )}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{words}</span>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={<X />}
        aria-label={intl.formatMessage(w.closeSent)}
        data-testid="note-sent-close"
        onClick={onClose}
      />
    </div>
  );
}

const CLOSED_KEY = 'goose.notes.closedSentLines';

function readClosed(): Set<string> {
  try {
    const raw = window.localStorage.getItem(CLOSED_KEY);
    const ids: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(ids) ? ids.filter((id) => typeof id === 'string') : []);
  } catch {
    return new Set();
  }
}

function writeClosed(ids: Set<string>): void {
  try {
    window.localStorage.setItem(CLOSED_KEY, JSON.stringify([...ids]));
  } catch {
    // A line the browser cannot remember closing shows again after a reload; nothing else depends on it.
  }
}

/**
 * Pinned above the composer of the chat that WROTE the notes: each draft `send_note` pinned, until
 * the person sends or cancels it, then one line that follows what became of the note there.
 */
export default function NoteDraftTray({
  sessionId,
  className,
}: {
  sessionId: string;
  className?: string;
}) {
  const intl = useIntl();
  const { list, error } = useChatNotes(sessionId);
  const [closed, setClosed] = useState<Set<string>>(readClosed);
  const drafts = (list?.drafts ?? []).filter((draft) => draft.status === 'draft');
  const sent = (list?.drafts ?? []).filter(
    (draft) => draft.status === 'sent' && !closed.has(draft.id)
  );
  if (drafts.length === 0 && sent.length === 0 && !error) return null;
  const close = (id: string) => {
    const next = new Set(closed).add(id);
    writeClosed(next);
    setClosed(next);
  };
  return (
    <div data-testid="note-draft-tray" className={cx('flex flex-col gap-2', className)}>
      {error && (
        <p role="alert" className="text-lz-meta text-lz-err">
          {intl.formatMessage(w.unreadable, { error })}
        </p>
      )}
      {sent.map((draft) => (
        <NoteSentLine key={draft.id} draft={draft} onClose={() => close(draft.id)} />
      ))}
      {drafts.map((draft) => (
        <NoteDraftCard key={draft.id} sessionId={sessionId} draft={draft} />
      ))}
    </div>
  );
}

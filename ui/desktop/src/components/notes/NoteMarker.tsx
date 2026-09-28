import { useId, useState } from 'react';
import { ChevronDown, Mail } from 'lucide-react';
import { useIntl } from '../../i18n';
import type { Message } from '../../types/message';
import { getTextAndImageContent } from '../../types/message';
import { FOCUS, MOTION, SURFACE, TNUM, TONE_FILL, cx } from '../lz';
import { NOTE_MESSAGE_PREFIX } from './noteIds';
import { noteWords as w } from './noteWords';

/** The chat a note came from, as its framing names it; `null` when the words are not a note's. */
export function noteSender(text: string): string | null {
  const match = /^Note from your other chat "(.*?)" \(/.exec(text);
  return match ? match[1] : null;
}

/** The sending chat's name when `message` is a note's (its id and its framing both say so). */
export function noteMessageSender(message: Message): string | null {
  if (message.role !== 'user' || !message.id?.startsWith(NOTE_MESSAGE_PREFIX)) return null;
  return noteSender(getTextAndImageContent(message).textContent);
}

function clock(seconds: number): string {
  const at = new Date(seconds * 1000);
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
}

/**
 * A note from another chat in the transcript (Q-358): a full-width divider like a loop tick's —
 * "Note from "Explore split mesh" · 14:05" — with the exact words goose read one click away. Never
 * edited, forked or looped: it is not the person's message.
 */
export function NoteMarker({ message, from }: { message: Message; from: string }) {
  const intl = useIntl();
  const [open, setOpen] = useState(false);
  const textId = useId();
  const { textContent } = getTextAndImageContent(message);
  return (
    <div data-testid="note-marker" data-message-id={message.id} className="w-full mt-[16px]">
      <div className="flex items-center gap-3">
        <span aria-hidden className="h-0 flex-1 border-t border-lz-border-strong" />
        <span
          data-testid="note-marker-title"
          className={cx(
            'inline-flex h-6 min-w-0 shrink items-center gap-1.5 rounded-lz-pill px-2.5 text-xs font-lz-semibold',
            TONE_FILL.secondary,
            TNUM
          )}
        >
          <Mail aria-hidden className="size-3.5 shrink-0" />
          <span className="truncate">
            {intl.formatMessage(w.markerTitle, {
              name: from,
              time: clock(message.created),
            })}
          </span>
        </span>
        <span aria-hidden className="h-0 flex-1 border-t border-lz-border-strong" />
        <button
          type="button"
          data-testid="note-marker-toggle"
          aria-expanded={open}
          aria-controls={textId}
          onClick={() => setOpen(!open)}
          className={cx(
            'inline-flex h-6 shrink-0 items-center gap-1 rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-xs font-lz-medium text-lz-ink hover:bg-lz-surface-2',
            FOCUS,
            MOTION
          )}
        >
          {intl.formatMessage(open ? w.markerHide : w.markerShow)}
          <ChevronDown aria-hidden className={cx('size-3.5', open && 'rotate-180')} />
        </button>
      </div>
      {open && (
        <pre
          id={textId}
          data-testid="note-marker-text"
          className={cx(
            'mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-lz-control border p-3 text-xs text-lz-ink bg-lz-surface-2',
            SURFACE.hairline
          )}
        >
          {textContent}
        </pre>
      )}
    </div>
  );
}

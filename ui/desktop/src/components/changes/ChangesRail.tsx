import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { ChevronRight, FileDiff as FileDiffIcon, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import type { Message } from '../../types/message';
import { FOCUS, MOTION, SURFACE, TYPE, cx } from '../lz';
import { DiffCounts, DiffView } from './DiffView';
import { type ChangedFile, revealToolCall, sessionChanges, splitPath } from './fileDiff';

const i18n = defineMessages({
  title: {
    id: 'changes.rail.title',
    defaultMessage: 'Changes',
  },
  fileCount: {
    id: 'changes.rail.fileCount',
    defaultMessage: '{count, plural, one {# file} other {# files}}',
  },
  open: {
    id: 'changes.rail.open',
    defaultMessage: 'Show the files this chat changed',
  },
  close: {
    id: 'changes.rail.close',
    defaultMessage: 'Close changes',
  },
  editOf: {
    id: 'changes.rail.editOf',
    defaultMessage: 'Change {index} of {count}',
  },
  showInChat: {
    id: 'changes.rail.showInChat',
    defaultMessage: 'Show in chat',
  },
});

/**
 * The session's Changes rail (Q-190): every file this chat's write/edit calls changed, from the same
 * diff the tool cards draw (`sessionChanges` over `fileDiffOf`). Collapsed it is one slim pill in the
 * pane's top-right corner — "N files · +A −R" — so it takes no width from the conversation; opened
 * it is an overlay panel over the chat's right side, an accordion of files, each opening to its
 * changes' hunks. A hunk takes the person to the call in the chat. Nothing changed: nothing shown.
 */
export default function ChangesRail({
  messages,
  className,
}: {
  messages: readonly Message[];
  className?: string;
}) {
  const intl = useIntl();
  const changes = useMemo(() => sessionChanges(messages), [messages]);
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const pillRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (open) panelRef.current?.focus();
  }, [open]);

  if (changes.files.length === 0) return null;

  const close = () => {
    setOpen(false);
    requestAnimationFrame(() => pillRef.current?.focus());
  };
  const fileCount = intl.formatMessage(i18n.fileCount, { count: changes.files.length });

  return (
    <div
      data-testid="changes-rail"
      className={cx('pointer-events-none flex flex-col items-end', className)}
    >
      {!open ? (
        <button
          ref={pillRef}
          type="button"
          data-testid="changes-rail-pill"
          aria-expanded={false}
          aria-controls={panelId}
          aria-label={`${intl.formatMessage(i18n.open)} — ${fileCount}`}
          onClick={() => setOpen(true)}
          className={cx(
            'pointer-events-auto inline-flex h-8 items-center gap-2 rounded-lz-pill border border-lz-border-strong bg-lz-surface px-3 text-xs text-lz-ink shadow-lz-overlay dark:shadow-lz-overlay-dark hover:bg-lz-surface-2',
            FOCUS,
            MOTION
          )}
        >
          <FileDiffIcon aria-hidden className="size-4 text-lz-accent" />
          <span className="font-lz-semibold">{fileCount}</span>
          <DiffCounts added={changes.added} removed={changes.removed} />
        </button>
      ) : (
        <div
          id={panelId}
          ref={panelRef}
          role="region"
          tabIndex={-1}
          aria-label={intl.formatMessage(i18n.title)}
          data-testid="changes-rail-panel"
          onKeyDown={(e) => {
            if (e.key === 'Escape') close();
          }}
          className={cx(
            'pointer-events-auto flex max-h-[65vh] w-[min(32rem,calc(100vw-2rem))] flex-col overflow-hidden outline-none',
            SURFACE.overlay
          )}
        >
          <div
            className={cx('flex shrink-0 items-center gap-2 border-b px-3 py-2', SURFACE.hairline)}
          >
            <FileDiffIcon aria-hidden className="size-4 shrink-0 text-lz-accent" />
            <span className={cx(TYPE.body, 'font-lz-semibold')}>
              {intl.formatMessage(i18n.title)}
            </span>
            <span className="text-xs text-lz-ink-2">{fileCount}</span>
            <DiffCounts added={changes.added} removed={changes.removed} />
            <button
              type="button"
              data-testid="changes-rail-close"
              aria-label={intl.formatMessage(i18n.close)}
              onClick={close}
              className={cx(
                'ml-auto inline-flex size-7 items-center justify-center rounded-lz-control text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink',
                FOCUS,
                MOTION
              )}
            >
              <X aria-hidden className="size-4" />
            </button>
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto">
            {changes.files.map((file) => (
              <ChangedFileRow key={file.path} file={file} startOpen={changes.files.length === 1} />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function ChangedFileRow({ file, startOpen }: { file: ChangedFile; startOpen: boolean }) {
  const intl = useIntl();
  const [open, setOpen] = useState(startOpen);
  const bodyId = useId();
  const { name, dir } = splitPath(file.path);
  return (
    <li
      data-testid="changes-rail-file"
      className={cx('border-b last:border-b-0', SURFACE.hairline)}
    >
      <button
        type="button"
        aria-expanded={open}
        aria-controls={bodyId}
        title={file.path}
        onClick={() => setOpen(!open)}
        className={cx(
          'flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left hover:bg-lz-surface-2',
          FOCUS,
          MOTION
        )}
      >
        <ChevronRight
          aria-hidden
          className={cx('size-4 shrink-0 text-lz-ink-2', open && 'rotate-90')}
        />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-lz-semibold text-lz-ink">{name}</span>
          {dir && <span className="block truncate text-xs text-lz-ink-3">{dir}</span>}
        </span>
        <DiffCounts added={file.added} removed={file.removed} />
      </button>
      {open && (
        <div id={bodyId} className="pb-2">
          {file.edits.map((edit, index) => (
            <div key={edit.toolCallId} data-testid="changes-rail-edit" className="mt-1">
              {file.edits.length > 1 && (
                <div className="flex items-center gap-2 px-3 py-1">
                  <span className="text-xs font-lz-semibold text-lz-ink-2">
                    {intl.formatMessage(i18n.editOf, {
                      index: index + 1,
                      count: file.edits.length,
                    })}
                  </span>
                  <DiffCounts added={edit.diff.added} removed={edit.diff.removed} />
                  <button
                    type="button"
                    onClick={() => revealToolCall(edit.toolCallId)}
                    className={cx(
                      'ml-auto rounded-lz-pill bg-lz-accent px-2 text-xs font-lz-semibold text-lz-accent-ink hover:bg-lz-accent-hover',
                      FOCUS,
                      MOTION
                    )}
                  >
                    {intl.formatMessage(i18n.showInChat)}
                  </button>
                </div>
              )}
              <DiffView diff={edit.diff} onShowInChat={() => revealToolCall(edit.toolCallId)} />
            </div>
          ))}
        </div>
      )}
    </li>
  );
}

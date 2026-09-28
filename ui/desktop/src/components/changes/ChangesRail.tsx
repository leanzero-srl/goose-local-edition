import { forwardRef, useId, useState } from 'react';
import { ChevronRight, FileDiff as FileDiffIcon } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, SURFACE, cx } from '../lz';
import { DiffCounts, DiffView } from './DiffView';
import { type ChangedFile, type SessionChanges, revealToolCall, splitPath } from './fileDiff';

export const changesRailMessages = defineMessages({
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

const i18n = changesRailMessages;

/**
 * The session's Changes (Q-190): every file this chat's write/edit calls changed, from the same diff
 * the tool cards draw (`sessionChanges` over `fileDiffOf`). The rail that hosts it is `SessionRail`
 * (Q-228 L5): collapsed, this is one slim pill in the pane's corner — "N files · +A −R" — so it takes
 * no width from the conversation; opened, `ChangesPanelBody` is the Changes tab of the overlay panel,
 * an accordion of files, each opening to its changes' hunks. A hunk takes the person to the call in
 * the chat. The loop's tick rows list their own tick's files with the same `ChangedFileList`.
 */
export const ChangesPill = forwardRef<
  HTMLButtonElement,
  { changes: SessionChanges; panelId: string; onOpen: () => void }
>(function ChangesPill({ changes, panelId, onOpen }, ref) {
  const intl = useIntl();
  const fileCount = intl.formatMessage(i18n.fileCount, { count: changes.files.length });
  return (
    <button
      ref={ref}
      type="button"
      data-testid="changes-rail-pill"
      aria-expanded={false}
      aria-controls={panelId}
      aria-label={`${intl.formatMessage(i18n.open)} — ${fileCount}`}
      onClick={onOpen}
      className={cx(
        'pointer-events-auto inline-flex h-8 max-w-[calc(100vw-2rem)] items-center gap-2 rounded-lz-pill border border-lz-border-strong bg-lz-surface px-3 text-xs text-lz-ink shadow-lz-overlay dark:shadow-lz-overlay-dark hover:bg-lz-surface-2',
        FOCUS,
        MOTION
      )}
    >
      <FileDiffIcon aria-hidden className="size-4 shrink-0 text-lz-accent" />
      <span className="font-lz-semibold">{fileCount}</span>
      <DiffCounts added={changes.added} removed={changes.removed} />
    </button>
  );
});

/** The Changes tab: how many files and lines, then the files. */
export function ChangesPanelBody({ changes }: { changes: SessionChanges }) {
  const intl = useIntl();
  return (
    <div data-testid="changes-rail-body" className="flex min-h-0 flex-1 flex-col">
      <div className={cx('flex shrink-0 items-center gap-2 border-b px-3 py-2', SURFACE.hairline)}>
        <FileDiffIcon aria-hidden className="size-4 shrink-0 text-lz-accent" />
        <span className="text-xs text-lz-ink-2">
          {intl.formatMessage(i18n.fileCount, { count: changes.files.length })}
        </span>
        <DiffCounts added={changes.added} removed={changes.removed} />
      </div>
      <ChangedFileList files={changes.files} className="min-h-0 flex-1 overflow-y-auto" />
    </div>
  );
}

/** Files as an accordion; one file opens straight to its hunks. */
export function ChangedFileList({
  files,
  className,
}: {
  files: ChangedFile[];
  className?: string;
}) {
  return (
    <ul className={className}>
      {files.map((file) => (
        <ChangedFileRow key={file.path} file={file} startOpen={files.length === 1} />
      ))}
    </ul>
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

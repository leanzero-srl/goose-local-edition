import { useEffect, useMemo, useState } from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, TNUM, cx } from '../lz';
import type { DiffHunk, DiffLine, FileDiff } from './fileDiff';

const i18n = defineMessages({
  hunkLine: {
    id: 'changes.diff.hunkLine',
    defaultMessage: 'Line {line}',
  },
  showInChat: {
    id: 'changes.diff.showInChat',
    defaultMessage: 'Show in chat',
  },
  showAll: {
    id: 'changes.diff.showAll',
    defaultMessage: 'Show all {count, plural, one {# line} other {# lines}}',
  },
  noLinesChanged: {
    id: 'changes.diff.noLinesChanged',
    defaultMessage: 'No lines changed — the file already held this text.',
  },
  newFile: {
    id: 'changes.diff.newFile',
    defaultMessage: 'New file',
  },
  unreadableBefore: {
    id: 'changes.diff.unreadableBefore',
    defaultMessage:
      'The file held something that was not text before — only the new text is shown.',
  },
  added: {
    id: 'changes.diff.addedLabel',
    defaultMessage: '{count, plural, one {# line added} other {# lines added}}',
  },
  removed: {
    id: 'changes.diff.removedLabel',
    defaultMessage: '{count, plural, one {# line removed} other {# lines removed}}',
  },
});

/** The row's own height; the collapse budget divides by it, and every row is drawn at it. */
const LINE_PX = 18;
// ratio: of the window's height — a diff taller than half the window folds behind "Show all".
const VIEWPORT_SHARE = 0.5;

function useViewportLineBudget(): number {
  const measure = () => Math.max(1, Math.floor((window.innerHeight * VIEWPORT_SHARE) / LINE_PX));
  const [budget, setBudget] = useState(measure);
  useEffect(() => {
    const onResize = () => setBudget(measure());
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return budget;
}

/** +N −M in solid ok/err ink; the words ride as the accessible name. */
export function DiffCounts({
  added,
  removed,
  className,
}: {
  added: number;
  removed: number;
  className?: string;
}) {
  const intl = useIntl();
  return (
    <span
      data-testid="diff-counts"
      className={cx('inline-flex shrink-0 items-center gap-1.5 font-mono text-xs', TNUM, className)}
    >
      <span
        className="font-lz-semibold text-lz-ok"
        aria-label={intl.formatMessage(i18n.added, { count: added })}
      >
        +{added}
      </span>
      <span
        className="font-lz-semibold text-lz-err"
        aria-label={intl.formatMessage(i18n.removed, { count: removed })}
      >
        −{removed}
      </span>
    </span>
  );
}

type Row = { type: 'hunk'; hunk: DiffHunk; index: number } | { type: 'line'; line: DiffLine };

function lineNumber(line: DiffLine, side: 'old' | 'new'): number | '' {
  if (line.kind === 'eof') return '';
  if (side === 'old') return line.kind === 'add' ? '' : line.oldNo;
  return line.kind === 'del' ? '' : line.newNo;
}

const SIGN: Record<DiffLine['kind'], string> = { add: '+', del: '−', ctx: '', eof: '' };
const SIGN_CELL: Record<DiffLine['kind'], string> = {
  add: 'bg-lz-ok-solid text-white',
  del: 'bg-lz-err-solid text-white',
  ctx: '',
  eof: '',
};
const TEXT: Record<DiffLine['kind'], string> = {
  add: 'text-lz-ok',
  del: 'text-lz-err',
  ctx: 'text-lz-ink-2',
  eof: 'text-lz-ink-3 italic',
};

export function DiffView({
  diff,
  onShowInChat,
  className,
}: {
  diff: FileDiff;
  /** The rail passes this: each hunk's header then takes the person to the call in the chat. */
  onShowInChat?: () => void;
  className?: string;
}) {
  const intl = useIntl();
  const budget = useViewportLineBudget();
  const [showAll, setShowAll] = useState(false);

  const rows = useMemo<Row[]>(
    () =>
      diff.hunks.flatMap((hunk, index) => [
        { type: 'hunk' as const, hunk, index },
        ...hunk.lines.map((line) => ({ type: 'line' as const, line })),
      ]),
    [diff.hunks]
  );
  const lineCount = rows.filter((r) => r.type === 'line').length;
  const folded = !showAll && rows.length > budget;
  const shown = folded ? rows.slice(0, budget) : rows;

  let lastNo = 0;
  for (const row of rows) {
    if (row.type !== 'line') continue;
    for (const side of ['old', 'new'] as const) {
      const no = lineNumber(row.line, side);
      if (no !== '' && no > lastNo) lastNo = no;
    }
  }
  const numberWidth = `${String(lastNo).length + 1}ch`;

  return (
    <div data-testid="diff-view" className={cx('min-w-0', className)}>
      {diff.before === 'none' && (
        <p data-testid="diff-new-file" className="px-3 pt-2 text-xs font-lz-semibold text-lz-ok">
          {intl.formatMessage(i18n.newFile)}
        </p>
      )}
      {diff.before === 'unreadable' && (
        <p data-testid="diff-unreadable-before" className="px-3 pt-2 text-xs text-lz-warn">
          {intl.formatMessage(i18n.unreadableBefore)}
        </p>
      )}
      {diff.hunks.length === 0 ? (
        <p data-testid="diff-empty" className="px-3 py-2 text-xs text-lz-ink-3">
          {intl.formatMessage(i18n.noLinesChanged)}
        </p>
      ) : (
        <div className="overflow-x-auto py-1 font-mono text-xs">
          <div className="min-w-max">
            {shown.map((row, i) =>
              row.type === 'hunk' ? (
                <HunkHeader
                  key={`h${i}`}
                  line={row.hunk.newStart || row.hunk.oldStart}
                  first={row.index === 0}
                  onShowInChat={onShowInChat}
                />
              ) : (
                <div
                  key={`l${i}`}
                  data-testid={`diff-line-${row.line.kind}`}
                  className="flex items-stretch pl-2"
                  style={{ height: LINE_PX, lineHeight: `${LINE_PX}px` }}
                >
                  <span
                    className={cx('shrink-0 select-none pr-1 text-right text-lz-ink-3', TNUM)}
                    style={{ width: numberWidth }}
                  >
                    {lineNumber(row.line, 'old')}
                  </span>
                  <span
                    className={cx('shrink-0 select-none pr-1 text-right text-lz-ink-3', TNUM)}
                    style={{ width: numberWidth }}
                  >
                    {lineNumber(row.line, 'new')}
                  </span>
                  <span
                    aria-hidden
                    className={cx(
                      'w-5 shrink-0 select-none text-center font-lz-semibold',
                      SIGN_CELL[row.line.kind]
                    )}
                  >
                    {SIGN[row.line.kind]}
                  </span>
                  <span className={cx('whitespace-pre pl-2 pr-3', TEXT[row.line.kind])}>
                    {row.line.text || ' '}
                  </span>
                </div>
              )
            )}
          </div>
        </div>
      )}
      {folded && (
        <button
          type="button"
          data-testid="diff-show-all"
          onClick={() => setShowAll(true)}
          className={cx(
            'mx-3 mb-2 rounded-lz-control border border-lz-border-strong px-2 py-1 text-xs font-lz-semibold text-lz-ink hover:bg-lz-surface-2',
            FOCUS,
            MOTION
          )}
        >
          {intl.formatMessage(i18n.showAll, { count: lineCount })}
        </button>
      )}
    </div>
  );
}

function HunkHeader({
  line,
  first,
  onShowInChat,
}: {
  line: number;
  first: boolean;
  onShowInChat?: () => void;
}) {
  const intl = useIntl();
  const label = intl.formatMessage(i18n.hunkLine, { line });
  const bar = cx(
    'flex w-full items-center gap-2 bg-lz-surface-2 px-3 font-sans text-xs text-lz-ink-2',
    !first && 'mt-1'
  );
  if (!onShowInChat) {
    return (
      <div data-testid="diff-hunk" className={bar} style={{ height: LINE_PX }}>
        {label}
      </div>
    );
  }
  return (
    <button
      type="button"
      data-testid="diff-hunk"
      onClick={onShowInChat}
      className={cx(bar, 'text-left hover:text-lz-ink', FOCUS, MOTION)}
      style={{ height: LINE_PX }}
    >
      <span>{label}</span>
      <span className="rounded-lz-pill bg-lz-accent px-2 leading-4 font-lz-semibold text-lz-accent-ink">
        {intl.formatMessage(i18n.showInChat)}
      </span>
    </button>
  );
}

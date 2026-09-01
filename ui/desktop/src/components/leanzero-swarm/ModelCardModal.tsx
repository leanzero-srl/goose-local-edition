import { useEffect, useState } from 'react';
import { Download, ExternalLink, Loader2, X } from 'lucide-react';
import MarkdownContent from '../MarkdownContent';
import { errorMessage } from '../../utils/conversionUtils';
import {
  mlxEngineModelCard,
  type MlxDownloadProgress,
  type MlxModelCard,
  type MlxRepoFile,
} from '../../acp/mlx-engine';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  KeyValue,
  Panel,
  FOCUS,
  MOTION,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type KeyValueItem,
} from '../lz';
import {
  DownloadProgressRow,
  SolidBanner,
  formatBytesShort,
  formatCount,
  formatDate,
  type DownloadLifecycleHandlers,
} from './primitives';

/** The exact file listing: path in mono, size right-aligned in tabular figures. */
const FILE_COLUMNS: DataTableColumn<MlxRepoFile>[] = [
  {
    key: 'path',
    header: 'File',
    cell: (file) => (
      <span className="break-all font-mono text-lz-mono text-lz-ink">{file.path}</span>
    ),
  },
  {
    key: 'size',
    header: 'Size',
    numeric: true,
    cell: (file) => <span className={cx(TYPE.meta, TNUM)}>{formatBytesShort(file.sizeBytes)}</span>,
  },
];

/**
 * Fullscreen model card for one Hugging Face repo — a custom modal (no iframe/webview,
 * no native chrome). The header Panel carries the repo facts as KeyValue rows and the SAME
 * download lifecycle the browse row shows; the body renders the README through the app's
 * chat markdown renderer plus the exact file listing as a table. Esc or ✕ closes.
 */
export function ModelCardModal({
  repoId,
  nodeId,
  onClose,
  progress,
  startError,
  onDownload,
  onPause,
  onResume,
  onCancel,
}: {
  repoId: string;
  /** The device the card is read from — undefined = local, byte-identical to before. */
  nodeId?: string;
  onClose: () => void;
  progress: MlxDownloadProgress | undefined;
  startError: string | undefined;
  onDownload: () => void;
} & DownloadLifecycleHandlers) {
  const [card, setCard] = useState<MlxModelCard | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    setCard(null);
    setError(null);
    void (async () => {
      try {
        const next = await mlxEngineModelCard(repoId, nodeId);
        if (!stale) setCard(next);
      } catch (e) {
        if (!stale) setError(errorMessage(e, 'Could not load the model card.'));
      }
    })();
    return () => {
      stale = true;
    };
  }, [repoId, nodeId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const author = repoId.includes('/') ? repoId.split('/')[0] : repoId;
  const hfUrl = `https://huggingface.co/${repoId}`;

  const facts: KeyValueItem[] | null = card && [
    {
      key: 'downloads',
      label: 'Downloads',
      value: (
        <span title={`${card.downloads.toLocaleString()} downloads`}>
          {formatCount(card.downloads)}
        </span>
      ),
    },
    {
      key: 'likes',
      label: 'Likes',
      value: <span title={`${card.likes.toLocaleString()} likes`}>{formatCount(card.likes)}</span>,
    },
    ...(card.license ? [{ key: 'license', label: 'License', value: card.license }] : []),
    ...(card.createdAt
      ? [{ key: 'created', label: 'Created', value: formatDate(card.createdAt) }]
      : []),
    ...(card.lastModified
      ? [{ key: 'updated', label: 'Updated', value: formatDate(card.lastModified) }]
      : []),
    {
      key: 'size',
      label: 'Size (exact)',
      value: (
        <span title="Exact sum of every file the repo tree lists">
          {formatBytesShort(card.totalBytes)}
        </span>
      ),
    },
  ];

  return (
    <div
      className="fixed inset-0 z-[90] flex flex-col bg-lz-bg"
      role="dialog"
      aria-modal="true"
      aria-label={`Model card for ${repoId}`}
      data-testid="mlx-model-card-modal"
    >
      {/* pt-10 clears the frameless window's 32px titlebar drag strip, the macOS traffic
          lights, and the floating sidebar toggle (caught live: the repo id rendered under
          the toggle icon). */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-4 pb-8 pt-10 md:px-6">
        <div className="mx-auto flex max-w-[960px] flex-col gap-4">
          {/* The header card floats on the one elevation token — this is an overlay. */}
          <Panel padded={false} className="shadow-lz-overlay dark:shadow-lz-overlay-dark">
            <div className="flex flex-col gap-3 px-4 py-3">
              <div className="flex flex-wrap items-start gap-3">
                <div className="min-w-0 flex-1">
                  <h1 className={cx('break-all font-mono', TYPE.h1)}>{repoId}</h1>
                  <div className="mt-1.5 flex flex-wrap items-center gap-2">
                    <Chip title={`Published by ${author}`}>{author}</Chip>
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={<ExternalLink />}
                      onClick={() => void window.electron.openExternal(hfUrl)}
                      aria-label={`Open ${repoId} on huggingface.co`}
                      title={hfUrl}
                    >
                      huggingface.co
                    </Button>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {!progress && (
                    <Button
                      variant="primary"
                      icon={<Download />}
                      onClick={onDownload}
                      aria-label={`Download ${repoId}`}
                    >
                      Download
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    icon={<X />}
                    onClick={onClose}
                    aria-label="Close model card"
                  />
                </div>
              </div>
              {startError && (
                <p className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
                  {startError}
                </p>
              )}
              {progress && (
                <DownloadProgressRow
                  repoId={repoId}
                  progress={progress}
                  onPause={onPause}
                  onResume={onResume}
                  onCancel={onCancel}
                />
              )}
            </div>
            {facts && (
              <div className={cx('border-t px-4', SURFACE.hairline)}>
                <KeyValue dense items={facts} aria-label="Model facts" />
              </div>
            )}
            {!card && !error && (
              <p
                className={cx(
                  'flex items-center gap-2 border-t px-4 py-3',
                  TYPE.meta,
                  SURFACE.hairline
                )}
              >
                <Loader2 className="size-3 animate-spin" />
                loading model card…
              </p>
            )}
          </Panel>

          {error && <SolidBanner tone="err" label="Model card failed" text={error} />}
          {card && (
            <>
              {card.tags.length > 0 && (
                <div className="flex flex-wrap items-center gap-1.5">
                  {card.tags.map((tag) => (
                    <Chip key={tag}>{tag}</Chip>
                  ))}
                </div>
              )}
              {card.readmeTruncated && (
                <div className={cx('px-4 py-3', SURFACE.card, TYPE.body)}>
                  This README was truncated —{' '}
                  <button
                    type="button"
                    onClick={() => void window.electron.openExternal(hfUrl)}
                    className={cx('underline', WEIGHT.semibold, TONE_TEXT.accent, FOCUS, MOTION)}
                  >
                    read the full page on huggingface.co/{repoId}
                  </button>
                </div>
              )}
              {card.readmeMarkdown != null ? (
                <Panel>
                  <div className="min-w-0 break-words" data-testid="mlx-model-card-readme">
                    <MarkdownContent content={card.readmeMarkdown} />
                  </div>
                </Panel>
              ) : (
                <p className={TYPE.bodyMuted}>This repo has no README.</p>
              )}

              <Panel
                title="Files"
                count={card.files.length}
                headerRight={
                  <span className={cx('text-lz-meta text-lz-ink', WEIGHT.semibold, TNUM)}>
                    {formatBytesShort(card.totalBytes)} total
                  </span>
                }
                padded={false}
              >
                <DataTable
                  dense
                  aria-label="Repository files"
                  columns={FILE_COLUMNS}
                  rows={card.files}
                  rowKey={(file) => file.path}
                  empty={<EmptyState title="No files" body="The repo tree lists nothing." />}
                />
              </Panel>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

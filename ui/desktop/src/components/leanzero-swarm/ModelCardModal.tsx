import React, { useEffect, useState } from 'react';
import { Download, ExternalLink, Loader2, X } from 'lucide-react';
import { Button } from '../ui/button';
import MarkdownContent from '../MarkdownContent';
import { errorMessage } from '../../utils/conversionUtils';
import {
  mlxEngineModelCard,
  type MlxDownloadProgress,
  type MlxModelCard,
} from '../../acp/mlx-engine';
import {
  AZURE,
  Chip,
  DownloadProgressRow,
  GREEN,
  RED,
  SLATE,
  SolidBanner,
  TEAL,
  VIOLET,
  authorHue,
  formatBytesShort,
  formatCount,
  formatDate,
  type DownloadLifecycleHandlers,
} from './primitives';

function HeaderFact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
        {label}
      </span>
      {children}
    </span>
  );
}

/**
 * Fullscreen model card for one Hugging Face repo — a custom modal (no iframe/webview,
 * no native chrome). Header carries the repo facts and the SAME download lifecycle the
 * browse row shows; the body renders the README through the app's chat markdown renderer
 * plus the exact file listing. Esc or ✕ closes.
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

  return (
    <div
      className="fixed inset-0 z-[90] flex flex-col bg-background-primary"
      role="dialog"
      aria-modal="true"
      aria-label={`Model card for ${repoId}`}
      data-testid="mlx-model-card-modal"
    >
      {/* Header strip — pt-10 clears the frameless window's 32px titlebar drag strip,
          the macOS traffic lights, and the floating sidebar toggle (caught live: the
          repo id rendered under the toggle icon). */}
      <div className="border-b border-border-primary bg-background-secondary px-4 pb-3 pt-10 md:px-6">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
              <h2 className="min-w-0 break-all font-mono text-lg font-bold text-text-primary">
                {repoId}
              </h2>
              <Chip color={authorHue(author)} title={`Published by ${author}`}>
                {author}
              </Chip>
              <Button
                size="xs"
                variant="outline"
                onClick={() => void window.electron.openExternal(hfUrl)}
                className="rounded"
                aria-label={`Open ${repoId} on huggingface.co`}
                title={hfUrl}
              >
                <ExternalLink className="w-3 h-3" />
                huggingface.co
              </Button>
            </div>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1.5">
              {card && (
                <>
                  <HeaderFact label="Downloads">
                    <span
                      className="text-xs font-bold tabular-nums"
                      style={{ color: AZURE }}
                      title={`${card.downloads.toLocaleString()} downloads`}
                    >
                      ↓ {formatCount(card.downloads)}
                    </span>
                  </HeaderFact>
                  <HeaderFact label="Likes">
                    <span
                      className="text-xs font-bold tabular-nums"
                      style={{ color: VIOLET }}
                      title={`${card.likes.toLocaleString()} likes`}
                    >
                      ♥ {formatCount(card.likes)}
                    </span>
                  </HeaderFact>
                  {card.license && (
                    <HeaderFact label="License">
                      <Chip color={SLATE}>{card.license}</Chip>
                    </HeaderFact>
                  )}
                  {card.createdAt && (
                    <HeaderFact label="Created">
                      <span className="text-xs tabular-nums text-text-primary">
                        {formatDate(card.createdAt)}
                      </span>
                    </HeaderFact>
                  )}
                  {card.lastModified && (
                    <HeaderFact label="Updated">
                      <span className="text-xs tabular-nums text-text-primary">
                        {formatDate(card.lastModified)}
                      </span>
                    </HeaderFact>
                  )}
                  <HeaderFact label="Size (exact)">
                    <Chip color={TEAL} title="Exact sum of every file the repo tree lists">
                      {formatBytesShort(card.totalBytes)}
                    </Chip>
                  </HeaderFact>
                </>
              )}
              {!card && !error && (
                <span className="inline-flex items-center gap-1.5 text-xs text-text-secondary">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  loading model card…
                </span>
              )}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {!progress && (
              <Button
                size="sm"
                onClick={onDownload}
                className="rounded font-bold text-white hover:opacity-90"
                style={{ backgroundColor: GREEN }}
                aria-label={`Download ${repoId}`}
              >
                <Download className="w-3.5 h-3.5" />
                Download
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              onClick={onClose}
              className="rounded"
              aria-label="Close model card"
            >
              <X className="w-4 h-4" />
            </Button>
          </div>
        </div>
        {startError && (
          <div className="mt-1 break-words text-xs font-semibold" style={{ color: RED }}>
            {startError}
          </div>
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

      {/* Body */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 md:px-6">
        <div className="mx-auto flex max-w-[880px] flex-col gap-4 pb-8">
          {error && <SolidBanner color={RED} label="Model card failed" text={error} />}
          {card && (
            <>
              {card.tags.length > 0 && (
                <div className="flex flex-wrap items-center gap-1.5">
                  {card.tags.map((tag) => (
                    <Chip key={tag} color={SLATE}>
                      {tag}
                    </Chip>
                  ))}
                </div>
              )}
              {card.readmeTruncated && (
                <div className="rounded border border-border-primary bg-background-secondary px-3 py-2 text-sm text-text-primary">
                  This README was truncated —{' '}
                  <button
                    type="button"
                    onClick={() => void window.electron.openExternal(hfUrl)}
                    className="font-semibold underline"
                    style={{ color: AZURE }}
                  >
                    read the full page on huggingface.co/{repoId}
                  </button>
                </div>
              )}
              {card.readmeMarkdown != null ? (
                <div className="min-w-0 break-words">
                  <MarkdownContent content={card.readmeMarkdown} />
                </div>
              ) : (
                <div className="text-sm text-text-secondary">This repo has no README.</div>
              )}

              <div className="overflow-hidden rounded border border-border-primary">
                <div className="flex flex-wrap items-center gap-2 border-b border-border-primary bg-background-secondary px-3 py-2">
                  <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                    Files
                  </span>
                  <Chip color={AZURE}>{card.files.length}</Chip>
                  <span className="ml-auto text-xs font-bold tabular-nums" style={{ color: TEAL }}>
                    {formatBytesShort(card.totalBytes)} total
                  </span>
                </div>
                <div>
                  {card.files.map((file) => (
                    <div
                      key={file.path}
                      className="flex min-w-0 items-center gap-3 border-t border-border-primary px-3 py-1.5 first:border-t-0"
                    >
                      <span className="min-w-0 flex-1 break-all font-mono text-xs text-text-primary">
                        {file.path}
                      </span>
                      <span className="shrink-0 font-mono text-xs font-bold tabular-nums text-text-secondary">
                        {formatBytesShort(file.sizeBytes)}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

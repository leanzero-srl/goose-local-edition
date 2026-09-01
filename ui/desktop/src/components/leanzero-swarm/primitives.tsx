import React from 'react';
import { Loader2, Pause, Play, X } from 'lucide-react';
import type { MlxDownloadProgress } from '../../acp/mlx-engine';
import {
  Button,
  Chip as StudioChip,
  StatusDot,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type Tone,
} from '../lz';

/**
 * A thin ADAPTER over the LeanZero Studio primitives (`src/components/lz`, ui/desktop/DESIGN.md):
 * the formatters, the SolidBanner MlxEngineView and ModelCardModal still consume, and the
 * download row. Every visual inside is a Studio token or primitive — no colour constant leaves
 * this module, so nothing here can reach the DOM as an inline style.
 */

export const GB = 1024 * 1024 * 1024;

export function formatGb(bytes: number): string {
  if (bytes <= 0) return 'unknown size';
  const gb = bytes / GB;
  if (gb >= 10) return `${gb.toFixed(0)} GB`;
  if (gb >= 0.1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
}

export function formatBytesShort(bytes: number): string {
  if (bytes <= 0) return '0 B';
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < GB) return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
  return `${(bytes / GB).toFixed(2)} GB`;
}

export function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

export function formatDate(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  return new Date(t).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

/**
 * A banner in the Studio register: a surface card carrying a solid status dot, a toned label and
 * the message in body ink. Red carries backend text VERBATIM — never paraphrased. The same
 * markup as studio.tsx's ToneBanner, kept under this name for its two remaining importers.
 */
export function SolidBanner({
  tone,
  label,
  text,
  action,
}: {
  tone: Tone;
  label: string;
  text: string;
  action?: React.ReactNode;
}) {
  return (
    <div
      className={cx('flex items-center gap-3 px-4 py-3', SURFACE.card)}
      role={tone === 'err' ? 'alert' : 'status'}
      data-tone={tone}
    >
      <StatusDot tone={tone} label={label} size={10} />
      <span className={cx('shrink-0 text-lz-meta', WEIGHT.semibold, TONE_TEXT[tone])}>{label}</span>
      <span className={cx('min-w-0 flex-1 break-words', TYPE.body)}>{text}</span>
      {action}
    </div>
  );
}

export interface DownloadLifecycleHandlers {
  onPause: () => void;
  onResume: () => void;
  onCancel: () => void;
}

const DOWNLOAD_STATE_TONE: Record<MlxDownloadProgress['state'], Tone> = {
  queued: 'stopped',
  downloading: 'accent',
  paused: 'stopped',
  done: 'ok',
  failed: 'err',
  cancelled: 'stopped',
};

/**
 * One tracked download: real-byte bar, state chip, lifecycle actions. Rendered under browse
 * rows, under incomplete local models, and mirrored inside the model-card modal — ONE component
 * so every surface tells the same truth. Cancel DELETES the partial repo dir (the backend does),
 * and its title says so.
 */
export function DownloadProgressRow({
  repoId,
  progress,
  onPause,
  onResume,
  onCancel,
}: {
  repoId: string;
  progress: MlxDownloadProgress;
} & DownloadLifecycleHandlers) {
  const pct =
    progress.totalBytes > 0
      ? Math.min(100, (progress.downloadedBytes / progress.totalBytes) * 100)
      : 0;
  const active = progress.state === 'queued' || progress.state === 'downloading';
  const paused = progress.state === 'paused';
  const failed = progress.state === 'failed';
  const barTone: Tone = paused ? 'stopped' : failed ? 'err' : 'accent';
  const stateTone = DOWNLOAD_STATE_TONE[progress.state];
  return (
    <div className="mt-2 flex flex-col gap-1.5" data-testid={`mlx-download-${repoId}`}>
      <div className="flex flex-wrap items-center gap-2">
        <div
          className={cx('h-2 min-w-[160px] flex-1 overflow-hidden', RADIUS.pill, SURFACE.inset)}
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`Download progress for ${repoId}`}
        >
          <div className={cx('h-full', TONE_DOT[barTone])} style={{ width: `${pct}%` }} />
        </div>
        <span className={cx('shrink-0 text-lz-meta', WEIGHT.semibold, TNUM, TONE_TEXT[barTone])}>
          {formatBytesShort(progress.downloadedBytes)}
          {progress.totalBytes > 0 ? ` / ${formatBytesShort(progress.totalBytes)}` : ''}
        </span>
        {active && (
          <Button
            size="sm"
            variant="secondary"
            icon={<Pause />}
            onClick={(e) => {
              e.stopPropagation();
              onPause();
            }}
            aria-label={`Pause ${repoId}`}
          >
            Pause
          </Button>
        )}
        {(paused || failed) && (
          <Button
            size="sm"
            variant="secondary"
            icon={<Play />}
            onClick={(e) => {
              e.stopPropagation();
              onResume();
            }}
            aria-label={`Resume ${repoId}`}
          >
            Resume
          </Button>
        )}
        {(active || paused || failed) && (
          <Button
            size="sm"
            variant="ghost"
            icon={<X />}
            onClick={(e) => {
              e.stopPropagation();
              onCancel();
            }}
            aria-label={`Cancel ${repoId}`}
            title="Cancel and DELETE the partial download from disk"
          >
            Cancel
          </Button>
        )}
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {progress.state !== 'cancelled' && (
          <StudioChip
            tone={stateTone}
            icon={
              progress.state === 'downloading' ? <Loader2 className="animate-spin" /> : undefined
            }
          >
            {progress.state}
          </StudioChip>
        )}
        {progress.currentFile && (
          <span className="truncate font-mono text-lz-mono text-lz-ink-3">
            {progress.currentFile}
          </span>
        )}
      </div>
      {progress.restartedFiles != null && progress.restartedFiles.length > 0 && (
        <div
          className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
          title={progress.restartedFiles.join('\n')}
        >
          restarted from zero: {progress.restartedFiles.length} file(s)
        </div>
      )}
      {failed && (
        <div className={cx('break-words text-lz-body', WEIGHT.semibold, TONE_TEXT.err)}>
          {progress.error ?? 'Download failed.'}
        </div>
      )}
    </div>
  );
}

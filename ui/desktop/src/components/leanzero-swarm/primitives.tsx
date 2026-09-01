import React from 'react';
import { Loader2, Minus, Pause, Play, Plus, X } from 'lucide-react';
import type { MlxDownloadProgress } from '../../acp/mlx-engine';
import {
  Button,
  Chip as StudioChip,
  StatusDot,
  FOCUS,
  MOTION,
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
 * A thin ADAPTER over the LeanZero Studio primitives (`src/components/lz`, ui/desktop/DESIGN.md).
 * The exported names and signatures are the ones AddNodeDialog, LeanZeroLinkSection,
 * LeanZeroSwarmView, SwarmNodesSection and the tests already consume; every visual inside is a
 * Studio token or primitive. The colour constants stay CSS strings because those consumers use
 * them as inline styles; `toneForColor` is the one join that reads them back as tones.
 */

/** The single accent: primary actions, the active segment, the live progress fill. */
export const AZURE = 'var(--color-action-solid, #1d4ed8)';
/** Status triad FILLS — they carry white text. */
export const GREEN = 'var(--color-status-ok-solid, #15803d)';
export const RED = 'var(--color-status-err-solid, #dc2626)';
export const SLATE = 'var(--color-status-stopped-solid, #475569)';
export const AMBER = 'var(--color-status-warn, #d97706)';
export const INK_DARK = '#1a1a1a';

/** The palette constants above read back as Studio tones; anything else is not a tone. */
export function toneForColor(color: string | undefined): Tone | null {
  switch (color) {
    case AZURE:
      return 'accent';
    case GREEN:
      return 'ok';
    case RED:
      return 'err';
    case SLATE:
      return 'stopped';
    case AMBER:
      return 'warn';
    default:
      return null;
  }
}

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
 * Two chip registers, both the Studio Chip. FILLED (`tone`, or a palette `color`) is for
 * SEMANTIC state — mounted, loading, failed, the single accent. QUIET (the default, or an
 * explicit `quiet`) is for METADATA: an outline in ink-3, no fill, so a row of attributes reads
 * as text, not as a pile of stickers. A colour outside the palette is metadata, not a state.
 * `ink` is accepted for the older callers; the tone token carries the measured ink.
 */
export function Chip({
  color,
  tone,
  quiet = false,
  children,
  title,
}: {
  color?: string;
  tone?: Tone;
  /** Accepted for older callers; the tone's own ink applies. */
  ink?: string;
  quiet?: boolean;
  children: React.ReactNode;
  title?: string;
}) {
  const resolved = quiet ? undefined : (tone ?? toneForColor(color) ?? undefined);
  return (
    <StudioChip tone={resolved} title={title}>
      {children}
    </StudioChip>
  );
}

/**
 * A banner in the Studio register: a surface card carrying a solid status dot, a toned label and
 * the message in body ink. Red carries backend text VERBATIM — never paraphrased. A colour outside
 * the palette reads as err: the one such caller (AddNodeDialog's failed banner) is an error.
 */
export function SolidBanner({
  color,
  tone: toneProp,
  label,
  text,
  action,
}: {
  color?: string;
  tone?: Tone;
  label: string;
  text: string;
  action?: React.ReactNode;
}) {
  const tone = toneProp ?? toneForColor(color) ?? 'err';
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

const STEP_BUTTON = cx(
  'flex size-7 shrink-0 items-center justify-center bg-lz-surface text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION
);

/**
 * The −/n/+ stepper for a node's relative task share (1–9). A custom control, never a native
 * slider or select; the value is the accent in tabular figures so a column of them lines up.
 * Shared by the Nodes card and the add-node dialog so both write the same range.
 */
export function WeightStepper({
  value,
  onChange,
  label = 'weight',
}: {
  value: number;
  onChange: (v: number) => void;
  label?: string;
}) {
  const clamp = (v: number) => Math.max(1, Math.min(9, v));
  return (
    <span className="inline-flex items-center gap-1">
      <button
        type="button"
        onClick={() => onChange(clamp(value - 1))}
        className={STEP_BUTTON}
        aria-label={`Less work (${label})`}
      >
        <Minus />
      </button>
      <span className={cx('w-6 text-center text-lz-body text-lz-accent', WEIGHT.semibold, TNUM)}>
        {value}
      </span>
      <button
        type="button"
        onClick={() => onChange(clamp(value + 1))}
        className={STEP_BUTTON}
        aria-label={`More work (${label})`}
      >
        <Plus />
      </button>
    </span>
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

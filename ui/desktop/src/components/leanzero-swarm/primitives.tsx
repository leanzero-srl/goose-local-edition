import React from 'react';
import { Loader2, Pause, Play, X } from 'lucide-react';
import { Button } from '../ui/button';
import type { MlxDownloadProgress } from '../../acp/mlx-engine';

// The LeanZero token doctrine (src/styles/main.css, `.local-edition`): ONE accent, the status
// triad for state, and nothing else coloured. Every constant here routes through a theme token
// with the light-theme value as its fallback (this window also runs without `.local-edition`,
// where a bare var() resolves to nothing and a solid fill silently turns transparent).
// Solid, saturated colours only — never a tint, never a left accent rail, never a native control.
/** The single accent: primary actions, the active segment, the live progress fill. */
export const AZURE = 'var(--color-action-solid, #1d4ed8)';
/** Status triad FILLS — they carry white text. */
export const GREEN = 'var(--color-status-ok-solid, #15803d)';
export const RED = 'var(--color-status-err-solid, #dc2626)';
export const SLATE = 'var(--color-status-stopped-solid, #475569)';
/** Warn is the FOREGROUND token on purpose: it pairs with INK_DARK at 5.4:1, which the darker
 *  -solid fill cannot (3.5:1). Every AMBER fill in this window carries dark ink. */
export const AMBER = 'var(--color-status-warn, #d97706)';
export const INK_DARK = '#1a1a1a';
/** @deprecated Decorative hues carry no meaning under the doctrine. Aliased to the accent /
 *  the neutral so remaining callers resolve to a sanctioned colour; delete once none remain. */
export const TEAL = AZURE;
export const VIOLET = SLATE;

/** A publisher is NOT a node: it takes no hue from the node ramp. One neutral, whatever the
 *  author — callers should prefer rendering the author as text or a quiet chip. */
export function authorHue(_author: string): string {
  return SLATE;
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
 * Two chip registers. FILLED (`color`) is reserved for SEMANTIC state — mounted, loading,
 * failed, the single accent — and carries its ink. QUIET is for METADATA (a publisher, a
 * quant, a count): a hairline outline in the secondary text colour, no fill, so a row of
 * attributes reads as text, not as a pile of stickers. Both are 11px, normal case,
 * tabular figures — uppercase-with-tracking belongs to section headers only.
 */
export function Chip({
  color,
  ink = '#ffffff',
  quiet = false,
  children,
  title,
}: {
  color?: string;
  ink?: string;
  quiet?: boolean;
  children: React.ReactNode;
  title?: string;
}) {
  if (quiet || !color) {
    return (
      <span
        title={title}
        className="inline-flex shrink-0 items-center gap-1 rounded border border-border-primary px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-text-secondary"
      >
        {children}
      </span>
    );
  }
  return (
    <span
      title={title}
      className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-semibold tabular-nums"
      style={{ backgroundColor: color, color: ink }}
    >
      {children}
    </span>
  );
}

/** Solid full-width banner. Red carries backend text VERBATIM — never paraphrased. */
export function SolidBanner({
  color,
  label,
  text,
  action,
}: {
  color: string;
  label: string;
  text: string;
  action?: React.ReactNode;
}) {
  const dark = color === AMBER;
  return (
    <div
      className="flex items-center gap-3 rounded px-4 py-3"
      style={{ backgroundColor: color }}
      role="alert"
    >
      <span
        className="shrink-0 text-[10px] font-black uppercase tracking-widest"
        style={{ color: dark ? INK_DARK : '#ffffff' }}
      >
        {label}
      </span>
      <span
        className="min-w-0 flex-1 break-words text-sm font-semibold"
        style={{ color: dark ? INK_DARK : '#ffffff' }}
      >
        {text}
      </span>
      {action}
    </div>
  );
}

// Custom −/number/+ stepper for a node's relative task-share weight (no native slider/select, per
// UI rules). Shared by the Nodes card and the add-node dialog so both write the same 1-9 range.
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
  const btn =
    'h-6 w-6 flex items-center justify-center border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors leading-none';
  return (
    <div className="flex items-center gap-1.5">
      <button
        type="button"
        onClick={() => onChange(clamp(value - 1))}
        className={btn}
        style={{ borderRadius: 3 }}
        aria-label={`Less work (${label})`}
      >
        −
      </button>
      <span className="w-4 text-center font-bold tabular-nums" style={{ color: AZURE }}>
        {value}
      </span>
      <button
        type="button"
        onClick={() => onChange(clamp(value + 1))}
        className={btn}
        style={{ borderRadius: 3 }}
        aria-label={`More work (${label})`}
      >
        +
      </button>
    </div>
  );
}

export interface DownloadLifecycleHandlers {
  onPause: () => void;
  onResume: () => void;
  onCancel: () => void;
}

/**
 * One tracked download: real-byte bar, state chip, lifecycle actions. Rendered under
 * browse rows, under incomplete local models, and mirrored inside the model-card modal —
 * ONE component so every surface tells the same truth. Cancel is destructive now (the
 * backend deletes the partial repo dir), so it is solid red and says so.
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
  const barColor = paused ? SLATE : failed ? RED : AZURE;
  return (
    <div className="mt-2 flex flex-col gap-1.5" data-testid={`mlx-download-${repoId}`}>
      <div className="flex flex-wrap items-center gap-2">
        <div
          className="h-2.5 min-w-[160px] flex-1 overflow-hidden rounded border border-border-primary"
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`Download progress for ${repoId}`}
        >
          <div className="h-full" style={{ width: `${pct}%`, backgroundColor: barColor }} />
        </div>
        <span className="shrink-0 text-xs font-bold tabular-nums" style={{ color: barColor }}>
          {formatBytesShort(progress.downloadedBytes)}
          {progress.totalBytes > 0 ? ` / ${formatBytesShort(progress.totalBytes)}` : ''}
        </span>
        {active && (
          <Button
            size="xs"
            onClick={(e) => {
              e.stopPropagation();
              onPause();
            }}
            variant="outline"
            className="shrink-0 rounded font-bold"
            aria-label={`Pause ${repoId}`}
          >
            <Pause className="w-3 h-3" />
            Pause
          </Button>
        )}
        {(paused || failed) && (
          <Button
            size="xs"
            onClick={(e) => {
              e.stopPropagation();
              onResume();
            }}
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: AZURE }}
            aria-label={`Resume ${repoId}`}
          >
            <Play className="w-3 h-3" />
            Resume
          </Button>
        )}
        {(active || paused || failed) && (
          <Button
            size="xs"
            onClick={(e) => {
              e.stopPropagation();
              onCancel();
            }}
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: RED }}
            aria-label={`Cancel ${repoId}`}
            title="Cancel and DELETE the partial download from disk"
          >
            <X className="w-3 h-3" />
            Cancel
          </Button>
        )}
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {progress.state === 'queued' && <Chip color={SLATE}>queued</Chip>}
        {progress.state === 'downloading' && (
          <Chip color={AZURE}>
            <Loader2 className="h-2.5 w-2.5 animate-spin" />
            downloading
          </Chip>
        )}
        {paused && <Chip color={SLATE}>paused</Chip>}
        {progress.state === 'done' && <Chip color={GREEN}>done</Chip>}
        {failed && <Chip color={RED}>failed</Chip>}
        {progress.currentFile && (
          <span className="truncate font-mono text-[11px] text-text-secondary">
            {progress.currentFile}
          </span>
        )}
      </div>
      {progress.restartedFiles != null && progress.restartedFiles.length > 0 && (
        <div
          className="text-xs font-semibold"
          style={{ color: AMBER }}
          title={progress.restartedFiles.join('\n')}
        >
          restarted from zero: {progress.restartedFiles.length} file(s)
        </div>
      )}
      {failed && (
        <div className="break-words text-xs font-semibold" style={{ color: RED }}>
          {progress.error ?? 'Download failed.'}
        </div>
      )}
    </div>
  );
}

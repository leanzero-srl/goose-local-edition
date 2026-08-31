import React from 'react';
import { Loader2, Pause, Play, X } from 'lucide-react';
import { Button } from '../ui/button';
import type { MlxDownloadProgress } from '../../acp/mlx-engine';

// Solid saturated palette — the benchmark register (BenchmarkView/ScoringDetail): full
// borders, bg-background-secondary strips, solid chips. Never faded tints, never a left
// accent rail, never a native control.
export const AZURE = '#2e8bff';
export const GREEN = '#2ecc71';
export const AMBER = '#f5a623';
export const RED = '#e5484d';
export const SLATE = '#64748b';
export const VIOLET = '#7c3aed';
export const TEAL = 'var(--color-block-teal, #13bbaf)';
export const INK_DARK = '#1a1a1a';

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

// Distinct solid hues for author chips — deterministic per author, full-rainbow, no washes.
// The node ramp lives under `.local-edition`; this window also runs in builds without that
// class, where a bare var() resolves to NOTHING — every node var carries a fallback.
const AUTHOR_HUES = [
  'var(--color-node-1, #1d4ed8)',
  'var(--color-node-2, #0891b2)',
  'var(--color-node-3, #7c3aed)',
  'var(--color-node-4, #ea580c)',
  'var(--color-node-5, #db2777)',
  'var(--color-node-6, #16a34a)',
];

export function authorHue(author: string): string {
  let h = 0;
  for (let i = 0; i < author.length; i += 1) h = (h * 31 + author.charCodeAt(i)) >>> 0;
  return AUTHOR_HUES[h % AUTHOR_HUES.length];
}

export function Chip({
  color,
  ink = '#ffffff',
  children,
  title,
}: {
  color: string;
  ink?: string;
  children: React.ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide"
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
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: SLATE }}
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
            style={{ backgroundColor: GREEN }}
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

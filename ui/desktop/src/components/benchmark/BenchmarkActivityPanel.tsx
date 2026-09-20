import { useState } from 'react';
import { Check, ChevronDown, ChevronRight, Loader2 } from 'lucide-react';
import type { BenchmarkActivity } from '../../benchActivity';
import type { BenchmarkPhase } from '../../benchPhase';
import { Chip, Panel, StatusDot, TYPE, TNUM, cx } from '../lz';
const phases = [
  { key: 'boot', label: 'Prepare' },
  { key: 'build', label: 'Model build' },
  { key: 'score', label: 'Scoring' },
  { key: 'done', label: 'Finalize' },
] as const;
const titles: Record<BenchmarkPhase, string> = {
  boot: 'Preparing benchmark',
  build: 'Model implementation',
  score: 'Scorer running',
  done: 'Finalizing result',
};
const toolTitles: Record<string, string> = {
  shell: 'Shell command',
  write: 'Write file',
  edit: 'Edit file',
  read_image: 'Inspect image',
};
function elapsed(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return seconds >= 3600
    ? `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`
    : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

function ActionDetail({ detail, truncated }: { detail: string; truncated?: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const shortened = detail.length > 200 || detail.split('\n').length > 3;
  const preview = detail.split('\n').slice(0, 3).join('\n').slice(0, 200);
  return (
    <div className="mt-1 min-w-0">
      <p className="whitespace-pre-wrap break-all font-mono text-xs text-lz-ink-2">
        {expanded ? detail : preview}
        {(shortened && !expanded) || truncated ? '…' : ''}
      </p>
      {shortened && (
        <button
          type="button"
          className="mt-1 text-xs font-medium text-lz-accent"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? 'Collapse action' : 'Expand action'}
        </button>
      )}
    </div>
  );
}

export function BenchmarkActivityPanel({
  phase,
  activity,
  now,
  startedAt,
  workdir,
}: {
  phase: BenchmarkPhase;
  activity: BenchmarkActivity;
  now: number;
  startedAt: number | null;
  workdir: string | null;
}) {
  const [rawOpen, setRawOpen] = useState(false);
  const [locationOpen, setLocationOpen] = useState(false);
  const active = phases.findIndex((p) => p.key === phase);
  const entries = activity.entries.slice(-6).reverse();
  return (
    <Panel
      title="Benchmark activity"
      className="min-w-0"
      headerRight={
        <>
          <StatusDot tone="accent" live label="run in progress" />
          <span className={cx(TYPE.meta, TNUM)}>
            {startedAt ? elapsed(now - startedAt) : 'Starting…'}
          </span>
        </>
      }
    >
      <div className="min-w-0 space-y-4">
        <div className="flex flex-wrap items-center gap-2" aria-label="Benchmark stages">
          {phases.map((p, i) => (
            <Chip
              key={p.key}
              tone={i < active ? 'ok' : i === active ? 'accent' : undefined}
              icon={
                i < active ? (
                  <Check />
                ) : i === active ? (
                  <Loader2 className="animate-spin" />
                ) : undefined
              }
            >
              {p.label}
            </Chip>
          ))}
        </div>
        <div>
          <h3 className="text-base font-semibold text-lz-ink">{titles[phase]}</h3>
          <p className={cx(TYPE.meta, 'mt-1')}>
            {activity.lastOutputAt === null
              ? 'No console output received yet.'
              : `Last console output ${elapsed(now - activity.lastOutputAt)} ago.`}
          </p>
        </div>
        {entries.length > 0 ? (
          <div>
            <p className={cx(TYPE.meta, 'mb-2')}>Latest action excerpts · {entries.length}</p>
            <ol className="space-y-2" aria-label="Recent benchmark actions">
              {entries.map((entry) => (
                <li key={entry.id} className="min-w-0 rounded-lg border border-lz-border px-3 py-2">
                  <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
                    <span className="text-sm font-medium text-lz-ink">
                      {entry.kind === 'tool'
                        ? (toolTitles[entry.title] ?? `Tool: ${entry.title}`)
                        : entry.title}
                    </span>
                    <span className={cx(TYPE.meta, TNUM)}>
                      {new Date(entry.at).toLocaleTimeString(undefined, {
                        hour: '2-digit',
                        minute: '2-digit',
                        second: '2-digit',
                      })}
                    </span>
                  </div>
                  {entry.detail && (
                    <ActionDetail detail={entry.detail} truncated={entry.detailTruncated} />
                  )}
                </li>
              ))}
            </ol>
          </div>
        ) : (
          <p className={TYPE.bodyMuted}>
            No structured action has been reported yet. Console output is available in the details
            below.
          </p>
        )}
        <div className="min-w-0 border-t border-lz-border pt-3">
          <button
            type="button"
            aria-expanded={locationOpen}
            onClick={() => setLocationOpen(!locationOpen)}
            className="flex max-w-full items-center gap-2 text-sm text-lz-ink"
          >
            {locationOpen ? (
              <ChevronDown className="size-4 shrink-0" />
            ) : (
              <ChevronRight className="size-4 shrink-0" />
            )}
            <span>Run location</span>
          </button>
          <p className="mt-1 break-all text-xs text-lz-ink-3">
            {workdir?.split(/[\\/]/).pop() ?? 'Waiting for run directory'}
          </p>
          {locationOpen && (
            <p className="mt-2 break-all rounded-lg bg-lz-surface p-3 font-mono text-xs text-lz-ink-2">
              {workdir ?? 'Not available'}
            </p>
          )}
          {startedAt && (
            <p className={cx(TYPE.meta, 'mt-2')}>Started {new Date(startedAt).toLocaleString()}</p>
          )}
        </div>
        <div className="min-w-0">
          <button
            type="button"
            aria-expanded={rawOpen}
            onClick={() => setRawOpen(!rawOpen)}
            className="flex items-center gap-2 text-sm text-lz-ink"
          >
            {rawOpen ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
            Console details
          </button>
          {rawOpen && (
            <div className="mt-2 min-w-0">
              <p className={cx(TYPE.meta, 'mb-2')}>
                Recent console tail · up to 16,000 characters. The model transcript is saved as
                engine-console.log in the run directory.
              </p>
              <pre className="max-h-64 max-w-full overflow-auto whitespace-pre-wrap break-all rounded-lg border border-lz-border p-3 text-xs text-lz-ink-2">
                {activity.raw || 'No console output received yet.'}
              </pre>
            </div>
          )}
        </div>
      </div>
    </Panel>
  );
}

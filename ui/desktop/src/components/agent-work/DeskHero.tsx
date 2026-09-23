import { useState, type ReactNode } from 'react';
import { Check, Copy, Pause, Play, Square, Trash2, Zap } from 'lucide-react';
import { Button, TNUM, TONE_FILL, TONE_TEXT, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { countdown, fmtClock, type DeskModel } from './agentWorkModel';

const STATUS_TONE: Record<string, Tone> = {
  ticking: 'ok',
  waiting: 'accent',
  paused: 'warn',
  holding: 'warn',
  starting: 'accent',
  stopped: 'stopped',
  stale: 'err',
  unknown: 'stopped',
};

export interface DeskControls {
  busy: boolean;
  onStart: () => void;
  onRunOnce?: () => void;
  onStop: () => void;
  onTickNow: () => void;
  onPause: (paused: boolean) => void;
}

/**
 * The desk at a glance: a solid status tile carrying the one number that matters now (the countdown,
 * the live tick, or "stopped"), the desk's name and schedule, the ONE action that fits its state,
 * what waits on the person, and what the desk has done so far. The path is metadata, not a title.
 */
export function DeskHero({
  model,
  title,
  schedule,
  dir,
  plannerModel,
  controls,
  onRemove,
  onReviewNeeds,
}: {
  model: DeskModel;
  title: string;
  schedule: string;
  dir: string;
  plannerModel: string;
  controls: DeskControls;
  onRemove: () => void;
  onReviewNeeds: () => void;
}) {
  const stopped = model.liveness === 'stopped';
  const stale = model.liveness === 'stale';
  const ticking = model.status === 'ticking' && !stopped;
  const tone: Tone = stale ? 'err' : (STATUS_TONE[model.status] ?? 'stopped');
  const statusLabel = stale ? 'Stale, no heartbeat' : cap(model.status);
  const needs = model.openAsks.length + model.pendingDrafts.length;
  return (
    <section
      data-testid="desk-hero"
      className="overflow-hidden rounded-lz-card border border-lz-border bg-lz-surface"
    >
      <div className="grid grid-cols-[minmax(0,208px)_minmax(0,1fr)]">
        <div
          data-testid="desk-status"
          data-tone={tone}
          className={cx('flex min-w-0 flex-col justify-between gap-4 p-5', TONE_FILL[tone])}
        >
          <div className={cx('flex items-center gap-2 text-lz-body', WEIGHT.semibold)}>
            <span
              aria-hidden
              className={cx(
                'inline-block size-2 shrink-0 rounded-lz-pill bg-current',
                ticking && 'animate-lz-live'
              )}
            />
            {statusLabel}
          </div>
          <div>
            <div
              className={cx('break-words text-lz-display leading-none', TNUM)}
              data-testid="next-tick-countdown"
            >
              {stopped
                ? 'Not scheduled'
                : ticking
                  ? `tick ${model.tick} live`
                  : countdown(model.nextTickInMs)}
            </div>
            <div className="mt-2 break-words text-lz-meta">
              {stopped
                ? 'start the desk to schedule one'
                : ticking
                  ? `started ${fmtClock(model.nextTickAt)}`
                  : `${model.nextTickLocal || fmtClock(model.nextTickAt)} — ${model.nextTickReason}`}
            </div>
          </div>
        </div>
        <div className="flex min-w-0 flex-col gap-3 p-5">
          <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
            <div className="min-w-0 flex-1 basis-64">
              <h2 className={cx(TYPE.h1, 'break-words')}>{title}</h2>
              <p className={cx(TYPE.bodyMuted, 'mt-1')}>
                {schedule}
                {!stopped && (
                  <span className={cx(model.windowOpen ? TONE_TEXT.ok : TONE_TEXT.stopped)}>
                    {model.windowOpen ? ' · window open' : ' · window closed'}
                  </span>
                )}
              </p>
            </div>
            <HeroActions model={model} controls={controls} />
          </div>
          <PathLine dir={dir} onRemove={onRemove} busy={controls.busy} />
        </div>
      </div>
      {model.holdReason && (
        <div
          className={cx('px-5 py-2.5 text-lz-body', TONE_FILL.warn, WEIGHT.medium)}
          data-testid="hold-reason"
        >
          Held: {model.holdReason}
        </div>
      )}
      {needs > 0 && (
        <button
          type="button"
          onClick={onReviewNeeds}
          data-testid="needs-you-banner"
          className={cx(
            'flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-5 py-3 text-left hover:bg-lz-warn',
            TONE_FILL.warn
          )}
        >
          <span className={cx('text-lz-h2', TNUM)}>{needs}</span>
          <span className={cx('min-w-0 flex-1 text-lz-body', WEIGHT.semibold)}>
            {needsLine(model.openAsks.length, model.pendingDrafts.length)}
          </span>
          <span className={cx('text-lz-body underline underline-offset-2', WEIGHT.semibold)}>
            Review
          </span>
        </button>
      )}
      <dl
        data-testid="desk-metrics"
        className="flex flex-wrap items-end gap-x-8 gap-y-3 border-t border-lz-border px-5 py-3"
      >
        <Metric label="Ticks" value={model.totals.ticks} />
        <Metric label="Lanes run" value={model.totals.lanes} />
        <Metric label="Lane-minutes" value={model.totals.laneMinutes.toFixed(1)} />
        <Metric label="Drafts staged" value={model.totals.staged} />
        <Metric
          label="Posted"
          value={model.totals.posted}
          tone={model.totals.posted > 0 ? 'ok' : undefined}
        />
        <Metric
          label="Asks raised"
          value={model.totals.asks}
          tone={model.totals.asks > 0 && model.openAsks.length > 0 ? 'warn' : undefined}
        />
        <div className="min-w-0">
          <dt className={TYPE.meta}>Orchestrator</dt>
          <dd className={cx(TYPE.body, WEIGHT.medium, 'break-all')}>{plannerModel || '—'}</dd>
        </div>
        {needs === 0 && (
          <p
            className={cx(TYPE.bodyMuted, 'ml-auto flex items-center gap-1.5 [&_svg]:size-4')}
            data-testid="needs-you-quiet"
          >
            <Check aria-hidden className={TONE_TEXT.ok} />
            Nothing waits on you
          </p>
        )}
      </dl>
    </section>
  );
}

function HeroActions({ model, controls }: { model: DeskModel; controls: DeskControls }) {
  const { busy, onStart, onRunOnce, onStop, onTickNow, onPause } = controls;
  const stopped = model.liveness === 'stopped';
  const stale = model.liveness === 'stale';
  const ticking = model.status === 'ticking' && !stopped;
  const paused = model.status === 'paused';
  let actions: ReactNode;
  if (stopped) {
    actions = (
      <>
        {onRunOnce && (
          <Button variant="secondary" icon={<Zap />} onClick={onRunOnce} disabled={busy}>
            Run once
          </Button>
        )}
        <Button variant="primary" icon={<Play />} onClick={onStart} disabled={busy}>
          Start schedule
        </Button>
      </>
    );
  } else {
    actions = (
      <>
        <Button
          variant={!ticking && !paused && !stale ? 'primary' : 'secondary'}
          icon={<Zap />}
          onClick={onTickNow}
          disabled={busy || ticking}
        >
          Tick now
        </Button>
        <Button
          variant={paused ? 'primary' : 'secondary'}
          icon={paused ? <Play /> : <Pause />}
          onClick={() => onPause(!paused)}
          disabled={busy}
        >
          {paused ? 'Resume' : 'Pause'}
        </Button>
        <Button
          variant={stale ? 'destructive' : 'secondary'}
          icon={<Square />}
          onClick={onStop}
          disabled={busy}
        >
          Stop
        </Button>
      </>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="desk-actions">
      {actions}
    </div>
  );
}

function PathLine({ dir, onRemove, busy }: { dir: string; onRemove: () => void; busy: boolean }) {
  const [copied, setCopied] = useState(false);
  const parts = dir.split('/').filter(Boolean);
  const leaf = parts.pop() ?? dir;
  const parent = parts.length ? `${parts[parts.length - 1]}/` : '';
  const copy = async () => {
    await navigator.clipboard.writeText(dir);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="flex min-w-0 items-center gap-2">
      <button
        type="button"
        onClick={copy}
        title={dir}
        aria-label={`Copy the desk folder path ${dir}`}
        data-testid="desk-path"
        className="flex min-w-0 items-center gap-1.5 rounded-lz-control px-1.5 py-0.5 text-lz-meta text-lz-ink-3 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5 [&_svg]:shrink-0"
      >
        {copied ? <Check aria-hidden /> : <Copy aria-hidden />}
        <span className="min-w-0 truncate">
          {copied ? (
            'Path copied'
          ) : (
            <>
              {parent}
              <span className="text-lz-ink-2">{leaf}</span>
            </>
          )}
        </span>
      </button>
      <span className="ml-auto shrink-0">
        <Button variant="ghost" size="sm" icon={<Trash2 />} onClick={onRemove} disabled={busy}>
          Remove from roster
        </Button>
      </span>
    </div>
  );
}

function Metric({ label, value, tone }: { label: string; value: number | string; tone?: Tone }) {
  return (
    <div className="min-w-0">
      <dt className={TYPE.meta}>{label}</dt>
      <dd className={cx('text-lz-h2', TNUM, tone ? TONE_TEXT[tone] : 'text-lz-ink')}>{value}</dd>
    </div>
  );
}

function needsLine(asks: number, drafts: number): string {
  const parts = [
    asks > 0 ? `${asks} ${asks === 1 ? 'question' : 'questions'} to answer` : '',
    drafts > 0 ? `${drafts} ${drafts === 1 ? 'draft' : 'drafts'} to decide` : '',
  ].filter(Boolean);
  return `Waiting on you: ${parts.join(' and ')}`;
}

function cap(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

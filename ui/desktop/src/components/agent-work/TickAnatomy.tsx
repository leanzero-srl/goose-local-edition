import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Button, TNUM, TONE_TEXT, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { fmtDuration, type DeskModel, type PhaseSpan, type PhaseState } from './agentWorkModel';

const PHASE_FILL: Record<PhaseState, string> = {
  done: 'bg-lz-stopped-solid text-white',
  live: 'bg-lz-accent text-lz-accent-ink',
  failed: 'bg-lz-err-solid text-white',
  interrupted: 'bg-lz-warn-solid text-white',
  next: 'border border-lz-border-strong bg-lz-surface text-lz-ink-2',
  skipped: '',
};

const OUTCOME_TONE: Record<string, Tone> = {
  done: 'ok',
  held: 'warn',
  failed: 'err',
  running: 'accent',
  interrupted: 'warn',
};

/**
 * Where one tick's time went. Every phase the engine ENTERED is a solid block whose width is its
 * share of the tick (from the `tick_phase` events, so a 2-minute lane phase reads as the tick's
 * bulk and a phase with nothing to do is a narrow block); what it skipped is named once, quietly;
 * while it runs, the live phase is the accent block and what is ahead is outlined. Nothing is drawn
 * that the events do not say.
 */
export function TickAnatomy({
  model,
  onOpenTick,
}: {
  model: DeskModel;
  onOpenTick: (tick: number | null) => void;
}) {
  const t = model.viewTick;
  const rec = model.viewRecord;
  const isCurrent = t === model.tick;
  const liveNow = isCurrent && model.liveness === 'running' && model.status === 'ticking';
  const outcome = rec?.outcome ?? (liveNow ? 'running' : isCurrent ? 'interrupted' : undefined);
  const known = new Set<number>([...model.ticks.map((x) => x.tick), model.tick]);
  const numbers = [...known].filter((n) => n > 0).sort((a, b) => a - b);
  const idx = numbers.indexOf(t);
  const older = idx > 0 ? numbers[idx - 1] : null;
  const newer = idx >= 0 && idx < numbers.length - 1 ? numbers[idx + 1] : null;
  const phases = model.phases;
  const shown = phases?.filter((p) => p.state !== 'skipped') ?? [];
  const skipped = phases?.filter((p) => p.state === 'skipped') ?? [];
  const total = shown.reduce((n, p) => n + (p.ms ?? 0), 0);
  const facts = [
    rec?.wall_secs != null ? `${fmtDuration(rec.wall_secs * 1000)} wall` : '',
    rec?.lane_secs != null ? `${(rec.lane_secs / 60).toFixed(1)} lane-min` : '',
    rec?.lanes ? `${rec.lanes.length} ${rec.lanes.length === 1 ? 'lane' : 'lanes'}` : '',
  ].filter(Boolean);

  return (
    <section data-testid="tick-anatomy" aria-label={`Tick ${t}`} className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <h2 className={TYPE.h2}>Tick {t}</h2>
        {outcome && (
          <span
            data-testid="tick-outcome"
            className={cx(
              'text-lz-body',
              WEIGHT.semibold,
              TONE_TEXT[OUTCOME_TONE[outcome] ?? 'stopped']
            )}
          >
            {outcome === 'running' ? 'in progress' : outcome}
          </span>
        )}
        {facts.length > 0 && <span className={cx(TYPE.meta, TNUM)}>{facts.join(' · ')}</span>}
        {numbers.length > 1 && (
          <div className="ml-auto flex items-center gap-1" data-testid="tick-nav">
            <Button
              variant="ghost"
              size="sm"
              icon={<ChevronLeft />}
              disabled={older == null}
              onClick={() => older != null && onOpenTick(older)}
            >
              Older
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={newer == null}
              onClick={() => newer != null && onOpenTick(newer === model.tick ? null : newer)}
            >
              Newer
              <ChevronRight aria-hidden className="size-3.5" />
            </Button>
          </div>
        )}
      </div>
      {phases == null ? (
        <p className={TYPE.bodyMuted} data-testid="phase-ribbon-absent">
          {model.tick === 0
            ? 'No tick has run yet.'
            : 'The event log holds no phase timings for this tick.'}
        </p>
      ) : (
        <>
          <ol
            className="flex w-full min-w-0 flex-wrap gap-1"
            aria-label="tick phases"
            data-testid="phase-ribbon"
          >
            {shown.map((p) => (
              <PhaseBlock key={p.key} phase={p} total={total} />
            ))}
          </ol>
          {skipped.length > 0 && (
            <p className={TYPE.meta} data-testid="phase-skipped">
              Skipped: {skipped.map((p) => p.label).join(', ')}
            </p>
          )}
        </>
      )}
    </section>
  );
}

function PhaseBlock({ phase, total }: { phase: PhaseSpan; total: number }) {
  // A block's width is its share of the tick; with no timings (a live tick read from the phase
  // clock) every block is equal. A block never shrinks below its own words (nowrap + the flex
  // item's automatic minimum), so a zero-length phase stays readable and the row wraps instead.
  const grow = total > 0 && phase.ms != null ? Math.max(1, (phase.ms / total) * 100) : 1;
  return (
    <li
      data-phase={phase.key}
      data-state={phase.state}
      style={{ flexGrow: grow, flexBasis: 0 }}
      title={phase.note}
      className={cx(
        'flex flex-col justify-center gap-0.5 whitespace-nowrap rounded-lz-control px-2.5 py-2',
        PHASE_FILL[phase.state]
      )}
    >
      <span className={cx('flex min-w-[52px] items-center gap-1.5 text-[12px]', WEIGHT.semibold)}>
        {phase.state === 'live' && (
          <span
            aria-hidden
            className="inline-block size-1.5 shrink-0 animate-lz-live rounded-lz-pill bg-current"
          />
        )}
        <span>{phase.label}</span>
      </span>
      <span className={cx('text-lz-meta', TNUM)}>
        {phase.ms != null
          ? fmtDuration(phase.ms)
          : phase.state === 'next'
            ? 'ahead'
            : phase.state === 'done'
              ? 'passed'
              : ''}
        {phase.note ? ` · ${phase.note}` : ''}
      </span>
    </li>
  );
}

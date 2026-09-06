import { Clock, Pause, Play, Square, Zap } from 'lucide-react';
import { Button, Chip, StatusDot, TNUM, TONE_FILL, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { PHASES, countdown, fmtClock, fmtDuration, phaseIndex, type DeskModel } from './agentWorkModel';

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

/**
 * The desk's clock: WHEN the next tick happens (countdown + wall clock in the desk's zone + why),
 * WHERE the current tick is (the phase ribbon with the elapsed time on the live phase), and the
 * controls. The countdown is the value the owner asked to see first; it is the biggest thing here.
 */
export function TickClock({
  model,
  onStart,
  onStop,
  onTickNow,
  onPause,
  busy,
}: {
  model: DeskModel;
  onStart: () => void;
  onStop: () => void;
  onTickNow: () => void;
  onPause: (paused: boolean) => void;
  busy: boolean;
}) {
  const stopped = model.liveness === 'stopped';
  const stale = model.liveness === 'stale';
  const paused = model.status === 'paused';
  const tone: Tone = stale ? 'err' : (STATUS_TONE[model.status] ?? 'stopped');
  const label = stale ? 'stale — no heartbeat' : model.status;
  const current = phaseIndex(model.phase);
  const ticking = model.status === 'ticking' && !stopped;

  return (
    <div className="flex flex-col gap-4" data-testid="tick-clock">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex items-end gap-6">
          <div>
            <div className={TYPE.zone}>Next tick</div>
            <div className={cx('text-lz-display text-lz-ink leading-none', TNUM)} data-testid="next-tick-countdown">
              {stopped ? 'stopped' : ticking ? `tick ${model.tick} live` : countdown(model.nextTickInMs)}
            </div>
            <div className={cx(TYPE.meta, 'mt-1')}>
              {stopped
                ? 'start the desk to schedule one'
                : ticking
                  ? `started ${fmtClock(model.nextTickAt)}`
                  : `${model.nextTickLocal || fmtClock(model.nextTickAt)} — ${model.nextTickReason}`}
            </div>
          </div>
          <div className="flex flex-col gap-1 pb-1">
            <StatusDot tone={tone} live={ticking} label={label} />
            <Chip tone={model.windowOpen ? 'ok' : 'stopped'} icon={<Clock />}>
              {model.windowOpen ? 'desk open' : 'desk closed'}
            </Chip>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {stopped ? (
            <Button variant="primary" icon={<Play />} onClick={onStart} disabled={busy}>
              Start desk
            </Button>
          ) : (
            <>
              <Button variant="secondary" icon={<Zap />} onClick={onTickNow} disabled={busy || ticking}>
                Tick now
              </Button>
              <Button
                variant="secondary"
                icon={paused ? <Play /> : <Pause />}
                onClick={() => onPause(!paused)}
                disabled={busy}
              >
                {paused ? 'Resume' : 'Pause'}
              </Button>
              <Button variant="destructive" icon={<Square />} onClick={onStop} disabled={busy}>
                Stop
              </Button>
            </>
          )}
        </div>
      </div>
      {model.holdReason && (
        <div className={cx('rounded-lz-control px-3 py-2 text-lz-body', TONE_FILL.warn, WEIGHT.medium)}>
          held: {model.holdReason}
        </div>
      )}
      <ol className="flex flex-wrap gap-1" aria-label="tick phases" data-testid="phase-ribbon">
        {PHASES.map((p, i) => {
          const state = !ticking ? 'idle' : i < current ? 'done' : i === current ? 'live' : 'next';
          return (
            <li
              key={p.key}
              data-phase={p.key}
              data-state={state}
              className={cx(
                'flex h-7 items-center gap-2 rounded-lz-control px-2.5 text-[12px]',
                WEIGHT.medium,
                state === 'live' && 'bg-lz-accent text-lz-accent-ink',
                state === 'done' && TONE_FILL.ok,
                state === 'next' && 'bg-lz-surface-2 text-lz-ink-2',
                state === 'idle' && 'bg-lz-surface-2 text-lz-ink-3'
              )}
            >
              <span>{p.label}</span>
              {state === 'live' && model.phaseElapsedMs != null && (
                <span className={TNUM}>{fmtDuration(model.phaseElapsedMs)}</span>
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

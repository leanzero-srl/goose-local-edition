import type { ReactNode } from 'react';
import { Loader2, Play, Square, X } from 'lucide-react';
import { RADIUS, TNUM, TONE_FILL, WEIGHT, cx, type Tone } from '../lz';
import type { MlxEngineState } from '../../acp/mlx-engine';
import {
  compactTokens,
  formatElapsed,
  lastMeasuredTps,
  liveDecodeTps,
  mlxActivity,
  sparklinePoints,
  type MlxActivity,
  type MlxLiveRead,
  type MlxLiveRequest,
  type MlxLiveStats,
  type MountCost,
  type MountFill,
  type TpsSample,
} from './mlxLiveStats';

/**
 * The Engine hero's state tile — ONE solid fill that is the engine's state, and, now that it has
 * the room, a live INSTRUMENT for that state. Every figure is measured (mlxLiveStats.ts): the decode
 * rate and the in-flight requests from Rapid-MLX's own /v1/status while RUNNING, the memory the
 * engine has claimed while MOUNTING, the model's size against free memory while STOPPED, the
 * engine's own error while FAILED. White ink on the solid fill; tracks are hollow white outlines
 * with a solid white fill — no tint, no opacity. The spinner is the only motion and it only says
 * "in flight"; nothing here pretends to be progress.
 */

const STATE_TONE: Record<MlxEngineState, Tone> = {
  running: 'ok',
  mounting: 'accent',
  failed: 'err',
  stopped: 'stopped',
};

const ACTIVITY_WORD: Record<MlxActivity, string> = {
  generating: 'Generating',
  prefill: 'Reading prompt',
  queued: 'Queued',
  idle: 'Idle',
  not_loaded: 'Model not loaded',
};

const BIG = cx('text-[56px] leading-none tracking-tight', WEIGHT.semibold, TNUM);
const LABEL = 'text-lz-meta';
const LINE = cx('text-lz-body', TNUM);

export interface MlxStateTileProps {
  state: MlxEngineState | null;
  unreachable: boolean;
  /** RUNNING: the last /v1/status read (null before the first one lands). */
  live: MlxLiveRead | null;
  /** RUNNING: the decode rate per read, oldest first. */
  history: readonly TpsSample[];
  /** MOUNTING: the memory the engine has claimed against the model's size. */
  mount: MountFill | null;
  /** STOPPED: what mounting the picked model would cost. */
  cost: MountCost | null;
  /** FAILED: the engine's own error (null when the banner above already carries it). */
  failedError: string | null;
  /** The state's own action (Mount / Retry), drawn on the tile. */
  action: ReactNode;
}

/** A hollow white track with a solid white fill: legible on every state colour, no alpha. */
function TileBar({ fraction, label }: { fraction: number; label: string }) {
  const pct = Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      className={cx('h-2.5 w-full overflow-hidden border border-current', RADIUS.pill)}
    >
      <div className="h-full bg-current" style={{ width: `${pct}%` }} />
    </div>
  );
}

const SPARK_W = 240;
const SPARK_H = 48;

function Sparkline({ samples }: { samples: readonly TpsSample[] }) {
  const points = sparklinePoints(samples, SPARK_W, SPARK_H);
  if (!points) return null;
  const last = points.split(' ').pop()!.split(',');
  return (
    <svg
      data-testid="mlx-tps-sparkline"
      role="img"
      aria-label={`Decode rate over the last ${samples.length} reads`}
      viewBox={`-3 -3 ${SPARK_W + 6} ${SPARK_H + 6}`}
      preserveAspectRatio="none"
      className="h-14 w-full min-w-0 flex-1 overflow-visible"
    >
      <line
        x1={0}
        x2={SPARK_W}
        y1={SPARK_H}
        y2={SPARK_H}
        stroke="currentColor"
        strokeWidth={1}
        vectorEffect="non-scaling-stroke"
      />
      <polyline
        points={points}
        fill="none"
        stroke="currentColor"
        strokeWidth={2.5}
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
      <circle cx={last[0]} cy={last[1]} r={3.5} fill="currentColor" />
    </svg>
  );
}

function RequestRow({ request }: { request: MlxLiveRequest }) {
  const waiting = request.status === 'waiting' || request.phase === 'queued';
  const prompt = request.promptTokens != null ? compactTokens(request.promptTokens) : null;
  if (waiting || request.phase === 'prefill') {
    // The long silent pre-fill: no token is out yet, so there is no fraction to draw — the prompt
    // size and the engine's own elapsed seconds are the honest measure of it.
    return (
      <li data-testid="mlx-live-request" data-phase={request.phase} className="flex flex-col gap-1">
        <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
          <span className={WEIGHT.semibold}>
            {waiting ? 'Queued' : 'Reading prompt'}
            {prompt ? ` · ${prompt} tokens` : ''}
          </span>
          {request.elapsedS != null && <span>{formatElapsed(request.elapsedS)}</span>}
        </div>
      </li>
    );
  }
  const max = request.maxTokens;
  const fraction = max != null && max > 0 ? request.completionTokens / max : null;
  return (
    <li data-testid="mlx-live-request" data-phase={request.phase} className="flex flex-col gap-1.5">
      <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
        <span className="min-w-0 truncate">
          <span className={WEIGHT.semibold}>Writing</span>
          {` · ${request.completionTokens.toLocaleString()}`}
          {max != null ? ` of ${max.toLocaleString()} tokens` : ' tokens'}
        </span>
        {fraction != null && (
          <span className={WEIGHT.semibold}>{Math.round(Math.min(1, fraction) * 100)}%</span>
        )}
      </div>
      {fraction != null && <TileBar fraction={fraction} label="Tokens written of the limit" />}
    </li>
  );
}

function Fact({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex min-w-0 flex-col">
      <span className={cx('text-lz-h2', TNUM)}>{value}</span>
      <span className={LABEL}>{label}</span>
    </div>
  );
}

function RunningInstrument({
  live,
  history,
}: {
  live: MlxLiveRead | null;
  history: readonly TpsSample[];
}) {
  if (live == null) {
    return <p className={LINE}>Reading live stats…</p>;
  }
  if (!live.ok) {
    return (
      <div data-testid="mlx-live-unavailable" className="flex flex-col gap-1">
        <span className="text-lz-h2">Live stats unavailable</span>
        <span className={cx('break-words', LINE)}>{live.detail}</span>
      </div>
    );
  }
  return <LiveReadout stats={live.stats} history={history} />;
}

function LiveReadout({ stats, history }: { stats: MlxLiveStats; history: readonly TpsSample[] }) {
  const activity = mlxActivity(stats);
  const generating = activity === 'generating';
  const rate = generating ? liveDecodeTps(stats) : lastMeasuredTps(history);
  // Running requests first, then the queue — the engine's own order within each.
  const requests = [
    ...stats.requests.filter((r) => r.status !== 'waiting'),
    ...stats.requests.filter((r) => r.status === 'waiting'),
  ];
  const facts: Array<{ key: string; value: string; label: string }> = [];
  if (stats.activeMemoryGb != null) {
    facts.push({
      key: 'mem',
      value: `${stats.activeMemoryGb.toFixed(1)} GB`,
      label: 'GPU memory in use',
    });
  }
  if (stats.cacheHitRate != null) {
    facts.push({
      key: 'cache',
      value: `${Math.round(stats.cacheHitRate * 100)}%`,
      label: 'Prompt cache hits',
    });
  }
  if (stats.numWaiting != null) {
    facts.push({ key: 'wait', value: String(stats.numWaiting), label: 'Waiting' });
  }
  return (
    <div data-testid="mlx-live" data-activity={activity} className="flex flex-col gap-4">
      <div className="flex items-end gap-4">
        <div className="flex shrink-0 flex-col gap-1">
          <span data-testid="mlx-live-tps" className={BIG}>
            {rate != null && rate > 0 ? rate.toFixed(1) : generating ? '0.0' : '—'}
          </span>
          <span className={LABEL}>
            {generating
              ? 'tokens per second'
              : rate != null && rate > 0
                ? 'tokens per second, last run'
                : 'no generation measured yet'}
          </span>
        </div>
        <Sparkline samples={history} />
      </div>
      {requests.length > 0 && (
        <ul aria-label="Requests in flight" className="flex flex-col gap-3">
          {requests.map((r) => (
            <RequestRow key={r.id} request={r} />
          ))}
        </ul>
      )}
      {facts.length > 0 && (
        <div className="grid grid-cols-3 gap-3 border-t border-current pt-3">
          {facts.map((f) => (
            <Fact key={f.key} value={f.value} label={f.label} />
          ))}
        </div>
      )}
    </div>
  );
}

function MountingInstrument({ mount }: { mount: MountFill | null }) {
  if (mount == null) {
    return <p className={cx('text-lz-h2')}>Loading weights</p>;
  }
  if (mount.fraction == null) {
    // No baseline (the view opened mid-mount): the size is a fact, a percentage would be a guess.
    return (
      <div data-testid="mlx-mount-fill" data-measured="false" className="flex flex-col gap-1">
        <span className={BIG}>{mount.modelGb.toFixed(1)} GB</span>
        <span className={LINE}>Loading weights</span>
      </div>
    );
  }
  return (
    <div data-testid="mlx-mount-fill" data-measured="true" className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className={BIG}>{mount.claimedGb.toFixed(1)}</span>
        <span className={cx('text-lz-h2', TNUM)}>of {mount.modelGb.toFixed(1)} GB</span>
      </div>
      <TileBar fraction={mount.fraction} label="Memory claimed toward the model size" />
      <span className={LINE}>Memory claimed since the mount began</span>
    </div>
  );
}

function StoppedInstrument({ cost }: { cost: MountCost | null }) {
  if (cost == null) {
    return <p className={LINE}>Pick a model to see what mounting it costs.</p>;
  }
  const verdictLine =
    cost.verdict === 'no-fit'
      ? `Needs ${(-cost.spareGb).toFixed(1)} GB more free memory`
      : cost.verdict === 'tight'
        ? `Fits, ${cost.spareGb.toFixed(1)} GB spare above the ${cost.reserveGb.toFixed(1)} GB reserve`
        : `Fits, ${cost.spareGb.toFixed(1)} GB to spare`;
  return (
    <div data-testid="mlx-mount-cost" data-verdict={cost.verdict} className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className={BIG}>{cost.modelGb.toFixed(1)}</span>
        <span className={cx('text-lz-h2', TNUM)}>GB to mount</span>
      </div>
      <TileBar
        fraction={cost.freeGb > 0 ? cost.modelGb / cost.freeGb : 1}
        label="Model size against free memory"
      />
      <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
        <span className={WEIGHT.semibold}>{verdictLine}</span>
        <span className="shrink-0">{cost.freeGb.toFixed(1)} GB free</span>
      </div>
    </div>
  );
}

export function MlxStateTile(props: MlxStateTileProps) {
  const { state, unreachable, live, history, mount, cost, failedError, action } = props;
  const tone: Tone = state === null ? (unreachable ? 'err' : 'stopped') : STATE_TONE[state];
  const word = state ?? (unreachable ? 'unreachable' : 'checking');
  const icon =
    state === 'running' ? (
      <Play />
    ) : state === 'mounting' || (state === null && !unreachable) ? (
      <Loader2 className="animate-spin" />
    ) : state === 'failed' || unreachable ? (
      <X />
    ) : (
      <Square />
    );
  const activity = state === 'running' && live?.ok ? mlxActivity(live.stats) : null;
  return (
    <div
      data-testid="mlx-state-badge"
      data-state={word}
      role="group"
      aria-label={`Engine ${word}`}
      className={cx(
        'flex w-full shrink-0 flex-col gap-5 p-4 [&_svg]:shrink-0',
        state === 'running' ? 'lg:w-[30rem]' : 'lg:w-80',
        RADIUS.card,
        TONE_FILL[tone]
      )}
    >
      <div className="flex items-center justify-between gap-3">
        <span className="flex items-center gap-2 [&_svg]:size-5">
          <span aria-hidden>{icon}</span>
          <span role="status" className="text-lz-h2 capitalize">
            {word}
          </span>
        </span>
        {activity && (
          <span className={cx('text-lz-body', WEIGHT.semibold)}>{ACTIVITY_WORD[activity]}</span>
        )}
      </div>
      {state === 'running' && <RunningInstrument live={live} history={history} />}
      {state === 'mounting' && <MountingInstrument mount={mount} />}
      {state === 'stopped' && <StoppedInstrument cost={cost} />}
      {state === 'failed' && (
        <p
          data-testid="mlx-failed-excerpt"
          title={failedError ?? undefined}
          className={cx('line-clamp-5 break-words', LINE, WEIGHT.semibold)}
        >
          {failedError ?? 'The mount failed — the error is in the banner above.'}
        </p>
      )}
      {state === null && unreachable && (
        <p className={LINE}>The engine status could not be read.</p>
      )}
      {action && <div className="mt-auto flex flex-wrap gap-2">{action}</div>}
    </div>
  );
}

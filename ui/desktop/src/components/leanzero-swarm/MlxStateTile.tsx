import type { ReactNode } from 'react';
import {
  Bot,
  CircleHelp,
  Loader2,
  MessageSquare,
  Network,
  Play,
  PlugZap,
  Square,
  X,
} from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { RADIUS, TNUM, TONE_FILL, WEIGHT, cx, type Tone } from '../lz';
import type { MlxEngineState } from '../../acp/mlx-engine';
import type { MlxClient, MlxServing } from '../../utils/mlxServing';
import {
  formatElapsed,
  formatRate,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  sparklinePoints,
  type LastRates,
  type MlxActivity,
  type MlxLiveRead,
  type MlxLiveRequest,
  type MlxLiveStats,
  type MountCost,
  type MountFill,
  type TpsSample,
} from './mlxLiveStats';

/**
 * The Engine hero's state tile — ONE solid fill that is the engine's state, and a live INSTRUMENT
 * for it. While RUNNING the fill follows what the engine is DOING, from its own request phases:
 * solid slate while idle (the last measured rates stay as plain facts), the accent while it reads a
 * prompt, the ok green while it writes. Every figure is measured (mlxLiveStats.ts): the writing
 * rate from the generating requests' own rates, the reading rate from a request's computed prompt
 * tokens over its time to first token, the lifetime counters from the engine's own totals, and WHO
 * it serves from goose's in-flight list read by main (utils/mlxServing.ts) — work neither goose
 * door explains is counted, never named. White ink on every fill; tracks are hollow white outlines
 * with a solid white fill — no tint, no opacity. The spinner is the only motion.
 */

const i18n = defineMessages({
  running: { id: 'mlxStateTile.state.running', defaultMessage: 'Running' },
  mounting: { id: 'mlxStateTile.state.mounting', defaultMessage: 'Mounting' },
  failed: { id: 'mlxStateTile.state.failed', defaultMessage: 'Failed' },
  stopped: { id: 'mlxStateTile.state.stopped', defaultMessage: 'Stopped' },
  unreachable: { id: 'mlxStateTile.state.unreachable', defaultMessage: 'Unreachable' },
  checking: { id: 'mlxStateTile.state.checking', defaultMessage: 'Checking' },
  groupLabel: { id: 'mlxStateTile.groupLabel', defaultMessage: 'Engine {state}' },
  generating: { id: 'mlxStateTile.activity.generating', defaultMessage: 'Writing' },
  prefill: { id: 'mlxStateTile.activity.prefill', defaultMessage: 'Reading prompt' },
  queued: { id: 'mlxStateTile.activity.queued', defaultMessage: 'Queued' },
  idle: { id: 'mlxStateTile.activity.idle', defaultMessage: 'Idle' },
  notLoaded: { id: 'mlxStateTile.activity.notLoaded', defaultMessage: 'Model not loaded' },
  readingLive: { id: 'mlxStateTile.readingLive', defaultMessage: 'Reading live stats…' },
  liveUnavailable: { id: 'mlxStateTile.liveUnavailable', defaultMessage: 'Live stats unavailable' },
  writeRate: { id: 'mlxStateTile.writeRate', defaultMessage: 'tok/s writing' },
  writeRateLast: { id: 'mlxStateTile.writeRateLast', defaultMessage: 'tok/s writing, last run' },
  writeRateNone: { id: 'mlxStateTile.writeRateNone', defaultMessage: 'nothing written yet' },
  readRate: { id: 'mlxStateTile.readRate', defaultMessage: 'tok/s reading this prompt' },
  readRateLast: { id: 'mlxStateTile.readRateLast', defaultMessage: 'tok/s reading, last prompt' },
  readRateNone: { id: 'mlxStateTile.readRateNone', defaultMessage: 'no prompt read yet' },
  promptSize: {
    id: 'mlxStateTile.promptSize',
    defaultMessage: 'prompt tokens, reading for {elapsed}',
  },
  waiting: {
    id: 'mlxStateTile.waiting',
    defaultMessage: '{count, plural, one {request waiting} other {requests waiting}}',
  },
  sparkAria: {
    id: 'mlxStateTile.sparkAria',
    defaultMessage: 'Writing rate over the last {count} reads',
  },
  requestsAria: { id: 'mlxStateTile.requestsAria', defaultMessage: 'Requests in flight' },
  rowQueued: { id: 'mlxStateTile.row.queued', defaultMessage: 'Queued' },
  rowReading: { id: 'mlxStateTile.row.reading', defaultMessage: 'Reading prompt' },
  rowTokens: { id: 'mlxStateTile.row.tokens', defaultMessage: '{count} tokens' },
  rowCached: { id: 'mlxStateTile.row.cached', defaultMessage: '{count} from cache' },
  rowWriting: { id: 'mlxStateTile.row.writing', defaultMessage: 'Writing' },
  rowWrittenOf: {
    id: 'mlxStateTile.row.writtenOf',
    defaultMessage: '{written} of {max} tokens',
  },
  rowWritten: { id: 'mlxStateTile.row.written', defaultMessage: '{written} tokens' },
  rowBar: { id: 'mlxStateTile.row.bar', defaultMessage: 'Tokens written of the limit' },
  serving: { id: 'mlxStateTile.serving', defaultMessage: 'Serving' },
  clientChat: { id: 'mlxStateTile.client.chat', defaultMessage: 'Chat · {name}' },
  clientExternal: {
    id: 'mlxStateTile.client.external',
    defaultMessage: 'External client via /v1 · {model}',
  },
  clientSession: {
    id: 'mlxStateTile.client.session',
    defaultMessage: 'goose session ({type}) · {name}',
  },
  clientTimes: { id: 'mlxStateTile.client.times', defaultMessage: '{count} requests' },
  unattributed: {
    id: 'mlxStateTile.unattributed',
    defaultMessage:
      "{count, plural, one {# request} other {# requests}} not from this app's chats or /v1",
  },
  swarmLive: { id: 'mlxStateTile.swarmLive', defaultMessage: 'Swarm run live: {runs}' },
  servingUnknown: {
    id: 'mlxStateTile.servingUnknown',
    defaultMessage: 'Who is using it could not be read: {detail}',
  },
  served: { id: 'mlxStateTile.fact.served', defaultMessage: 'requests served' },
  promptRead: { id: 'mlxStateTile.fact.promptRead', defaultMessage: 'prompt tokens read' },
  written: { id: 'mlxStateTile.fact.written', defaultMessage: 'tokens written' },
  cacheSaved: {
    id: 'mlxStateTile.fact.cacheSaved',
    defaultMessage: 'prompt tokens from cache',
  },
  cacheHits: { id: 'mlxStateTile.fact.cacheHits', defaultMessage: 'of cache lookups hit' },
  gpu: { id: 'mlxStateTile.fact.gpu', defaultMessage: 'GPU memory in use' },
  uptime: { id: 'mlxStateTile.fact.uptime', defaultMessage: 'engine uptime' },
  loadingWeights: { id: 'mlxStateTile.loadingWeights', defaultMessage: 'Loading weights' },
  ofGb: { id: 'mlxStateTile.ofGb', defaultMessage: 'of {gb} GB' },
  claimed: {
    id: 'mlxStateTile.claimed',
    defaultMessage: 'Memory claimed since the mount began',
  },
  claimedBar: {
    id: 'mlxStateTile.claimedBar',
    defaultMessage: 'Memory claimed toward the model size',
  },
  pickModel: {
    id: 'mlxStateTile.pickModel',
    defaultMessage: 'Pick a model to see what mounting it costs.',
  },
  gbToMount: { id: 'mlxStateTile.gbToMount', defaultMessage: 'GB to mount' },
  needsMore: { id: 'mlxStateTile.needsMore', defaultMessage: 'Needs {gb} GB more free memory' },
  fitsTight: {
    id: 'mlxStateTile.fitsTight',
    defaultMessage: 'Fits, {spare} GB spare above the {reserve} GB reserve',
  },
  fits: { id: 'mlxStateTile.fits', defaultMessage: 'Fits, {spare} GB to spare' },
  free: { id: 'mlxStateTile.free', defaultMessage: '{gb} GB free' },
  costBar: { id: 'mlxStateTile.costBar', defaultMessage: 'Model size against free memory' },
  failedFallback: {
    id: 'mlxStateTile.failedFallback',
    defaultMessage: 'The mount failed — the error is in the banner above.',
  },
  statusUnread: {
    id: 'mlxStateTile.statusUnread',
    defaultMessage: 'The engine status could not be read.',
  },
});

const STATE_TONE: Record<MlxEngineState, Tone> = {
  running: 'ok',
  mounting: 'accent',
  failed: 'err',
  stopped: 'stopped',
};

/** While running, the fill is what the engine is DOING: slate at rest, blue reading, green writing. */
const ACTIVITY_TONE: Record<MlxActivity, Tone> = {
  generating: 'ok',
  prefill: 'accent',
  queued: 'accent',
  idle: 'stopped',
  not_loaded: 'stopped',
};

const ACTIVITY_WORD = {
  generating: i18n.generating,
  prefill: i18n.prefill,
  queued: i18n.queued,
  idle: i18n.idle,
  not_loaded: i18n.notLoaded,
} as const;

const STATE_WORD = {
  running: i18n.running,
  mounting: i18n.mounting,
  failed: i18n.failed,
  stopped: i18n.stopped,
  unreachable: i18n.unreachable,
  checking: i18n.checking,
} as const;

const HERO = cx('text-[56px] leading-none tracking-tight', WEIGHT.semibold, TNUM);
const HERO_QUIET = cx('text-[40px] leading-none tracking-tight', WEIGHT.semibold, TNUM);
const SECOND = cx('text-[28px] leading-none tracking-tight', WEIGHT.semibold, TNUM);
const LABEL = 'text-lz-meta';
const LINE = cx('text-lz-body', TNUM);

export interface MlxStateTileProps {
  state: MlxEngineState | null;
  unreachable: boolean;
  /** RUNNING: the last /v1/status read (null before the first one lands). */
  live: MlxLiveRead | null;
  /** RUNNING: the writing rate per read, oldest first. */
  history: readonly TpsSample[];
  /** RUNNING: the last rates this view measured, for the idle tile. */
  last: LastRates;
  /** RUNNING: who the engine is serving, from main's read of goose's in-flight list. */
  serving: MlxServing | null;
  /** MOUNTING: the memory the engine has claimed against the model's size. */
  mount: MountFill | null;
  /** STOPPED: what mounting the picked model would cost. */
  cost: MountCost | null;
  /** FAILED: the engine's own error (null when the banner above already carries it). */
  failedError: string | null;
  /** The state's own action (Mount / Retry), drawn on the tile. */
  action: ReactNode;
}

function compact(intl: IntlShape, n: number): string {
  return intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });
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
const SPARK_H = 40;

function Sparkline({ samples }: { samples: readonly TpsSample[] }) {
  const intl = useIntl();
  const points = sparklinePoints(samples, SPARK_W, SPARK_H);
  if (!points) return null;
  const last = points.split(' ').pop()!.split(',');
  return (
    <svg
      data-testid="mlx-tps-sparkline"
      role="img"
      aria-label={intl.formatMessage(i18n.sparkAria, { count: samples.length })}
      viewBox={`-3 -3 ${SPARK_W + 6} ${SPARK_H + 6}`}
      preserveAspectRatio="none"
      className="h-12 w-full min-w-0 overflow-visible"
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
  const intl = useIntl();
  const waiting = request.status === 'waiting' || request.phase === 'queued';
  if (waiting || request.phase === 'prefill') {
    // The long silent pre-fill: no token is out yet and the engine reports no per-request progress,
    // so there is no fraction to draw — the prompt size, what the cache supplied and the engine's
    // own elapsed seconds are the honest measure of it.
    const parts = [
      intl.formatMessage(waiting ? i18n.rowQueued : i18n.rowReading),
      request.promptTokens != null
        ? intl.formatMessage(i18n.rowTokens, { count: compact(intl, request.promptTokens) })
        : null,
      request.cachedTokens
        ? intl.formatMessage(i18n.rowCached, { count: compact(intl, request.cachedTokens) })
        : null,
    ].filter(Boolean);
    return (
      <li data-testid="mlx-live-request" data-phase={request.phase} className="flex flex-col gap-1">
        <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
          <span className={cx('min-w-0 truncate', WEIGHT.semibold)}>{parts.join(' · ')}</span>
          {request.elapsedS != null && (
            <span className="shrink-0">{formatElapsed(request.elapsedS)}</span>
          )}
        </div>
      </li>
    );
  }
  const max = request.maxTokens;
  const fraction = max != null && max > 0 ? request.completionTokens / max : null;
  const written = intl.formatNumber(request.completionTokens);
  return (
    <li data-testid="mlx-live-request" data-phase={request.phase} className="flex flex-col gap-1.5">
      <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
        <span className="min-w-0 truncate">
          <span className={WEIGHT.semibold}>{intl.formatMessage(i18n.rowWriting)}</span>
          {' · '}
          {max != null
            ? intl.formatMessage(i18n.rowWrittenOf, { written, max: intl.formatNumber(max) })
            : intl.formatMessage(i18n.rowWritten, { written })}
        </span>
        {fraction != null && (
          <span className={cx('shrink-0', WEIGHT.semibold)}>
            {Math.round(Math.min(1, fraction) * 100)}%
          </span>
        )}
      </div>
      {fraction != null && <TileBar fraction={fraction} label={intl.formatMessage(i18n.rowBar)} />}
    </li>
  );
}

function clientText(intl: IntlShape, client: MlxClient): string {
  switch (client.kind) {
    case 'chat':
      return intl.formatMessage(i18n.clientChat, {
        name: client.sessionName || client.sessionId,
      });
    case 'external':
      return intl.formatMessage(i18n.clientExternal, { model: client.model });
    case 'session':
      return intl.formatMessage(i18n.clientSession, {
        type: client.sessionType ? client.sessionType.replace(/_/g, ' ') : '—',
        name: client.sessionName || client.sessionId || '—',
      });
  }
}

const CLIENT_ICON = { chat: MessageSquare, external: PlugZap, session: Bot } as const;

/** WHO the engine is serving — only what goose listed; the rest is a count beside the live runs. */
function ServingList({ serving }: { serving: MlxServing }) {
  const intl = useIntl();
  const rows: Array<{ key: string; icon: ReactNode; text: string; extra?: string }> =
    serving.clients.map((c) => {
      const Icon = CLIENT_ICON[c.kind];
      return {
        key: c.key,
        icon: <Icon />,
        text: clientText(intl, c),
        extra: c.count > 1 ? intl.formatMessage(i18n.clientTimes, { count: c.count }) : undefined,
      };
    });
  if (serving.unattributed > 0) {
    rows.push({
      key: 'unattributed',
      icon: <CircleHelp />,
      text: intl.formatMessage(i18n.unattributed, { count: serving.unattributed }),
    });
    if (serving.swarmRuns.length > 0) {
      rows.push({
        key: 'swarm',
        icon: <Network />,
        text: intl.formatMessage(i18n.swarmLive, { runs: serving.swarmRuns.join(', ') }),
      });
    }
  }
  if (serving.error) {
    rows.push({
      key: 'error',
      icon: <CircleHelp />,
      text: intl.formatMessage(i18n.servingUnknown, { detail: serving.error }),
    });
  }
  if (rows.length === 0) return null;
  return (
    <div data-testid="mlx-serving" className="flex flex-col gap-1.5">
      <span className={LABEL}>{intl.formatMessage(i18n.serving)}</span>
      <ul className="flex flex-col gap-1">
        {rows.map((r) => (
          <li
            key={r.key}
            data-testid="mlx-serving-row"
            className={cx('flex min-w-0 items-center gap-2 [&_svg]:size-4', LINE)}
          >
            <span aria-hidden>{r.icon}</span>
            <span className={cx('min-w-0 truncate', WEIGHT.semibold)} title={r.text}>
              {r.text}
            </span>
            {r.extra && <span className="shrink-0">{r.extra}</span>}
          </li>
        ))}
      </ul>
    </div>
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

function RunningInstrument(props: {
  live: MlxLiveRead | null;
  history: readonly TpsSample[];
  last: LastRates;
  serving: MlxServing | null;
}) {
  const intl = useIntl();
  const { live } = props;
  if (live == null) {
    return <p className={LINE}>{intl.formatMessage(i18n.readingLive)}</p>;
  }
  if (!live.ok) {
    return (
      <div data-testid="mlx-live-unavailable" className="flex flex-col gap-1">
        <span className="text-lz-h2">{intl.formatMessage(i18n.liveUnavailable)}</span>
        <span className={cx('break-words', LINE)}>{live.detail}</span>
      </div>
    );
  }
  return (
    <LiveReadout
      stats={live.stats}
      history={props.history}
      last={props.last}
      serving={props.serving}
    />
  );
}

interface Figure {
  testId: string;
  value: string;
  label: string;
}

/** The two figures the tile leads with, per activity — each one measured, or a dash that says so. */
function figures(
  intl: IntlShape,
  stats: MlxLiveStats,
  last: LastRates
): { hero: Figure; second: Figure } {
  const activity = mlxActivity(stats);
  const rate = (tps: number) => formatRate(tps, intl.locale);
  const prefillNow = measuredPrefillTps(stats);
  const readFigure: Figure =
    prefillNow > 0
      ? {
          testId: 'mlx-live-pps',
          value: rate(prefillNow),
          label: intl.formatMessage(i18n.readRate),
        }
      : last.prefillTps != null
        ? {
            testId: 'mlx-live-pps',
            value: rate(last.prefillTps),
            label: intl.formatMessage(i18n.readRateLast),
          }
        : { testId: 'mlx-live-pps', value: '—', label: intl.formatMessage(i18n.readRateNone) };
  const lastWrite: Figure =
    last.decodeTps != null
      ? {
          testId: 'mlx-live-tps',
          value: rate(last.decodeTps),
          label: intl.formatMessage(i18n.writeRateLast),
        }
      : { testId: 'mlx-live-tps', value: '—', label: intl.formatMessage(i18n.writeRateNone) };

  if (activity === 'generating') {
    return {
      hero: {
        testId: 'mlx-live-tps',
        value: rate(liveDecodeTps(stats)),
        label: intl.formatMessage(i18n.writeRate),
      },
      second: readFigure,
    };
  }
  if (activity === 'prefill') {
    const reading = stats.requests
      .filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
      .sort((a, b) => (b.elapsedS ?? 0) - (a.elapsedS ?? 0))[0];
    return {
      hero: {
        testId: 'mlx-live-prompt',
        value: reading?.promptTokens != null ? compact(intl, reading.promptTokens) : '—',
        label: intl.formatMessage(i18n.promptSize, {
          elapsed: formatElapsed(reading?.elapsedS ?? 0),
        }),
      },
      second: readFigure,
    };
  }
  if (activity === 'queued') {
    return {
      hero: {
        testId: 'mlx-live-queued',
        value: intl.formatNumber(stats.requests.length),
        label: intl.formatMessage(i18n.waiting, { count: stats.requests.length }),
      },
      second: readFigure,
    };
  }
  return { hero: lastWrite, second: readFigure };
}

function LiveReadout({
  stats,
  history,
  last,
  serving,
}: {
  stats: MlxLiveStats;
  history: readonly TpsSample[];
  last: LastRates;
  serving: MlxServing | null;
}) {
  const intl = useIntl();
  const activity = mlxActivity(stats);
  const active = activity === 'generating' || activity === 'prefill' || activity === 'queued';
  const { hero, second } = figures(intl, stats, last);
  // Running requests first, then the queue — the engine's own order within each.
  const requests = [
    ...stats.requests.filter((r) => r.status !== 'waiting'),
    ...stats.requests.filter((r) => r.status === 'waiting'),
  ];
  const facts: Array<{ key: string; value: string; label: string }> = [];
  if (stats.totalRequests != null) {
    facts.push({
      key: 'served',
      value: intl.formatNumber(stats.totalRequests),
      label: intl.formatMessage(i18n.served),
    });
  }
  if (stats.totalPromptTokens != null) {
    facts.push({
      key: 'prompt',
      value: compact(intl, stats.totalPromptTokens),
      label: intl.formatMessage(i18n.promptRead),
    });
  }
  if (stats.totalCompletionTokens != null) {
    facts.push({
      key: 'written',
      value: compact(intl, stats.totalCompletionTokens),
      label: intl.formatMessage(i18n.written),
    });
  }
  if (stats.cacheTokensSaved != null) {
    facts.push({
      key: 'saved',
      value: compact(intl, stats.cacheTokensSaved),
      label: intl.formatMessage(i18n.cacheSaved),
    });
  }
  if (stats.cacheHitRate != null) {
    facts.push({
      key: 'hits',
      value: intl.formatNumber(stats.cacheHitRate, { style: 'percent' }),
      label: intl.formatMessage(i18n.cacheHits),
    });
  }
  if (stats.activeMemoryGb != null) {
    facts.push({
      key: 'mem',
      value: `${intl.formatNumber(stats.activeMemoryGb, { maximumFractionDigits: 1, minimumFractionDigits: 1 })} GB`,
      label: intl.formatMessage(i18n.gpu),
    });
  }
  if (stats.uptimeS != null) {
    facts.push({
      key: 'uptime',
      value: formatElapsed(stats.uptimeS),
      label: intl.formatMessage(i18n.uptime),
    });
  }
  return (
    <div data-testid="mlx-live" data-activity={activity} className="flex flex-col gap-4">
      <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)] items-end gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <span data-testid={hero.testId} className={active ? HERO : HERO_QUIET}>
            {hero.value}
          </span>
          <span className={LABEL}>{hero.label}</span>
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <span data-testid={second.testId} className={SECOND}>
            {second.value}
          </span>
          <span className={LABEL}>{second.label}</span>
        </div>
      </div>
      <Sparkline samples={history} />
      {serving && <ServingList serving={serving} />}
      {requests.length > 0 && (
        <ul aria-label={intl.formatMessage(i18n.requestsAria)} className="flex flex-col gap-3">
          {requests.map((r) => (
            <RequestRow key={r.id} request={r} />
          ))}
        </ul>
      )}
      {facts.length > 0 && (
        <div
          data-testid="mlx-live-facts"
          className="grid grid-cols-3 gap-x-3 gap-y-2.5 border-t border-current pt-3"
        >
          {facts.map((f) => (
            <Fact key={f.key} value={f.value} label={f.label} />
          ))}
        </div>
      )}
    </div>
  );
}

function MountingInstrument({ mount }: { mount: MountFill | null }) {
  const intl = useIntl();
  const gb = (n: number) =>
    intl.formatNumber(n, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (mount == null) {
    return <p className="text-lz-h2">{intl.formatMessage(i18n.loadingWeights)}</p>;
  }
  if (mount.fraction == null) {
    // No baseline (the view opened mid-mount): the size is a fact, a percentage would be a guess.
    return (
      <div data-testid="mlx-mount-fill" data-measured="false" className="flex flex-col gap-1">
        <span className={HERO}>{gb(mount.modelGb)} GB</span>
        <span className={LINE}>{intl.formatMessage(i18n.loadingWeights)}</span>
      </div>
    );
  }
  return (
    <div data-testid="mlx-mount-fill" data-measured="true" className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className={HERO}>{gb(mount.claimedGb)}</span>
        <span className={cx('text-lz-h2', TNUM)}>
          {intl.formatMessage(i18n.ofGb, { gb: gb(mount.modelGb) })}
        </span>
      </div>
      <TileBar fraction={mount.fraction} label={intl.formatMessage(i18n.claimedBar)} />
      <span className={LINE}>{intl.formatMessage(i18n.claimed)}</span>
    </div>
  );
}

function StoppedInstrument({ cost }: { cost: MountCost | null }) {
  const intl = useIntl();
  const gb = (n: number) =>
    intl.formatNumber(n, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (cost == null) {
    return <p className={LINE}>{intl.formatMessage(i18n.pickModel)}</p>;
  }
  const verdictLine =
    cost.verdict === 'no-fit'
      ? intl.formatMessage(i18n.needsMore, { gb: gb(-cost.spareGb) })
      : cost.verdict === 'tight'
        ? intl.formatMessage(i18n.fitsTight, {
            spare: gb(cost.spareGb),
            reserve: gb(cost.reserveGb),
          })
        : intl.formatMessage(i18n.fits, { spare: gb(cost.spareGb) });
  return (
    <div data-testid="mlx-mount-cost" data-verdict={cost.verdict} className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className={HERO}>{gb(cost.modelGb)}</span>
        <span className={cx('text-lz-h2', TNUM)}>{intl.formatMessage(i18n.gbToMount)}</span>
      </div>
      <TileBar
        fraction={cost.freeGb > 0 ? cost.modelGb / cost.freeGb : 1}
        label={intl.formatMessage(i18n.costBar)}
      />
      <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
        <span className={WEIGHT.semibold}>{verdictLine}</span>
        <span className="shrink-0">{intl.formatMessage(i18n.free, { gb: gb(cost.freeGb) })}</span>
      </div>
    </div>
  );
}

export function MlxStateTile(props: MlxStateTileProps) {
  const intl = useIntl();
  const { state, unreachable, live, history, last, serving, mount, cost, failedError, action } =
    props;
  const activity = state === 'running' && live?.ok ? mlxActivity(live.stats) : null;
  const tone: Tone =
    state === null
      ? unreachable
        ? 'err'
        : 'stopped'
      : state === 'running'
        ? activity
          ? ACTIVITY_TONE[activity]
          : 'stopped'
        : STATE_TONE[state];
  const word = state ?? (unreachable ? 'unreachable' : 'checking');
  const wordText = intl.formatMessage(STATE_WORD[word]);
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
  return (
    <div
      data-testid="mlx-state-badge"
      data-state={word}
      data-activity={activity ?? undefined}
      role="group"
      aria-label={intl.formatMessage(i18n.groupLabel, { state: wordText })}
      className={cx(
        'flex w-full shrink-0 flex-col gap-5 p-4 [&_svg]:shrink-0',
        state === 'running' ? 'lg:w-[32rem]' : 'lg:w-80',
        RADIUS.card,
        TONE_FILL[tone]
      )}
    >
      <div className="flex items-center justify-between gap-3">
        <span className="flex items-center gap-2 [&_svg]:size-5">
          <span aria-hidden>{icon}</span>
          <span role="status" className="text-lz-h2">
            {wordText}
          </span>
        </span>
        {activity && (
          <span data-testid="mlx-activity" className={cx('text-lz-body', WEIGHT.semibold)}>
            {intl.formatMessage(ACTIVITY_WORD[activity])}
          </span>
        )}
      </div>
      {state === 'running' && (
        <RunningInstrument live={live} history={history} last={last} serving={serving} />
      )}
      {state === 'mounting' && <MountingInstrument mount={mount} />}
      {state === 'stopped' && <StoppedInstrument cost={cost} />}
      {state === 'failed' && (
        <p
          data-testid="mlx-failed-excerpt"
          title={failedError ?? undefined}
          className={cx('line-clamp-5 break-words', LINE, WEIGHT.semibold)}
        >
          {failedError ?? intl.formatMessage(i18n.failedFallback)}
        </p>
      )}
      {state === null && unreachable && (
        <p className={LINE}>{intl.formatMessage(i18n.statusUnread)}</p>
      )}
      {action && <div className="mt-auto flex flex-wrap gap-2">{action}</div>}
    </div>
  );
}

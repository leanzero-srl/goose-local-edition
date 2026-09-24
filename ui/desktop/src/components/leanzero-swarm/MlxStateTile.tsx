import { useSyncExternalStore, type ReactNode } from 'react';
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
import { PHASE_FILL, RADIUS, TNUM, WEIGHT, cx, type EnginePhase } from '../lz';
import type { MlxEngineState } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  remoteRouteUp,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import {
  gb1,
  gib,
  layerSpan,
  layerSpanShort,
  nodeLoadProgress,
  nodeStartWord,
  ownsTheMac,
  planForRank,
  runStateInFlight,
  type LoadProgress,
} from './mlxDistributed';
import { hostingPhase, nodePhase, runPhase, singlePhase } from './mlxPhase';
import { distributedStateWord } from './mlxModeLabel';
import type { MlxClient, MlxServing } from '../../utils/mlxServing';
import {
  formatElapsed,
  formatRate,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  sparklinePoints,
  type LastRates,
  type MlxLiveRead,
  type MlxLiveRequest,
  type MlxLiveStats,
  type MountCost,
  type MountFill,
  type SingleLoad,
  type TpsSample,
} from './mlxLiveStats';

/**
 * The Engine hero's state tile — ONE solid fill that is the engine's state, and a live INSTRUMENT
 * for it, in the ENGINE-PHASE palette every MLX surface shares (lz/tokens.ts PHASE_FILL, mapped by
 * mlxPhase.ts): dark outlined with no model, grey idle (the last measured rates stay as plain
 * facts), amber while weights load, blue while it reads a prompt, green while it writes, orange
 * when requests are queued or admission is held, red when it failed. Every figure is measured
 * (mlxLiveStats.ts): the writing rate from the generating requests' own rates, the reading rate
 * from a request's computed prompt tokens over its time to first token, the lifetime counters from
 * the engine's own totals, and WHO it serves from goose's in-flight list read by main
 * (utils/mlxServing.ts) — work neither goose door explains is counted, never named. The ink is the phase's own; tracks are hollow outlines
 * in that ink with a solid fill of it — no tint, no opacity. The only motion is the spinner and
 * the indeterminate track of a load goose reported no figure for.
 */

const i18n = defineMessages({
  running: { id: 'mlxStateTile.state.running', defaultMessage: 'Running' },
  servingFrom: {
    id: 'mlxStateTile.servingFrom',
    defaultMessage: 'Chat: serving from {peer} · {state}',
  },
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
  makingRoom: { id: 'mlxStateTile.makingRoom', defaultMessage: 'Making room' },
  startingEngine: { id: 'mlxStateTile.startingEngine', defaultMessage: 'Starting the engine' },
  warming: { id: 'mlxStateTile.warming', defaultMessage: 'Warming up' },
  weightsGb: { id: 'mlxStateTile.weightsGb', defaultMessage: 'GB of weights' },
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
  available: { id: 'mlxStateTile.available', defaultMessage: '{gb} GB available' },
  costBar: { id: 'mlxStateTile.costBar', defaultMessage: 'Model size against free memory' },
  failedFallback: {
    id: 'mlxStateTile.failedFallback',
    defaultMessage: 'The mount failed — the error is in the banner above.',
  },
  statusUnread: {
    id: 'mlxStateTile.statusUnread',
    defaultMessage: 'The engine status could not be read.',
  },
  distInflight: {
    id: 'mlxStateTile.dist.inflight',
    defaultMessage: '{count, plural, one {request in flight} other {requests in flight}}',
  },
  distInflightUnknown: {
    id: 'mlxStateTile.dist.inflightUnknown',
    defaultMessage: 'in flight: not measured',
  },
  distSlots: { id: 'mlxStateTile.dist.slots', defaultMessage: 'slots {used} of {slots}' },
  distWaiting: { id: 'mlxStateTile.dist.waiting', defaultMessage: '{count} waiting' },
  distPeak: { id: 'mlxStateTile.dist.peak', defaultMessage: '{peak} of {budget} GiB peak' },
  distPeakNoBudget: { id: 'mlxStateTile.dist.peakNoBudget', defaultMessage: '{peak} GiB peak' },
  distNoPeak: { id: 'mlxStateTile.dist.noPeak', defaultMessage: 'no peak yet' },
  distPeakBar: {
    id: 'mlxStateTile.dist.peakBar',
    defaultMessage: 'Peak memory against the budget',
  },
  distHeld: {
    id: 'mlxStateTile.dist.held',
    defaultMessage: 'Admission closed: a node is low on memory',
  },
  hostingRank: {
    id: 'mlxStateTile.hosting.rank',
    defaultMessage: 'Rank {rank} of {size}',
  },
  hostingFor: {
    id: 'mlxStateTile.hosting.for',
    defaultMessage: "for {requester}'s distributed engine over LeanZero Link",
  },
  hostingPid: { id: 'mlxStateTile.hosting.pid', defaultMessage: 'rank pid {pid}' },
  hostingLoading: {
    id: 'mlxStateTile.hosting.loading',
    defaultMessage: 'Loading rank {rank} for {requester}',
  },
  loadBytes: { id: 'mlxStateTile.load.bytes', defaultMessage: '{done} of {total} GB' },
  loadBar: { id: 'mlxStateTile.load.bar', defaultMessage: 'Weights loaded on {node}' },
  thisMacNode: { id: 'mlxStateTile.thisMacNode', defaultMessage: 'this Mac' },
  distStarting: {
    id: 'mlxStateTile.dist.starting',
    defaultMessage: 'Each Mac, as it loads its part',
  },
  hostingSingleRefused: {
    id: 'mlxStateTile.hosting.singleRefused',
    defaultMessage: 'The single engine here is refused while this Mac serves the rank.',
  },
});

/** A rank this Mac serves for ANOTHER Mac's distributed engine: which one, for whom, its pid. */
function HostingInstrument({ hosting }: { hosting: NonNullable<MlxDistributedStatus['hosting']> }) {
  const intl = useIntl();
  return (
    <div data-testid="mlx-hosting-tile" className="flex flex-col gap-3">
      <div className="flex items-baseline gap-2">
        <span className={HERO}>
          {intl.formatMessage(i18n.hostingRank, { rank: hosting.rank, size: hosting.size })}
        </span>
      </div>
      <span className={cx(LINE, WEIGHT.semibold)}>
        {intl.formatMessage(i18n.hostingFor, { requester: hosting.requesterName })}
      </span>
      <span className={cx('break-all font-mono text-lz-mono', WEIGHT.semibold)}>
        {hosting.modelId}
      </span>
      {hostingPhase(hosting.state) === 'loading' && (
        <LoadBar
          progress={nodeLoadProgress(hosting)}
          label={intl.formatMessage(i18n.loadBar, { node: intl.formatMessage(i18n.thisMacNode) })}
        />
      )}
      {hosting.pid != null && (
        <span className={LINE}>{intl.formatMessage(i18n.hostingPid, { pid: hosting.pid })}</span>
      )}
      <span className={LINE}>{intl.formatMessage(i18n.hostingSingleRefused)}</span>
    </div>
  );
}

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
  /**
   * RUNNING: the last /v1/status read (null before the first one lands) — of the single engine, or
   * of the distributed engine's rank 0 while that run owns this Mac and is up.
   */
  live: MlxLiveRead | null;
  /** RUNNING: the writing rate per read, oldest first. */
  history: readonly TpsSample[];
  /** RUNNING: the last rates this view measured, for the idle tile. */
  last: LastRates;
  /** RUNNING: who the engine is serving, from main's read of goose's in-flight list. */
  serving: MlxServing | null;
  /** MOUNTING: the memory the engine has claimed against the model's size. */
  mount: MountFill | null;
  /**
   * MOUNTING: the sidecar's own measure of the start (phase + resident bytes of the weights) —
   * preferred over `mount`; null on a backend that does not report it.
   */
  load?: SingleLoad | null;
  /** STOPPED: what mounting the picked model would cost. */
  cost: MountCost | null;
  /** FAILED: the engine's own error (null when the banner above already carries it). */
  failedError: string | null;
  /** The state's own action (Mount / Retry), drawn on the tile. */
  action: ReactNode;
  /** Which engine owns this Mac, in words ("Single · this Mac" / "Distributed · 2 nodes · JACCL"). */
  modeLabel: string;
  /**
   * The distributed engine's status. While it owns this Mac the tile IS that engine: its state,
   * rank 0's live read (`live`), and each rank's peak memory against its budget.
   */
  distributed: MlxDistributedStatus | null;
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

/**
 * A load with no measured figure yet: the same hollow track, a solid third of it, and no number —
 * "loading" is the claim, never a percentage it did not measure.
 */
function IndeterminateBar({ label }: { label: string }) {
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuetext={label}
      data-testid="mlx-load-indeterminate"
      className={cx('relative h-2.5 w-full overflow-hidden border border-current', RADIUS.pill)}
    >
      <div className="absolute inset-y-0 left-0 w-1/3 animate-lz-indeterminate bg-current" />
    </div>
  );
}

/** A load's progress: the measured bar with its figure, or the indeterminate track. */
function LoadBar({ progress, label }: { progress: LoadProgress | null; label: string }) {
  const intl = useIntl();
  if (progress == null) return <IndeterminateBar label={label} />;
  const figure = intl.formatMessage(i18n.loadBytes, {
    done: gb1(gib(progress.done)),
    total: gb1(gib(progress.total)),
  });
  return (
    <div className="flex flex-col gap-1">
      <TileBar fraction={progress.done / progress.total} label={label} />
      <span data-testid="mlx-load-figure" className={cx(LABEL, TNUM)}>
        {figure}
      </span>
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
    // The long pre-fill: no token is out yet. The single engine reports no per-request progress,
    // so there it draws no fraction — the prompt size, what the cache supplied and the engine's
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
    // The distributed engine reports how far into the prompt it is; the single engine does not.
    const read =
      !waiting && request.prefilledTokens != null && request.promptTokens
        ? request.prefilledTokens / request.promptTokens
        : null;
    return (
      <li data-testid="mlx-live-request" data-phase={request.phase} className="flex flex-col gap-1">
        <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
          <span className={cx('min-w-0 truncate', WEIGHT.semibold)}>{parts.join(' · ')}</span>
          <span className="shrink-0">
            {[
              read != null ? `${Math.round(Math.min(1, read) * 100)}%` : null,
              request.elapsedS != null ? formatElapsed(request.elapsedS) : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        </div>
        {read != null && <TileBar fraction={read} label={intl.formatMessage(i18n.rowReading)} />}
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

/** The sidecar's measured start: its phase in words, and resident ÷ on-disk bytes while loading. */
function LoadInstrument({ load }: { load: SingleLoad }) {
  const intl = useIntl();
  const word =
    load.phase === 'makingRoom'
      ? intl.formatMessage(i18n.makingRoom)
      : load.phase === 'starting'
        ? intl.formatMessage(i18n.startingEngine)
        : load.phase === 'loading'
          ? intl.formatMessage(i18n.loadingWeights)
          : load.phase === 'warming'
            ? intl.formatMessage(i18n.warming)
            : load.phase;
  const measured =
    (load.phase === 'loading' || load.phase === 'warming') &&
    load.residentBytes != null &&
    load.weightsBytes > 0;
  return (
    <div
      data-testid="mlx-mount-load"
      data-load-phase={load.phase}
      data-measured={measured}
      className="flex flex-col gap-2"
    >
      <div className="flex items-baseline gap-2">
        <span className={HERO}>{gb1(gib(load.weightsBytes))}</span>
        <span className={cx('text-lz-h2', TNUM)}>{intl.formatMessage(i18n.weightsGb)}</span>
      </div>
      <span className={cx(LINE, WEIGHT.semibold)}>{word}</span>
      <LoadBar
        progress={
          measured
            ? {
                unit: 'bytes',
                done: Math.min(load.residentBytes as number, load.weightsBytes),
                total: load.weightsBytes,
              }
            : null
        }
        label={word}
      />
    </div>
  );
}

function MountingInstrument({ mount, load }: { mount: MountFill | null; load: SingleLoad | null }) {
  const intl = useIntl();
  const gb = (n: number) =>
    intl.formatNumber(n, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (load) return <LoadInstrument load={load} />;
  if (mount == null) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-lz-h2">{intl.formatMessage(i18n.loadingWeights)}</p>
        <IndeterminateBar label={intl.formatMessage(i18n.loadingWeights)} />
      </div>
    );
  }
  if (mount.fraction == null) {
    // No baseline (the view opened mid-mount): the size is a fact, a percentage would be a guess.
    return (
      <div data-testid="mlx-mount-fill" data-measured="false" className="flex flex-col gap-2">
        <span className={HERO}>{gb(mount.modelGb)} GB</span>
        <IndeterminateBar label={intl.formatMessage(i18n.loadingWeights)} />
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
        <span className="shrink-0">
          {intl.formatMessage(i18n.available, { gb: gb(cost.freeGb) })}
        </span>
      </div>
    </div>
  );
}

/** A starting rank's word: goose's own step names, else the run's state vocabulary. */
function startWordText(intl: IntlShape, word: string): string {
  if (word === 'makingRoom') return intl.formatMessage(i18n.makingRoom);
  if (word === 'warming') return intl.formatMessage(i18n.warming);
  return distributedStateWord(intl, word);
}

/**
 * While the distributed run starts, each Mac as it loads its part: its own phase colour, its layer
 * range and its load progress (the backend's per-node figure, else the indeterminate track). Every
 * row sits inside a border in the tile's ink so a node in the tile's own colour still reads apart.
 */
function NodeStrip({ status }: { status: MlxDistributedStatus }) {
  const intl = useIntl();
  const rows =
    status.nodes.length > 0
      ? status.nodes.map((node) => ({
          key: `${node.rank}|${node.name}`,
          name: node.name,
          word: nodeStartWord(status, node),
          span: layerSpanShort(layerSpan(node)),
          progress: nodeLoadProgress(node),
        }))
      : // Preflight lists no ranks yet: the configured Macs are what it is checking.
        (status.config?.nodes ?? []).map((node) => ({
          key: node.name,
          name: node.name,
          word: nodeStartWord(status, { name: node.name, state: status.state }),
          span: null,
          progress: null,
        }));
  if (rows.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      <span className={LABEL}>{intl.formatMessage(i18n.distStarting)}</span>
      <ul data-testid="mlx-dist-strip" className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        {rows.map((row) => {
          const phase = nodePhase(row.word);
          return (
            <li key={row.key} className={cx('border-2 border-current', RADIUS.control)}>
              <div
                data-testid="mlx-dist-strip-node"
                data-node={row.name}
                data-phase={phase}
                className={cx('flex h-full flex-col gap-1.5 p-2.5', PHASE_FILL[phase])}
              >
                <div className={cx('flex items-baseline justify-between gap-2', LINE)}>
                  <span className={cx('min-w-0 truncate', WEIGHT.semibold)}>{row.name}</span>
                  <span className="shrink-0">{startWordText(intl, row.word)}</span>
                </div>
                {row.span && <span className={cx(LABEL, TNUM)}>{row.span}</span>}
                {phase === 'loading' && (
                  <LoadBar
                    progress={row.word === 'makingRoom' ? null : row.progress}
                    label={intl.formatMessage(i18n.loadBar, { node: row.name })}
                  />
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** A run that is up answers requests on its base URL; only then is rank 0's live read taken. */
function runIsUp(status: MlxDistributedStatus): boolean {
  return status.state === 'ready' || status.state === 'serving';
}

/**
 * The distributed run on the tile: rank 0's live read through the single engine's instrument
 * (reading / writing / queued, the rates, the request rows), the pipeline's slots and queue, then
 * every rank's peak against its budget. Before the first live read, the supervisor's in-flight count.
 */
function DistributedInstrument({
  status,
  live,
  history,
  last,
  serving,
}: {
  status: MlxDistributedStatus;
  live: MlxLiveRead | null;
  history: readonly TpsSample[];
  last: LastRates;
  serving: MlxServing | null;
}) {
  const intl = useIntl();
  if (status.state === 'preflight' || status.state === 'starting') {
    return (
      <div data-testid="mlx-dist-tile" className="flex flex-col gap-4">
        {status.modelId && (
          <span className={cx('break-all font-mono text-lz-mono', WEIGHT.semibold)}>
            {status.modelId}
          </span>
        )}
        <NodeStrip status={status} />
      </div>
    );
  }
  // Pipeline slots and the queue from rank 0's /v1/status; the tensor runner reports no slots.
  const load = [
    status.slots != null && status.slotsInUse != null
      ? intl.formatMessage(i18n.distSlots, { used: status.slotsInUse, slots: status.slots })
      : null,
    status.waiting != null ? intl.formatMessage(i18n.distWaiting, { count: status.waiting }) : null,
  ].filter((part): part is string => part != null);
  return (
    <div data-testid="mlx-dist-tile" className="flex flex-col gap-4">
      {status.modelId && (
        <span className={cx('break-all font-mono text-lz-mono', WEIGHT.semibold)}>
          {status.modelId}
        </span>
      )}
      {live != null && runIsUp(status) ? (
        <RunningInstrument live={live} history={history} last={last} serving={serving} />
      ) : status.inflight != null ? (
        <div className="flex items-baseline gap-2">
          <span data-testid="mlx-dist-tile-inflight" className={HERO}>
            {intl.formatNumber(status.inflight)}
          </span>
          <span className={LABEL}>
            {intl.formatMessage(i18n.distInflight, { count: status.inflight })}
          </span>
        </div>
      ) : (
        <span className={cx(LINE, WEIGHT.semibold)}>
          {intl.formatMessage(i18n.distInflightUnknown)}
        </span>
      )}
      {load.length > 0 && (
        <span data-testid="mlx-dist-tile-load" className={LINE}>
          {load.join(' · ')}
        </span>
      )}
      {!status.admissionOpen && (
        <span className={cx(LINE, WEIGHT.semibold)}>{intl.formatMessage(i18n.distHeld)}</span>
      )}
      <ul className="flex flex-col gap-3">
        {status.nodes.map((node) => {
          const plan = planForRank(status, node.rank);
          const budget = plan ? gib(plan.budgetBytes) : null;
          const peak = node.peakMemoryGb ?? null;
          const span = layerSpanShort(layerSpan(node));
          return (
            <li
              key={`${node.rank}|${node.name}`}
              data-testid="mlx-dist-tile-node"
              className="flex flex-col gap-1.5"
            >
              <div className={cx('flex items-baseline justify-between gap-3', LINE)}>
                <span className={cx('min-w-0 truncate', WEIGHT.semibold)}>
                  {span ? `${node.name} · ${span}` : node.name}
                </span>
                <span className="shrink-0">
                  {peak == null
                    ? intl.formatMessage(i18n.distNoPeak)
                    : budget != null
                      ? intl.formatMessage(i18n.distPeak, { peak: gb1(peak), budget: gb1(budget) })
                      : intl.formatMessage(i18n.distPeakNoBudget, { peak: gb1(peak) })}
                </span>
              </div>
              {peak != null && budget != null && budget > 0 && (
                <TileBar fraction={peak / budget} label={intl.formatMessage(i18n.distPeakBar)} />
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function MlxStateTile(props: MlxStateTileProps) {
  const intl = useIntl();
  const {
    state,
    unreachable,
    live,
    history,
    last,
    serving,
    mount,
    cost,
    failedError,
    action,
    modeLabel,
    distributed,
  } = props;
  const load = props.load ?? null;
  // A start the sidecar is measuring IS the mount, whatever word the state carries (making room
  // runs before the engine flips to mounting).
  const starting = state === 'mounting' || (load != null && state !== 'running');
  const dist = ownsTheMac(distributed) ? distributed : null;
  const hosting = !dist ? (distributed?.hosting ?? null) : null;
  const remoteStatus = useSyncExternalStore(
    subscribeMlxRemoteSingleStatus,
    latestMlxRemoteSingleStatus
  );
  const remote = remoteRouteUp(remoteStatus) ? remoteStatus : null;
  const engineUp = dist ? runIsUp(dist) : !hosting && state === 'running';
  const activity = engineUp && live?.ok ? mlxActivity(live.stats) : null;
  const phase: EnginePhase = dist
    ? runPhase(dist.state, dist.admissionOpen, activity)
    : hosting
      ? hostingPhase(hosting.state)
      : starting
        ? 'loading'
        : singlePhase(state, unreachable, activity);
  const word = starting ? 'mounting' : (state ?? (unreachable ? 'unreachable' : 'checking'));
  const wordText = dist
    ? distributedStateWord(intl, dist.state)
    : hosting
      ? hosting.state === 'loading'
        ? intl.formatMessage(i18n.hostingLoading, {
            rank: hosting.rank,
            requester: hosting.requesterName,
          })
        : distributedStateWord(intl, hosting.state)
      : intl.formatMessage(STATE_WORD[word]);
  const icon = hosting ? (
    hosting.state === 'loading' ? (
      <Loader2 className="animate-spin" />
    ) : (
      <Network />
    )
  ) : dist ? (
    runStateInFlight(dist.state) ? (
      <Loader2 className="animate-spin" />
    ) : (
      <Network />
    )
  ) : state === 'running' ? (
    <Play />
  ) : starting || (state === null && !unreachable) ? (
    <Loader2 className="animate-spin" />
  ) : state === 'failed' || unreachable ? (
    <X />
  ) : (
    <Square />
  );
  return (
    <div
      data-testid="mlx-state-badge"
      data-state={dist ? dist.state : word}
      data-mode={dist ? 'distributed' : hosting ? 'hosting' : 'single'}
      data-activity={activity ?? undefined}
      data-phase={phase}
      role="group"
      aria-label={intl.formatMessage(i18n.groupLabel, { state: wordText })}
      className={cx(
        'flex w-full shrink-0 flex-col gap-5 p-4 [&_svg]:shrink-0',
        state === 'running' || dist ? 'lg:w-[32rem]' : 'lg:w-80',
        RADIUS.card,
        PHASE_FILL[phase]
      )}
    >
      <div className="flex flex-col gap-1">
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
        <span data-testid="mlx-mode" className={cx(LINE, WEIGHT.semibold)}>
          {modeLabel}
        </span>
        {remote && (
          <span
            data-testid="mlx-remote"
            data-state={remote.state}
            title={remote.lastError ?? remote.modelId ?? undefined}
            className={cx(LINE, WEIGHT.semibold)}
          >
            {intl.formatMessage(i18n.servingFrom, {
              peer: remote.peerHostname ?? remote.peer ?? '',
              state: remote.state,
            })}
          </span>
        )}
      </div>
      {dist && (
        <DistributedInstrument
          status={dist}
          live={live}
          history={history}
          last={last}
          serving={serving}
        />
      )}
      {hosting && <HostingInstrument hosting={hosting} />}
      {!dist && !hosting && state === 'running' && (
        <RunningInstrument live={live} history={history} last={last} serving={serving} />
      )}
      {!dist && !hosting && starting && <MountingInstrument mount={mount} load={load} />}
      {!dist && !hosting && !starting && state === 'stopped' && <StoppedInstrument cost={cost} />}
      {!dist && !starting && state === 'failed' && (
        <p
          data-testid="mlx-failed-excerpt"
          title={failedError ?? undefined}
          className={cx('line-clamp-5 break-words', LINE, WEIGHT.semibold)}
        >
          {failedError ?? intl.formatMessage(i18n.failedFallback)}
        </p>
      )}
      {!dist && state === null && unreachable && (
        <p className={LINE}>{intl.formatMessage(i18n.statusUnread)}</p>
      )}
      {action && !hosting && <div className="mt-auto flex flex-wrap gap-2">{action}</div>}
    </div>
  );
}

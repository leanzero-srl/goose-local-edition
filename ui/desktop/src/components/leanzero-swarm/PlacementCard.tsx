import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Gauge, Loader2, Network, Play, RefreshCw, Square, Zap } from 'lucide-react';
import {
  Button,
  Chip,
  Disclosure,
  RADIUS,
  SURFACE,
  Segmented,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type EnginePhase,
  type Tone,
} from '../lz';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { ToneBanner } from './studio';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import {
  goalFigure,
  linkPeerOf,
  measuredFigure,
  mlxMeasureSpeed,
  mlxPlacementPlan,
  type PlacementBadge as PlacementBadgeDto,
  type PlacementCandidate,
  type PlacementGoal,
  type PlacementPlan,
  type SpeedFigure,
} from '../../acp/mlx-placement';
import { mlxEngineUnmount, type MlxEngineStatus } from '../../acp/mlx-engine';
import {
  mlxDistributedDiscover,
  mlxDistributedProvision,
  mlxDistributedStart,
  mlxDistributedStatus,
  mlxDistributedStop,
  type MlxDistributedProvision,
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStart,
  mlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { mlxErrorMessage } from './mlxErrorMessage';
import {
  cleanConfig,
  ownsTheMac,
  runnerUpdateMacs,
  splitContextFromFreeMemory,
  splitConfigFor,
  splitPlan,
  type SplitBlocker,
} from './mlxDistributed';
import { MLX_STATUS_POLL_MS } from './mlxLiveStats';
import { touchLocalNetwork } from './LocalNetworkNotice';
import { distributedStateWord } from './mlxModeLabel';
import { remotePhase, runPhase, singlePhase } from './mlxPhase';
import { dropRoute } from './routeSwitch';
import { formatGb } from './primitives';
import {
  macForPlacementNode,
  minutesAt,
  peerRefuses,
  routePeerName,
  SELF_KEY,
  type Mac,
} from './macs';
import { WithMacs, copyKey, copyRunning, useMacs } from './useMacs';

/**
 * RUN IT — the one way to start a model: on this Mac, on another Mac (its single engine, chat over
 * LeanZero Link), or split across your Macs. Every way carries goose's speed figure for the goal and
 * why it lost; the running one carries Stop, Measure speed and its state in the engine-phase
 * palette. A Mac that does not hold the model yet gets "Copy to <Mac> first" — and the start goes
 * on by itself when the copy lands. The split's own controls (preflight, checks, set up, the
 * supervisor's events) fold under its Details.
 */

const i18n = defineMessages({
  title: { id: 'placementCard.runIt', defaultMessage: 'Run it' },
  goalLabel: { id: 'placementCard.goalLabel', defaultMessage: 'What matters most' },
  goalChat: { id: 'placementCard.goalChat', defaultMessage: 'Chat' },
  goalLong: { id: 'placementCard.goalLong', defaultMessage: 'Long documents' },
  goalMany: { id: 'placementCard.goalMany', defaultMessage: 'Many requests' },
  planning: {
    id: 'placementCard.planning',
    defaultMessage: 'Measuring your Macs and the model…',
  },
  planFailed: { id: 'placementCard.planFailed', defaultMessage: 'Could not plan' },
  noPlanWays: {
    id: 'placementCard.noPlanWays',
    defaultMessage: 'Without a plan there is no speed figure — every way can still be started.',
  },
  best: { id: 'placementCard.best', defaultMessage: 'Best' },
  bestNow: { id: 'placementCard.bestNow', defaultMessage: 'Best you can start now' },
  runHere: { id: 'placementCard.runHere', defaultMessage: 'Run on this Mac' },
  runOn: { id: 'placementCard.runOn', defaultMessage: 'Run on {name}' },
  runAcross: {
    id: 'placementCard.runAcross',
    defaultMessage: 'Run across {count, plural, =2 {both Macs} other {# Macs}}',
  },
  runAcrossAny: { id: 'placementCard.runAcrossAny', defaultMessage: 'Run across your Macs' },
  tensor: { id: 'placementCard.tensorKind', defaultMessage: 'tensor split · {link}' },
  pipeline: { id: 'placementCard.pipelineKind', defaultMessage: 'pipeline split · {link}' },
  writes: { id: 'placementCard.writes', defaultMessage: '~{value} tok/s writing' },
  reads: { id: 'placementCard.reads', defaultMessage: '~{value} tok/s reading' },
  readsWholeTurn: {
    id: 'placementCard.readsWholeTurn',
    defaultMessage: '~{value} tok/s through a whole document turn, answer included',
  },
  total: {
    id: 'placementCard.total',
    defaultMessage: '~{value} tok/s total at {concurrency} at once',
  },
  range: { id: 'placementCard.range', defaultMessage: '{low}–{high}' },
  middleHalf: { id: 'placementCard.middleHalf', defaultMessage: '{low}–{high} middle half' },
  context: { id: 'placementCard.context', defaultMessage: '{tokens} context' },
  measured: {
    id: 'placementCard.measured',
    defaultMessage: '{runs, plural, one {measured · # run} other {measured · # runs}}',
  },
  estimated: { id: 'placementCard.estimated', defaultMessage: 'estimated' },
  run: { id: 'placementCard.run', defaultMessage: 'Run' },
  starting: { id: 'placementCard.starting', defaultMessage: 'Starting…' },
  stop: { id: 'placementCard.stop', defaultMessage: 'Stop' },
  measure: { id: 'placementCard.measure', defaultMessage: 'Measure speed' },
  measuring: {
    id: 'placementCard.measuring',
    defaultMessage: 'Measuring: a ~2k-token prompt, then 256 tokens…',
  },
  measureFirst: {
    id: 'placementCard.measureFirst',
    defaultMessage:
      'Running and not measured yet — Measure speed replaces the estimate with this Mac’s own number.',
  },
  splitChecking: {
    id: 'placementCard.splitChecking',
    defaultMessage: 'Checking {nodes} for {model}…',
  },
  splitBuilding: {
    id: 'placementCard.splitBuilding',
    defaultMessage: 'Building goose’s Python on {nodes}…',
  },
  splitStarting: {
    id: 'placementCard.splitStarting',
    defaultMessage: 'Starting {model} across {nodes}…',
  },
  splitNotSplittable: {
    id: 'placementCard.splitNotSplittable',
    defaultMessage: 'goose found no way to split {model}.',
  },
  splitModelMissing: {
    id: 'placementCard.splitModelMissing',
    defaultMessage: '{model} is not on {nodes} yet — copy it there, then Run.',
  },
  splitNoUv: {
    id: 'placementCard.splitNoUv',
    defaultMessage: 'goose cannot build its Python on {nodes}: there is no uv there.',
  },
  splitNotFound: {
    id: 'placementCard.splitNotFound',
    defaultMessage: 'goose could not find what the split needs: {items}',
  },
  splitBuildFailed: {
    id: 'placementCard.splitBuildFailed',
    defaultMessage: 'goose’s Python did not build on {node}: {reason}',
  },
  splitNoPeers: {
    id: 'placementCard.splitNoPeers',
    defaultMessage: 'goose planned this split without another Mac — open Details › Set up.',
  },
  distributedOwns: {
    id: 'placementCard.distributedOwns',
    defaultMessage: 'The split owns this Mac — stop it to run a model here alone.',
  },
  refresh: { id: 'placementCard.refresh', defaultMessage: 'Plan again' },
  others: {
    id: 'placementCard.others',
    defaultMessage: '{count, plural, one {# other split} other {# other splits}}',
  },
  slower: {
    id: 'placementCard.slower',
    defaultMessage: 'Slower for this: ~{mine} vs ~{best} tok/s',
  },
  tied: {
    id: 'placementCard.tied',
    defaultMessage: 'As fast within the error (~{mine} vs ~{best} tok/s), but needs more Macs',
  },
  short: { id: 'placementCard.short', defaultMessage: 'Does not fit: short {gb} on {node}' },
  shortSplit: { id: 'placementCard.shortSplit', defaultMessage: 'Does not fit: short {gb}' },
  fitUnknown: { id: 'placementCard.fitUnknown', defaultMessage: 'Fit unknown' },
  noFigureReason: { id: 'placementCard.noFigureReason', defaultMessage: 'No speed figure' },
  smallerContext: {
    id: 'placementCard.smallerContext',
    defaultMessage: 'fits only at {tokens} context',
  },
  tradeOffReadsFaster: {
    id: 'placementCard.tradeOffReadsFaster',
    defaultMessage:
      '{name} alone fits this model. Split across your Macs it writes ~{splitWrite} tok/s against ~{singleWrite} there, and reads prompts {ratio}× faster (~{splitRead} vs ~{singleRead} tok/s) — worth it only when prompts are long and replies short.',
  },
  tradeOffReadsSlower: {
    id: 'placementCard.tradeOffReadsSlower',
    defaultMessage:
      '{name} alone fits this model. Split across your Macs it writes ~{splitWrite} tok/s against ~{singleWrite} there, and reads prompts slower too (~{splitRead} vs ~{singleRead} tok/s).',
  },
  tradeOffWritesOnly: {
    id: 'placementCard.tradeOffWritesOnly',
    defaultMessage:
      '{name} alone fits this model. Split across your Macs it writes ~{splitWrite} tok/s against ~{singleWrite} there.',
  },
  splitContextFixed: {
    id: 'placementCard.splitContextFixed',
    defaultMessage:
      'Its {tokens} context was sized from the memory free when it started, and stays that size while it runs — restart it with more memory free to grow it.',
  },
  started: { id: 'placementCard.started', defaultMessage: 'Starting — this card follows it.' },
  switching: {
    id: 'placementCard.switching',
    defaultMessage: 'Stopping {model} where it runs now ({where}) so this way gets its memory…',
  },
  switchStopFailed: {
    id: 'placementCard.switchStopFailed',
    defaultMessage:
      'Nothing started: {model} could not be stopped where it runs now ({where}): {reason}',
  },
  measuredResult: {
    id: 'placementCard.measuredResult',
    defaultMessage: 'Measured: {decode} tok/s writing, {prefill} tok/s reading',
  },
  storeErrors: {
    id: 'placementCard.storeErrors',
    defaultMessage: 'Unreadable lines in the measurement store',
  },
  badgeThisMac: { id: 'placementCard.badgeThisMac', defaultMessage: 'Fits this Mac' },
  badgePeer: { id: 'placementCard.badgePeer', defaultMessage: 'Fits {name}' },
  badgeEvery: {
    id: 'placementCard.badgeEvery',
    defaultMessage: 'Fits {count, plural, =2 {both Macs} other {all # Macs}}',
  },
  badgeBoth: { id: 'placementCard.badgeBoth', defaultMessage: 'Needs both Macs' },
  badgeTooBig: { id: 'placementCard.badgeTooBig', defaultMessage: 'Too big, short {gb}' },
  badgeUnknown: { id: 'placementCard.badgeUnknown', defaultMessage: 'Fit unknown' },
  badgeOnceStops: {
    id: 'placementCard.badgeOnceStops',
    defaultMessage: '{badge} · fits once {models} {count, plural, one {stops} other {stop}}',
  },
  stopsFirst: {
    id: 'placementCard.stopsFirst',
    defaultMessage: 'Run stops {model} on {where} first.',
  },
  fitsOnceStops: {
    id: 'placementCard.fitsOnceStops',
    defaultMessage: 'Fits once {model} stops — Run stops it on {where} first.',
  },
  thisMac: { id: 'placementCard.thisMac', defaultMessage: 'this Mac' },
  yourMacs: { id: 'placementCard.yourMacs', defaultMessage: 'your Macs' },
  actionFailed: { id: 'placementCard.actionFailed', defaultMessage: 'The action failed' },
  refusedUnnamed: {
    id: 'placementCard.refusedUnnamed',
    defaultMessage: 'Refused, and goose named no reason',
  },
  liveMounting: { id: 'placementCard.live.mounting', defaultMessage: 'Mounting' },
  liveRunning: { id: 'placementCard.live.running', defaultMessage: 'Running' },
  liveFailed: { id: 'placementCard.live.failed', defaultMessage: 'Failed' },
  liveReconnecting: { id: 'placementCard.live.reconnecting', defaultMessage: 'Reconnecting' },
  copyFirst: {
    id: 'placementCard.copyFirst',
    defaultMessage:
      'Copy to {name} first · {kind, select, thunderbolt {Thunderbolt} other {network}}',
  },
  copyFirstMinutes: {
    id: 'placementCard.copyFirstMinutes',
    defaultMessage:
      'Copy to {name} first (~{minutes} min over {kind, select, thunderbolt {Thunderbolt} other {the network}})',
  },
  copyFirstWhy: {
    id: 'placementCard.copyFirstWhy',
    defaultMessage:
      '{model} ({size}) is not on {name} yet; the start goes on by itself once it lands.',
  },
  copyingThen: {
    id: 'placementCard.copyingThen',
    defaultMessage: 'Copying to {name} — {pct}% · it starts when the copy lands',
  },
  copyThenFailed: {
    id: 'placementCard.copyThenFailed',
    defaultMessage: 'The copy to {name} did not finish, so nothing started: {reason}',
  },
  details: { id: 'placementCard.details', defaultMessage: 'Details' },
  detailsMeta: {
    id: 'placementCard.detailsMeta',
    defaultMessage: 'set up, preflight, checks, events',
  },
  stopSplitTitle: {
    id: 'placementCard.stopSplitTitle',
    defaultMessage: 'Stop the split?',
  },
  stopSplitMessage: {
    id: 'placementCard.stopSplitMessage',
    defaultMessage:
      'Every part on {nodes} is stopped and verified gone. Requests in flight are cut off.',
  },
  keepRunning: { id: 'placementCard.keepRunning', defaultMessage: 'Keep running' },
  runnerUpdating: {
    id: 'placementCard.runnerUpdating',
    defaultMessage: 'Updating the split’s runner on {nodes}…',
  },
  runnerStep: {
    id: 'placementCard.runnerStep',
    defaultMessage:
      '{step, select, check {checking} uv {finding uv} venv {making the env} install {installing} done {done} fail {failed} other {starting}}',
  },
  noticeDetails: { id: 'placementCard.noticeDetails', defaultMessage: 'Details' },
});

type NoticeTone = Exclude<Tone, 'secondary'>;

/** Why a start did not happen: the words the card shows, and what stands behind them for Details. */
interface StartRefusal {
  text: string;
  detail?: string | null;
}

const said = (text: string): StartRefusal => ({ text });

/** A start's answer as the card shows it: `null` once it started, else its refusal and Details. */
function startRefusal(
  response: { started: boolean; refusal?: { message: string; detail?: string | null } | null },
  refused: string
): StartRefusal | null {
  if (response.started) return null;
  return { text: response.refusal?.message ?? refused, detail: response.refusal?.detail };
}

/**
 * A start rebuilding goose's split runner on older-pin Macs (Q-116): which Macs, then each one's
 * step and latest line as its node reports them.
 */
function RunnerUpdateNotice({ update }: { update: MlxDistributedProvision }) {
  const intl = useIntl();
  const nodes = intl.formatList(runnerUpdateMacs(update), { type: 'conjunction' });
  return (
    <div data-testid="placement-runner-update" className="flex flex-col gap-1.5">
      <ToneBanner
        tone="accent"
        live
        label={intl.formatMessage(i18n.title)}
        text={intl.formatMessage(i18n.runnerUpdating, { nodes })}
      />
      <ul className="flex flex-col gap-1">
        {update.nodes.map((n) => {
          const last = n.lines.length > 0 ? n.lines[n.lines.length - 1] : null;
          return (
            <li
              key={`${n.rank}|${n.python}`}
              data-testid="placement-runner-node"
              data-state={n.state}
              className="flex min-w-0 items-center gap-2"
            >
              <Chip
                tone={n.state === 'failed' ? 'err' : n.state === 'done' ? 'ok' : 'accent'}
                icon={n.state === 'running' ? <Loader2 className="animate-spin" /> : undefined}
              >
                {intl.formatMessage(i18n.runnerStep, { step: n.step ?? 'start' })}
              </Chip>
              <span className={cx('shrink-0', TYPE.body, WEIGHT.semibold)}>{n.name}</span>
              {last && <span className={cx('min-w-0 truncate', TYPE.meta)}>{last}</span>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** Details under the split: folded until the person opens it, and remembered for the session. */
const SPLIT_DETAILS_KEY = 'placement-split-details-open';

function splitDetailsOpenAtFirst(): boolean {
  return sessionStorage.getItem(SPLIT_DETAILS_KEY) === 'open';
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** A split's blocker, in the person's words, naming each Mac. */
export function splitBlockerText(intl: IntlShape, blocker: SplitBlocker, modelId: string): string {
  const model = modelId.split('/').pop() || modelId;
  switch (blocker.kind) {
    case 'notSplittable':
      return blocker.reason ?? intl.formatMessage(i18n.splitNotSplittable, { model });
    case 'modelMissing':
      return intl.formatMessage(i18n.splitModelMissing, {
        model,
        nodes: intl.formatList(blocker.nodes, { type: 'conjunction' }),
      });
    case 'noUv':
      return intl.formatMessage(i18n.splitNoUv, {
        nodes: intl.formatList(blocker.nodes, { type: 'conjunction' }),
      });
    case 'notFound':
      return intl.formatMessage(i18n.splitNotFound, {
        items: blocker.items
          .map((item) => `${item.node ? `${item.node} · ` : ''}${item.field}: ${item.reason}`)
          .join('; '),
      });
  }
}

const GIB = 1024 * 1024 * 1024;

function gb(bytes: number): string {
  return `${(bytes / GIB).toFixed(1)} GB`;
}

function tps(value: number): string {
  return value >= 100 ? value.toFixed(0) : value.toFixed(1);
}

/** A model as the card names it: its folder name, without the publisher. */
export function shortModel(modelId: string): string {
  return modelId.split('/').pop() || modelId;
}

function linkWord(candidate: PlacementCandidate): string {
  return candidate.key.link === 'jaccl' ? 'JACCL' : (candidate.key.link ?? '');
}

function figureText(
  intl: IntlShape,
  goal: PlacementGoal,
  figure: SpeedFigure,
  candidate: PlacementCandidate
): string {
  const value = tps(figure.estimate.value);
  const concurrency = candidate.speed.concurrency;
  if (goal === 'longDocuments') {
    return intl.formatMessage(candidate.speed.turn ? i18n.readsWholeTurn : i18n.reads, { value });
  }
  if (goal === 'manyRequests')
    return intl.formatMessage(i18n.total, { value, concurrency: concurrency ?? '—' });
  return intl.formatMessage(i18n.writes, { value });
}

/**
 * The range beside a figure: an estimate's error, or — for a measured figure — the runs' middle half,
 * only when there is more than one run and they differ (measuredFigure, the rule the Engine tile and
 * the tray use). One run is never drawn as "29.6–29.6" (Q-129).
 */
function FigureRange({ figure }: { figure: SpeedFigure }) {
  const intl = useIntl();
  if (figure.measured) {
    const spread = measuredFigure(figure)?.spread;
    if (!spread) return null;
    return (
      <span className={cx(TYPE.meta, TNUM)} data-testid="placement-figure-range">
        {intl.formatMessage(i18n.middleHalf, { low: tps(spread.low), high: tps(spread.high) })}
      </span>
    );
  }
  return (
    <span className={cx(TYPE.meta, TNUM)} data-testid="placement-figure-range">
      {intl.formatMessage(i18n.range, {
        low: tps(figure.estimate.low),
        high: tps(figure.estimate.high),
      })}
    </span>
  );
}

function SourceChip({ figure }: { figure: SpeedFigure }) {
  const intl = useIntl();
  return figure.measured ? (
    <Chip
      tone="ok"
      title={figure.lastMeasuredMs ? new Date(figure.lastMeasuredMs).toLocaleString() : undefined}
    >
      {intl.formatMessage(i18n.measured, { runs: figure.runs })}
    </Chip>
  ) : (
    <Chip tone="secondary">{intl.formatMessage(i18n.estimated)}</Chip>
  );
}

/** Why a candidate lost, in words; `null` for the winners. */
export function outcomeText(intl: IntlShape, candidate: PlacementCandidate): string | null {
  const o = candidate.outcome;
  switch (o.code) {
    case 'best':
    case 'bestAvailableNow':
      return null;
    case 'slower':
      return intl.formatMessage(i18n.slower, { mine: tps(o.mine), best: tps(o.best) });
    case 'tiedNeedsMoreMacs':
      return intl.formatMessage(i18n.tied, { mine: tps(o.mine), best: tps(o.best) });
    case 'doesNotFit': {
      const short = candidate.fit.shortBytes ?? 0;
      return candidate.fit.shortNode
        ? intl.formatMessage(i18n.short, { gb: gb(short), node: candidate.fit.shortNode })
        : intl.formatMessage(i18n.shortSplit, { gb: gb(short) });
    }
    case 'fitUnknown':
      return `${intl.formatMessage(i18n.fitUnknown)}: ${o.reason}`;
    case 'notSupported':
      return o.reason;
    case 'noFigure':
      return `${intl.formatMessage(i18n.noFigureReason)}: ${o.reason}`;
  }
}

/** A single-Mac candidate goose's fit rule lets one engine hold. */
function fitsAlone(c: PlacementCandidate): boolean {
  return (
    c.key.kind === 'single' &&
    c.supported &&
    (c.fit.status === 'fits' || c.fit.status === 'smallerContext')
  );
}

/**
 * What the split costs against running the model on ONE Mac, from the plan's own speed figures
 * (measured where goose has runs, else its estimate — never a typed number). Null when no single
 * Mac fits the model, or either side lacks a writing figure. The single compared is the fitting
 * Mac that writes fastest.
 */
export interface SplitTradeOff {
  single: PlacementCandidate;
  splitWrite: number;
  singleWrite: number;
  splitRead: number | null;
  singleRead: number | null;
}

export function splitTradeOff(
  plan: PlacementPlan | null,
  split: PlacementCandidate
): SplitTradeOff | null {
  if (split.key.kind === 'single') return null;
  const splitWrite = split.speed.decode?.estimate.value;
  if (splitWrite == null) return null;
  let single: PlacementCandidate | null = null;
  for (const c of plan?.candidates ?? []) {
    const write = c.speed.decode?.estimate.value;
    if (!fitsAlone(c) || write == null) continue;
    if (single == null || write > (single.speed.decode?.estimate.value ?? 0)) single = c;
  }
  if (!single) return null;
  return {
    single,
    splitWrite,
    singleWrite: single.speed.decode!.estimate.value,
    splitRead: split.speed.prefill?.estimate.value ?? null,
    singleRead: single.speed.prefill?.estimate.value ?? null,
  };
}

function tradeOffText(intl: IntlShape, t: SplitTradeOff): string {
  const base = {
    name: t.single.nodeNames[0] ?? '—',
    splitWrite: tps(t.splitWrite),
    singleWrite: tps(t.singleWrite),
  };
  if (t.splitRead == null || t.singleRead == null || t.singleRead <= 0) {
    return intl.formatMessage(i18n.tradeOffWritesOnly, base);
  }
  const reads = { ...base, splitRead: tps(t.splitRead), singleRead: tps(t.singleRead) };
  return t.splitRead > t.singleRead
    ? intl.formatMessage(i18n.tradeOffReadsFaster, {
        ...reads,
        ratio: intl.formatNumber(t.splitRead / t.singleRead, { maximumFractionDigits: 1 }),
      })
    : intl.formatMessage(i18n.tradeOffReadsSlower, reads);
}

function badgeTone(badge: PlacementBadgeDto): Tone | undefined {
  switch (badge.kind) {
    case 'fitsThisMac':
      return 'ok';
    case 'fitsPeer':
      return 'accent';
    case 'needsBothMacs':
      return 'warn';
    case 'tooBig':
      return 'err';
    case 'unknown':
      return undefined;
  }
}

/**
 * The picker's badge for one model: goose's badge, plus every Mac ONE engine fits it on, by name,
 * from the plan's own single candidates. goose's `fitsThisMac` names no Mac, and under "Memory on
 * Work's Mac Studio" the picker's "Fits this Mac" read as the Studio when it meant the MacBook (Q-42).
 */
export interface PickerBadge {
  badge: PlacementBadgeDto;
  /** The Macs a single engine fits the model on (goose's fit rule), in the plan's order. */
  fitsOn: string[];
  /** How many Macs the plan judged alone — "both" / "all" only when every one fits. */
  macs: number;
  /** The other models Run stops first for this fit to hold (goose's `badgeAfterStopping`). */
  afterStopping: string[];
}

/** The picker's badge from one plan (`null` when goose sent none). */
export function pickerBadgeOf(plan: PlacementPlan): PickerBadge | null {
  if (!plan.badge) return null;
  const singles = (plan.candidates ?? []).filter((c) => c.key.kind === 'single');
  const fitsOn = singles
    .filter((c) => c.supported && (c.fit.status === 'fits' || c.fit.status === 'smallerContext'))
    .flatMap((c) => (c.nodeNames[0] ? [c.nodeNames[0]] : []));
  return {
    badge: plan.badge,
    fitsOn,
    macs: singles.length,
    afterStopping: plan.badgeAfterStopping ?? [],
  };
}

/** The model picker's badge: where this model fits, measured just now — each Mac by its name. */
export function PlacementBadge({ badge: picker }: { badge: PickerBadge }) {
  const intl = useIntl();
  const { badge, fitsOn, macs, afterStopping } = picker;
  const fitsAlone = badge.kind === 'fitsThisMac' || badge.kind === 'fitsPeer';
  const fitText =
    fitsAlone && fitsOn.length >= 2 && fitsOn.length === macs
      ? intl.formatMessage(i18n.badgeEvery, { count: fitsOn.length })
      : fitsAlone && fitsOn.length > 0
        ? intl.formatMessage(i18n.badgePeer, {
            name: intl.formatList(fitsOn, { type: 'conjunction' }),
          })
        : badge.kind === 'fitsThisMac'
          ? intl.formatMessage(i18n.badgeThisMac)
          : badge.kind === 'fitsPeer'
            ? intl.formatMessage(i18n.badgePeer, { name: badge.name })
            : badge.kind === 'needsBothMacs'
              ? intl.formatMessage(i18n.badgeBoth)
              : badge.kind === 'tooBig'
                ? intl.formatMessage(i18n.badgeTooBig, { gb: gb(badge.shortBytes) })
                : intl.formatMessage(i18n.badgeUnknown);
  // A fit that holds only once the model serving now stops says so — never "too big" (Q-120).
  const text =
    afterStopping.length > 0
      ? intl.formatMessage(i18n.badgeOnceStops, {
          badge: fitText,
          models: intl.formatList(afterStopping.map(shortModel), { type: 'conjunction' }),
          count: afterStopping.length,
        })
      : fitText;
  return (
    <Chip tone={badgeTone(badge)} title={badge.kind === 'unknown' ? badge.reason : undefined}>
      {text}
    </Chip>
  );
}

/**
 * Every local model's plan, planned once per change of `modelKey`: the picker's badges read it. A
 * failed plan leaves no entry — the Run it card names the failure.
 */
export function usePlacementPlans(modelKey: string): Map<string, PlacementPlan> {
  const [plans, setPlans] = useState<Map<string, PlacementPlan>>(new Map());
  useEffect(() => {
    let cancelled = false;
    mlxPlacementPlan('chat')
      .then((response) => {
        if (cancelled) return;
        setPlans(new Map(response.plans.map((plan) => [plan.modelId, plan])));
      })
      .catch(() => {
        if (!cancelled) setPlans(new Map());
      });
    return () => {
      cancelled = true;
    };
  }, [modelKey]);
  return plans;
}

/** The picker's badge per model, from the plans. */
export function badgesOf(plans: Map<string, PlacementPlan>): Map<string, PickerBadge> {
  const out = new Map<string, PickerBadge>();
  for (const [id, plan] of plans) {
    const badge = pickerBadgeOf(plan);
    if (badge) out.set(id, badge);
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// The ways
// ---------------------------------------------------------------------------------------------

/** One way to run the model; `candidate` = goose's plan for it (null when no plan could be read). */
export interface Way {
  key: string;
  kind: 'local' | 'peer' | 'split';
  candidate: PlacementCandidate | null;
  /** A peer way: the Mac, and the Link node id its single engine is started by. */
  mac: Mac | null;
  peerNodeId: string | null;
}

function splitRank(c: PlacementCandidate): number {
  if (!c.supported) return 2;
  return c.action.kind === 'unavailable' ? 1 : 0;
}

/**
 * The plan's candidates as the ways a person picks between: every single-Mac candidate (this Mac
 * first), then ONE split — the supported, startable one first; the other splits fold away with
 * their reason. With no plan, the ways goose can start anyway: this Mac, each peer that lets this
 * Mac load models, the split when goose offers it.
 */
export function waysOf(
  plan: PlacementPlan | null,
  macs: readonly Mac[],
  distributedCapability: boolean
): { ways: Way[]; otherSplits: PlacementCandidate[] } {
  if (plan && !plan.error && (plan.candidates?.length ?? 0) > 0) {
    const singles = (plan.candidates ?? []).filter((c) => c.key.kind === 'single');
    const splits = [...(plan.candidates ?? []).filter((c) => c.key.kind !== 'single')].sort(
      (a, b) => splitRank(a) - splitRank(b)
    );
    const local = singles.filter((c) => c.key.nodes[0] === 'local');
    const peers = singles.filter((c) => c.key.nodes[0] !== 'local');
    const ways: Way[] = [
      ...local.map((c) => ({
        key: c.id,
        kind: 'local' as const,
        candidate: c,
        mac: null,
        peerNodeId: null,
      })),
      ...peers.map((c) => ({
        key: c.id,
        kind: 'peer' as const,
        candidate: c,
        mac: macForPlacementNode(macs, c.key.nodes[0] ?? ''),
        peerNodeId: linkPeerOf(c),
      })),
      ...(splits[0]
        ? [
            {
              key: splits[0].id,
              kind: 'split' as const,
              candidate: splits[0],
              mac: null,
              peerNodeId: null,
            },
          ]
        : []),
    ];
    return { ways, otherSplits: splits.slice(1) };
  }
  const ways: Way[] = [
    { key: 'local', kind: 'local', candidate: null, mac: null, peerNodeId: null },
  ];
  for (const mac of macs) {
    if (mac.isSelf || !mac.online || peerRefuses(mac, 'manage') || !mac.nodeId) continue;
    ways.push({
      key: `peer:${mac.key}`,
      kind: 'peer',
      candidate: null,
      mac,
      peerNodeId: mac.nodeId,
    });
  }
  if (distributedCapability) {
    ways.push({ key: 'split', kind: 'split', candidate: null, mac: null, peerNodeId: null });
  }
  return { ways, otherSplits: [] };
}

/** The engine a way IS right now — whichever model it holds — in the engine-phase palette. */
export function wayServing(
  way: Way,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null
): { phase: EnginePhase; state: string; modelId: string } | null {
  if (way.kind === 'local') {
    if (!single?.modelId) return null;
    if (single.state !== 'mounting' && single.state !== 'running' && single.state !== 'failed') {
      return null;
    }
    return {
      phase: singlePhase(single.state, false, null),
      state: single.state,
      modelId: single.modelId,
    };
  }
  if (way.kind === 'peer') {
    const remote = latestMlxRemoteSingleStatus();
    if (!way.peerNodeId || remote?.peer !== way.peerNodeId || !remote.modelId) return null;
    if (remote.state === 'off') return null;
    return {
      phase: remotePhase(remote.state, null),
      state: remote.state === 'ready' ? 'running' : remote.state,
      modelId: remote.modelId,
    };
  }
  if (!distributed?.modelId) return null;
  if (!ownsTheMac(distributed) && distributed.state !== 'failed') return null;
  return {
    phase: runPhase(distributed.state, distributed.admissionOpen),
    state: distributed.state,
    modelId: distributed.modelId,
  };
}

/** The engine a way IS right now for `modelId`; null = not this way, or another model. */
export function wayLive(
  way: Way,
  modelId: string,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null
): { phase: EnginePhase; state: string } | null {
  const serving = wayServing(way, single, distributed);
  if (!serving || serving.modelId !== modelId) return null;
  return { phase: serving.phase, state: serving.state };
}

/** A way serving chat now (not failed), whichever model — what Run stops before it starts. */
export interface ServingWay {
  way: Way;
  modelId: string;
}

/**
 * Every way serving now, found among the card's ways or — when the plan has no row for it (a Mac
 * the plan could not measure) — built from what serves: Run is a SWITCH, so each of these stops
 * first, whether it holds the picked model or another (Q-119: the 27B on the Studio while Flash is
 * picked).
 */
export function servingWays(
  ways: readonly Way[],
  macs: readonly Mac[],
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null
): ServingWay[] {
  const remote = latestMlxRemoteSingleStatus();
  const candidates: Way[] = [
    ways.find((w) => w.kind === 'local') ?? {
      key: 'local',
      kind: 'local',
      candidate: null,
      mac: null,
      peerNodeId: null,
    },
    ...(remote?.peer
      ? [
          ways.find((w) => w.kind === 'peer' && w.peerNodeId === remote.peer) ?? {
            key: `peer:${remote.peer}`,
            kind: 'peer' as const,
            candidate: null,
            mac: macs.find((m) => m.nodeId === remote.peer) ?? null,
            peerNodeId: remote.peer,
          },
        ]
      : []),
    ways.find((w) => w.kind === 'split') ?? {
      key: 'split',
      kind: 'split',
      candidate: null,
      mac: null,
      peerNodeId: null,
    },
  ];
  return candidates.flatMap((way) => {
    const serving = wayServing(way, single, distributed);
    return serving && serving.state !== 'failed' ? [{ way, modelId: serving.modelId }] : [];
  });
}

function LiveChip({ live }: { live: { phase: EnginePhase; state: string } }) {
  const intl = useIntl();
  const word =
    live.state === 'mounting'
      ? intl.formatMessage(i18n.liveMounting)
      : live.state === 'running'
        ? intl.formatMessage(i18n.liveRunning)
        : live.state === 'failed'
          ? intl.formatMessage(i18n.liveFailed)
          : live.state === 'reconnecting'
            ? intl.formatMessage(i18n.liveReconnecting)
            : distributedStateWord(intl, live.state);
  return (
    <Chip phase={live.phase}>
      <span data-testid="placement-live" data-phase={live.phase}>
        {word}
      </span>
    </Chip>
  );
}

interface PlacementCardProps {
  modelId: string;
  single: MlxEngineStatus | null;
  distributed: MlxDistributedStatus | null;
  /** Mount the model on this Mac's single engine (the view's own mount path, gate and all). */
  onMountHere: () => void;
  /** Unmount this Mac's single engine. */
  onStopHere: () => void;
  mountBusy: boolean;
  /** goose offers the split at all (the `mlxDistributed` capability). */
  distributedCapability?: boolean;
  /** The split's own controls — set up, preflight, checks, events — folded under its Details. */
  splitDetails?: ReactNode;
}

function PlacementCardBody({
  modelId,
  single,
  distributed,
  onMountHere,
  onStopHere,
  mountBusy,
  distributedCapability = false,
  splitDetails,
}: PlacementCardProps) {
  const intl = useIntl();
  const macs = useMacs();
  const [goal, setGoal] = useState<PlacementGoal>('chat');
  const [plan, setPlan] = useState<PlacementPlan | null>(null);
  const [storeErrors, setStoreErrors] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  // `follows`: the way a "Starting — this card follows it." notice speaks for; it ends when that way
  // serves or fails (it stayed 25 min after a switch — Q-36).
  // `detail`: what stands behind a refusal (a node's output, pids) — under Details, never inline.
  const [notice, setNotice] = useState<{
    tone: NoticeTone;
    text: string;
    follows?: string;
    detail?: string | null;
  } | null>(null);
  const [othersOpen, setOthersOpen] = useState(false);
  const [detailsOpen, setDetailsOpenState] = useState(splitDetailsOpenAtFirst);
  const setDetailsOpen = useCallback((open: boolean) => {
    sessionStorage.setItem(SPLIT_DETAILS_KEY, open ? 'open' : 'folded');
    setDetailsOpenState(open);
  }, []);
  const [confirmStopSplit, setConfirmStopSplit] = useState(false);
  const [, setRemoteTick] = useState(0);
  const request = useRef(0);

  useEffect(() => subscribeMlxRemoteSingleStatus(() => setRemoteTick((t) => t + 1)), []);

  const load = useCallback(async () => {
    const mine = ++request.current;
    setLoading(true);
    setError(null);
    // Where chat goes now decides which way is running; a failed read publishes "unknown".
    mlxRemoteSingleStatus().catch(() => undefined);
    try {
      const response = await mlxPlacementPlan(goal, modelId);
      if (mine !== request.current) return;
      setPlan(response.plans[0] ?? null);
      setStoreErrors(response.storeErrors ?? []);
    } catch (e) {
      if (mine !== request.current) return;
      setPlan(null);
      setError(mlxErrorMessage(e, intl.formatMessage(i18n.planFailed)));
    } finally {
      if (mine === request.current) setLoading(false);
    }
  }, [goal, modelId, intl]);

  useEffect(() => {
    setNotice(null);
    void load();
  }, [load]);

  // What serves this model decides the plan's credits and its notes ("its memory could not be
  // read" while the Studio's engine loaded stayed under Run it after it served — 3.0.31). The plan
  // is asked again whenever that changes; the notice stays, since a start's failure moves the state
  // too.
  // A Mac coming back on LeanZero Link, or the route's failure changing its words, changes what the
  // plan can say: after a Link outage the Studio row kept "unreachable over LeanZero Link" and no Run
  // minutes after Link was back (Q-35, R3 2026-09-25).
  const remoteNow = latestMlxRemoteSingleStatus();
  const servingKey = [
    single?.state,
    single?.modelId,
    distributed?.state,
    distributed?.modelId,
    remoteNow?.state,
    remoteNow?.peer,
    remoteNow?.modelId,
    remoteNow?.lastError,
  ].join('|');
  // A Mac this card already knew coming back ONLINE on LeanZero Link re-plans too (not the roster's
  // first arrival — the plan was just asked for it).
  const onlineBefore = useRef<Map<string, boolean> | null>(null);
  useEffect(() => {
    const now = new Map(macs.macs.map((m) => [m.key, m.online] as const));
    const before = onlineBefore.current;
    onlineBefore.current = now;
    if (before && [...now].some(([key, online]) => online && before.get(key) === false)) {
      void load();
    }
  }, [macs.macs, load]);
  useEffect(() => {
    if (!notice?.follows) return;
    const way = waysOf(plan, macs.macs, distributedCapability).ways.find(
      (w) => w.key === notice.follows
    );
    const live = way ? wayLive(way, modelId, single, distributed) : null;
    if (
      live &&
      live.state !== 'mounting' &&
      live.state !== 'starting' &&
      live.state !== 'preflight'
    ) {
      setNotice(null);
    }
  }, [notice, plan, macs.macs, distributedCapability, modelId, single, distributed, servingKey]);
  const plannedFor = useRef(servingKey);
  useEffect(() => {
    if (plannedFor.current === servingKey) return;
    plannedFor.current = servingKey;
    void load();
  }, [servingKey, load]);

  const refused = intl.formatMessage(i18n.refusedUnnamed);
  const savedSplit = distributed?.config ?? null;
  // A start rebuilding the split's runner (Q-116) speaks for itself while it runs: the Macs, and
  // each one's step as its node reports it.
  const runnerUpdate =
    busy?.startsWith('run:') && distributed?.runnerUpdate?.state === 'running'
      ? distributed.runnerUpdate
      : null;

  /**
   * The split for a model the saved setup does not carry (or with nothing saved): goose detects
   * the candidate's Macs for THIS model, keeps the owner's saved config where it spans the same
   * Macs, builds its Python where a node has none, and starts — preflight included. Only a
   * genuinely missing piece stops it, named by Mac.
   */
  const startSplitFor = useCallback(
    async (way: Way): Promise<StartRefusal | null> => {
      // The plan's Macs for this split; with no plan, the Macs the saved setup spans.
      const peers = way.candidate
        ? way.candidate.key.nodes.filter((node) => node !== 'local')
        : (savedSplit?.nodes ?? []).flatMap((node) => (node.ssh ? [node.ssh] : []));
      if (peers.length === 0) return said(intl.formatMessage(i18n.splitNoPeers));
      const targets = peers
        .map((id) => macForPlacementNode(macs.macs, id))
        .filter((m): m is Mac => m != null);
      const off = targets.find((mac) => peerRefuses(mac, 'split'));
      if (off) return said(macs.offText(off, 'split'));
      const names = intl.formatList(
        way.candidate?.nodeNames ?? savedSplit?.nodes.map((n) => n.name) ?? [],
        { type: 'conjunction' }
      );
      const model = modelId.split('/').pop() || modelId;
      const say = (text: string) => setNotice({ tone: 'accent', text });

      say(intl.formatMessage(i18n.splitChecking, { nodes: names, model }));
      await touchLocalNetwork();
      const discovery = await mlxDistributedDiscover(peers, modelId);
      const config = cleanConfig(splitConfigFor(discovery, savedSplit));
      const plan = splitPlan(discovery, modelId, config);
      if ('blocker' in plan) {
        if (plan.blocker.kind === 'modelMissing') {
          for (const mac of targets) void macs.refreshModels(mac.key);
        }
        return said(splitBlockerText(intl, plan.blocker, modelId));
      }
      if (plan.provision.length > 0) {
        say(
          intl.formatMessage(i18n.splitBuilding, {
            nodes: intl.formatList(plan.provision, { type: 'conjunction' }),
          })
        );
        let provision = await mlxDistributedProvision(config);
        // Bounded by the build's own state, never a clock: it ends done or failed.
        while (provision.state === 'running') {
          await sleep(MLX_STATUS_POLL_MS);
          const status = await mlxDistributedStatus();
          if (!status.provision) break;
          provision = status.provision;
        }
        const failed = provision.nodes.find((n) => n.state === 'failed');
        if (provision.state === 'failed' || failed) {
          return said(
            intl.formatMessage(i18n.splitBuildFailed, {
              node: failed?.name ?? names,
              reason: failed?.detail || failed?.lines[failed.lines.length - 1] || refused,
            })
          );
        }
      }
      say(intl.formatMessage(i18n.splitStarting, { nodes: names, model }));
      return startRefusal(await mlxDistributedStart(config), refused);
    },
    [intl, macs, modelId, refused, savedSplit]
  );

  /** Start one way; the refusal/failure, or null when it started. */
  const startWay = useCallback(
    async (way: Way): Promise<StartRefusal | null> => {
      if (way.kind === 'local') {
        onMountHere();
        return null;
      }
      if (way.kind === 'peer') {
        if (!way.peerNodeId) return said(refused);
        const response = await mlxRemoteSingleStart(way.peerNodeId, modelId);
        return response.started ? null : said(response.refusal?.message ?? refused);
      }
      const action = way.candidate?.action;
      const setUpForThisModel =
        action?.kind === 'startSplit'
          ? action.setupMatches
          : savedSplit != null && savedSplit.modelId === modelId;
      if (!setUpForThisModel) return startSplitFor(way);
      return startRefusal(await mlxDistributedStart(null), refused);
    },
    [modelId, onMountHere, refused, savedSplit, startSplitFor]
  );

  /**
   * Stop a way of this model and return once it has let go of its memory — the ONE rule for
   * "the old way stops first", whether that way is running or still LOADING: the single engine's
   * unmount and the peer's unmount both return after the engine process exits and the Mac's load
   * lock is released (a load in flight is cancelled, Q-112); the split is followed until it no
   * longer owns the Mac — bounded by its own state, never a clock. A route is withdrawn on this
   * Mac (routeSwitch.ts): a linked Mac that is not answering, or keeps its model, is a quiet line
   * (PeerHeldLine, in the Engine view), never a switch that cannot go on — except that a switch
   * to the SPLIT, which runs on that Mac too, starts only once that Mac has answered its unmount
   * (`settled`): on 3.0.44, right after a relaunch, the split's start began on the MacBook
   * (05:53:48.94) before the Studio even received the unmount (05:53:49.11), and the preflight
   * was refused by the route's own load. Throws only when the current way could not be stopped
   * at all.
   */
  const stopForSwitch = async (way: Way, next: Way): Promise<void> => {
    if (way.kind === 'local') {
      await mlxEngineUnmount();
      return;
    }
    if (way.kind === 'peer') {
      const drop = dropRoute();
      await drop.routeGone;
      if (next.kind === 'split') await drop.settled;
      return;
    }
    let status = (await mlxDistributedStop()).status;
    while (ownsTheMac(status) && status.state !== 'failed') {
      await sleep(MLX_STATUS_POLL_MS);
      status = await mlxDistributedStatus();
    }
  };

  /**
   * Run is a SWITCH: the model runs one way at a time — chat follows one engine, and the plan
   * credits the running way's memory to the others. Starting a second copy beside the first left
   * it idle and held its memory (3.0.29: Run across both Macs read "short 5.2 GB on Work's Mac
   * Studio" while the Studio's own copy held 53 GB).
   */
  const run = async (way: Way) => {
    setNotice(null);
    // Whatever serves now stops first — the picked model on another way, or another model
    // anywhere (Q-119: Run for Flash while the Studio served the 27B).
    const current = serving;
    if (current.length === 0 && way.kind === 'local') {
      onMountHere();
      return;
    }
    setBusy(`run:${way.key}`);
    try {
      if (current.length > 0) {
        for (const stopping of current) {
          const model = shortModel(stopping.modelId);
          const where = title(stopping.way);
          setNotice({
            tone: 'accent',
            text: intl.formatMessage(i18n.switching, { model, where }),
          });
          try {
            await stopForSwitch(stopping.way, way);
          } catch (e) {
            const reason = mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed));
            setNotice({
              tone: 'err',
              text: intl.formatMessage(i18n.switchStopFailed, { model, where, reason }),
            });
            return;
          }
        }
        if (way.kind === 'local') {
          onMountHere();
          setNotice({ tone: 'accent', text: intl.formatMessage(i18n.started), follows: way.key });
          return;
        }
      }
      const refusal = await startWay(way);
      setNotice(
        refusal == null
          ? { tone: 'accent', text: intl.formatMessage(i18n.started), follows: way.key }
          : {
              tone: 'err',
              text: way.mac ? macs.describeError(way.mac, refusal.text) : refusal.text,
              detail: refusal.detail,
            }
      );
    } catch (e) {
      const text = mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed));
      setNotice({ tone: 'err', text: way.mac ? macs.describeError(way.mac, text) : text });
    } finally {
      setBusy(null);
    }
  };

  const stop = async (way: Way) => {
    if (way.kind === 'local') {
      onStopHere();
      return;
    }
    if (way.kind === 'split') {
      setConfirmStopSplit(true);
      return;
    }
    setBusy(`stop:${way.key}`);
    try {
      // Withdrawn here at once when its Mac is not answering; a kept model is PeerHeldLine's.
      await dropRoute().routeGone;
    } catch (e) {
      setNotice({ tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)) });
    } finally {
      setBusy(null);
    }
  };

  const stopSplit = async () => {
    setConfirmStopSplit(false);
    setBusy('stop:split');
    try {
      await mlxDistributedStop();
    } catch (e) {
      setNotice({ tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)) });
    } finally {
      setBusy(null);
    }
  };

  const measure = async (placementId: string) => {
    setNotice(null);
    setBusy(`measure:${placementId}`);
    try {
      const response = await mlxMeasureSpeed(modelId, placementId, goal === 'longDocuments');
      const chat = response.records.find((r) => r.workload === 'chat') ?? response.records[0];
      if (chat) {
        setNotice({
          tone: 'ok',
          text: intl.formatMessage(i18n.measuredResult, {
            decode: chat.decodeTps != null ? tps(chat.decodeTps) : '—',
            prefill: chat.prefillTps != null ? tps(chat.prefillTps) : '—',
          }),
        });
      }
      await load();
    } catch (e) {
      setNotice({ tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)) });
    } finally {
      setBusy(null);
    }
  };

  // The Mac that must receive the model before this way can start: a peer (or the split's other
  // Macs) whose models folder was READ and holds no complete copy — never guessed from a gap.
  const missingOn = (way: Way): Mac | null => {
    const targets: Mac[] =
      way.kind === 'peer'
        ? way.mac
          ? [way.mac]
          : []
        : way.kind === 'split'
          ? (way.candidate?.key.nodes ?? [])
              .map((id) => macForPlacementNode(macs.macs, id))
              .filter((m): m is Mac => m != null && !m.isSelf)
          : [];
    return (
      targets.find((mac) => {
        const models = macs.factsOf(mac.key).models;
        return models != null && !models.some((m) => m.id === modelId && m.complete);
      }) ?? null
    );
  };
  const selfModel = macs.factsOf(SELF_KEY).models?.find((m) => m.id === modelId && m.complete);

  const copyFirst = (way: Way, to: Mac) => {
    const key = copyKey(modelId, to.key);
    macs.copy(modelId, SELF_KEY, to.key);
    macs.whenCopied(key, () => {
      setBusy(`run:${way.key}`);
      void startWay(way)
        .then((refusal) =>
          setNotice(
            refusal == null
              ? { tone: 'accent', text: intl.formatMessage(i18n.started), follows: way.key }
              : {
                  tone: 'err',
                  text: macs.describeError(to, refusal.text),
                  detail: refusal.detail,
                }
          )
        )
        .catch((e: unknown) =>
          setNotice({
            tone: 'err',
            text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)),
          })
        )
        .finally(() => {
          setBusy(null);
          void load();
        });
    });
  };

  const { ways, otherSplits } = waysOf(plan, macs.macs, distributedCapability);
  const distributedOwns = ownsTheMac(distributed);
  const serving = servingWays(ways, macs.macs, single, distributed);

  /** Where a serving way runs, in the words the card's lines use. */
  const servedWhere = (way: Way): string => {
    if (way.kind === 'local') return intl.formatMessage(i18n.thisMac);
    if (way.kind === 'peer') {
      const remote = latestMlxRemoteSingleStatus();
      return way.mac?.name ?? (remote ? routePeerName(remote) : '—');
    }
    const live = distributed?.nodes.map((n) => n.name).filter(Boolean) ?? [];
    const names =
      live.length > 0
        ? live
        : (distributed?.config?.nodes.map((n) => n.name).filter(Boolean) ?? []);
    return names.length > 0
      ? intl.formatList(names, { type: 'conjunction' })
      : intl.formatMessage(i18n.yourMacs);
  };

  /** What pressing Run on `way` does first, in plain words — one line per engine it stops. */
  const stopsFirstLines = (way: Way): string[] =>
    serving.map((s) => {
      const values = { model: shortModel(s.modelId), where: servedWhere(s.way) };
      return way.candidate?.fit.afterStopping?.includes(s.modelId)
        ? intl.formatMessage(i18n.fitsOnceStops, values)
        : intl.formatMessage(i18n.stopsFirst, values);
    });

  const title = (way: Way): string => {
    if (way.kind === 'local') return intl.formatMessage(i18n.runHere);
    if (way.kind === 'peer') {
      return intl.formatMessage(i18n.runOn, {
        name: way.mac?.name ?? way.candidate?.nodeNames[0] ?? '—',
      });
    }
    const count = way.candidate?.key.nodes.length;
    return count
      ? intl.formatMessage(i18n.runAcross, { count })
      : intl.formatMessage(i18n.runAcrossAny);
  };

  const renderWay = (way: Way) => {
    const c = way.candidate;
    const live = wayLive(way, modelId, single, distributed);
    const running = live != null && live.state !== 'failed';
    const figure = c ? goalFigure(c, goal) : null;
    const action = c?.action ?? null;
    // "Best" is goose's: the planner ranks long documents by a whole turn's expected time, so a
    // split that reads faster but writes slower wins only when the turn's answer is short enough.
    const isBest = c != null && plan?.best === c.id;
    const isBestNow = c != null && plan?.bestAvailable === c.id && plan.bestAvailable !== plan.best;
    // The split beside a Mac the model fits on states what it costs and buys (Q-72) — that line
    // carries both writing figures, so goose's "Slower for this" would only repeat half of it.
    const tradeOff = c && way.kind === 'split' ? splitTradeOff(plan, c) : null;
    const why = tradeOff ? tradeOffText(intl, tradeOff) : c ? outcomeText(intl, c) : null;
    const needsCopy = missingOn(way);
    const copyJob = needsCopy ? macs.copies[copyKey(modelId, needsCopy.key)] : undefined;
    const copyLink = needsCopy ? macs.linkBetween(SELF_KEY, needsCopy.key) : null;
    const blockedByDistributed = way.kind === 'local' && distributedOwns;
    // A way goose judged short (or could not judge) is not offered: the start would be refused.
    const fitsForGoose =
      c == null || (c.supported && c.fit.status !== 'short' && c.fit.status !== 'unknown');
    const startable =
      !running &&
      !blockedByDistributed &&
      needsCopy == null &&
      fitsForGoose &&
      (action == null || action.kind !== 'unavailable');
    const placementId = c?.id ?? (way.kind === 'local' ? 'single:local' : null);
    const measuring = busy === `measure:${placementId}`;
    const rate = needsCopy ? macs.copyRate(SELF_KEY, needsCopy.key) : null;
    const minutes = rate && selfModel ? minutesAt(selfModel.sizeBytes, rate) : 0;

    return (
      <li
        key={way.key}
        data-testid={`placement-way-${way.kind}`}
        data-way={way.key}
        className={cx('flex flex-col gap-2 p-3', SURFACE.inset, RADIUS.card)}
      >
        <div className="flex flex-wrap items-center gap-2">
          <span className={TYPE.h2}>{title(way)}</span>
          {isBest && <Chip tone="accent">{intl.formatMessage(i18n.best)}</Chip>}
          {isBestNow && <Chip tone="ok">{intl.formatMessage(i18n.bestNow)}</Chip>}
          {c && way.kind === 'split' && (
            <Chip>
              {intl.formatMessage(c.key.kind === 'tensor' ? i18n.tensor : i18n.pipeline, {
                link: linkWord(c),
              })}
            </Chip>
          )}
          {live && <LiveChip live={live} />}
        </div>
        {c && (
          <div className="flex flex-wrap items-center gap-2">
            {figure && (
              <>
                <span className={cx('text-lz-body', WEIGHT.semibold, TNUM, TONE_TEXT.accent)}>
                  {figureText(intl, goal, figure, c)}
                </span>
                <FigureRange figure={figure} />
                <SourceChip figure={figure} />
              </>
            )}
            {c.fit.context != null && (
              <Chip>
                {intl.formatMessage(
                  c.fit.status === 'smallerContext' ? i18n.smallerContext : i18n.context,
                  { tokens: c.fit.context.toLocaleString() }
                )}
              </Chip>
            )}
          </div>
        )}
        {why && (
          <p className={cx('break-words', TYPE.meta)} title={c?.fit.detail}>
            {why}
          </p>
        )}
        {startable &&
          stopsFirstLines(way).map((line) => (
            <p
              key={line}
              data-testid={`placement-stops-first-${way.kind}`}
              className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}
            >
              {line}
            </p>
          ))}
        {way.kind === 'split' &&
          running &&
          distributed?.contextLimit != null &&
          splitContextFromFreeMemory(distributed) && (
            <p data-testid="placement-split-context" className={cx('break-words', TYPE.meta)}>
              {intl.formatMessage(i18n.splitContextFixed, {
                tokens: distributed.contextLimit.toLocaleString(),
              })}
            </p>
          )}
        <div className="flex flex-wrap items-center gap-2">
          {startable && (
            <Button
              variant={isBest || (!plan && way.kind === 'local') ? 'primary' : 'secondary'}
              icon={busy === `run:${way.key}` ? <Loader2 className="animate-spin" /> : <Play />}
              disabled={busy != null || (way.kind === 'local' && mountBusy)}
              onClick={() => void run(way)}
              data-testid={`placement-run-${way.kind}`}
            >
              {busy === `run:${way.key}`
                ? intl.formatMessage(i18n.starting)
                : intl.formatMessage(i18n.run)}
            </Button>
          )}
          {running && (
            <Button
              variant="secondary"
              icon={busy === `stop:${way.key}` ? <Loader2 className="animate-spin" /> : <Square />}
              disabled={busy != null}
              onClick={() => void stop(way)}
              data-testid={`placement-stop-${way.kind}`}
            >
              {intl.formatMessage(i18n.stop)}
            </Button>
          )}
          {running && placementId && live?.state !== 'mounting' && (
            <Button
              variant={figure?.measured ? 'secondary' : 'primary'}
              icon={measuring ? <Loader2 className="animate-spin" /> : <Gauge />}
              disabled={busy != null}
              onClick={() => void measure(placementId)}
              data-testid={`placement-measure-${way.kind}`}
            >
              {intl.formatMessage(i18n.measure)}
            </Button>
          )}
          {needsCopy && !running && !(copyJob && copyRunning(copyJob)) && selfModel && copyLink && (
            <Button
              variant="primary"
              icon={copyLink.kind === 'thunderbolt' ? <Zap /> : <Network />}
              disabled={busy != null}
              onClick={() => copyFirst(way, needsCopy)}
              data-testid={`placement-copy-first-${way.kind}`}
            >
              {minutes > 0
                ? intl.formatMessage(i18n.copyFirstMinutes, {
                    name: needsCopy.name,
                    minutes,
                    kind: copyLink.kind,
                  })
                : intl.formatMessage(i18n.copyFirst, { name: needsCopy.name, kind: copyLink.kind })}
            </Button>
          )}
        </div>
        {measuring && <p className={TYPE.meta}>{intl.formatMessage(i18n.measuring)}</p>}
        {running && figure && !figure.measured && !measuring && (
          <p className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.measureFirst)}</p>
        )}
        {needsCopy && selfModel && !(copyJob && copyRunning(copyJob)) && !copyJob?.error && (
          <p className={TYPE.meta}>
            {intl.formatMessage(i18n.copyFirstWhy, {
              model: modelId,
              size: formatGb(selfModel.sizeBytes),
              name: needsCopy.name,
            })}
          </p>
        )}
        {copyJob && copyRunning(copyJob) && needsCopy && (
          <div className="flex flex-col gap-1" data-testid={`placement-copying-${way.kind}`}>
            <span className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
              {intl.formatMessage(i18n.copyingThen, {
                name: needsCopy.name,
                pct:
                  copyJob.progress && copyJob.progress.totalBytes > 0
                    ? Math.round((copyJob.progress.copiedBytes / copyJob.progress.totalBytes) * 100)
                    : 0,
              })}
            </span>
            <div className={cx('h-1.5 w-full overflow-hidden', RADIUS.pill, SURFACE.card)}>
              <div
                className={cx('h-full', TONE_DOT.accent)}
                style={{
                  width: `${
                    copyJob.progress && copyJob.progress.totalBytes > 0
                      ? Math.min(
                          100,
                          (copyJob.progress.copiedBytes / copyJob.progress.totalBytes) * 100
                        )
                      : 0
                  }%`,
                }}
              />
            </div>
          </div>
        )}
        {copyJob &&
          !copyRunning(copyJob) &&
          (copyJob.error || copyJob.progress?.state === 'failed') &&
          needsCopy && (
            <p className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}>
              {intl.formatMessage(i18n.copyThenFailed, {
                name: needsCopy.name,
                reason: copyJob.error ?? copyJob.progress?.error ?? '—',
              })}
            </p>
          )}
        {blockedByDistributed && !running && (
          <p className={cx('break-words', TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.distributedOwns)}
          </p>
        )}
        {action?.kind === 'unavailable' && !needsCopy && (
          <p className={cx('break-words', TYPE.meta)}>
            {way.mac ? macs.describeError(way.mac, action.reason) : action.reason}
          </p>
        )}
        {way.kind === 'split' && splitDetails && (
          <Disclosure
            variant="plain"
            title={intl.formatMessage(i18n.details)}
            meta={<span className={TYPE.meta}>{intl.formatMessage(i18n.detailsMeta)}</span>}
            open={detailsOpen}
            onOpenChange={setDetailsOpen}
            testId="placement-split-details"
          >
            {splitDetails}
          </Disclosure>
        )}
      </li>
    );
  };

  return (
    <section
      aria-label={intl.formatMessage(i18n.title)}
      data-testid="placement-card"
      className={cx('flex flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className={TYPE.zone}>{intl.formatMessage(i18n.title)}</span>
        <div className="flex items-center gap-2">
          <Segmented<PlacementGoal>
            size="sm"
            aria-label={intl.formatMessage(i18n.goalLabel)}
            options={[
              { value: 'chat', label: intl.formatMessage(i18n.goalChat) },
              { value: 'longDocuments', label: intl.formatMessage(i18n.goalLong) },
              { value: 'manyRequests', label: intl.formatMessage(i18n.goalMany) },
            ]}
            value={goal}
            onChange={setGoal}
          />
          <Button
            size="sm"
            variant="ghost"
            iconOnly
            icon={loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            aria-label={intl.formatMessage(i18n.refresh)}
            title={intl.formatMessage(i18n.refresh)}
            disabled={loading}
            onClick={() => void load()}
          />
        </div>
      </div>
      {loading && !plan && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.planning)}</p>}
      {error && <ToneBanner tone="err" label={intl.formatMessage(i18n.planFailed)} text={error} />}
      {plan?.error && (
        <ToneBanner tone="err" label={intl.formatMessage(i18n.planFailed)} text={plan.error} />
      )}
      {(error || plan?.error) && <p className={TYPE.meta}>{intl.formatMessage(i18n.noPlanWays)}</p>}
      {runnerUpdate ? (
        <RunnerUpdateNotice update={runnerUpdate} />
      ) : (
        notice && (
          <>
            <ToneBanner
              tone={notice.tone}
              label={intl.formatMessage(i18n.title)}
              text={notice.text}
            />
            {notice.detail && (
              <Disclosure
                variant="plain"
                title={intl.formatMessage(i18n.noticeDetails)}
                testId="placement-notice-detail"
              >
                <p className={cx('whitespace-pre-wrap break-all', TYPE.mono)}>{notice.detail}</p>
              </Disclosure>
            )}
          </>
        )
      )}
      {storeErrors.length > 0 && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.storeErrors)}
          text={storeErrors.join('; ')}
        />
      )}
      {!(loading && !plan && !error) && (
        <ul className="flex flex-col gap-2" data-testid="placement-ways">
          {ways.map(renderWay)}
        </ul>
      )}
      {(plan?.notes ?? []).map((note) => (
        <p key={note} className={TYPE.meta}>
          {note}
        </p>
      ))}
      {otherSplits.length > 0 && (
        <Disclosure
          title={intl.formatMessage(i18n.others, { count: otherSplits.length })}
          open={othersOpen}
          onOpenChange={setOthersOpen}
          testId="placement-others"
        >
          <ul className="flex flex-col gap-2">
            {otherSplits.map((c) => (
              <li
                key={c.id}
                className="flex flex-col gap-0.5"
                data-testid={`placement-other-${c.id}`}
              >
                <span className={cx(TYPE.body, WEIGHT.semibold)}>
                  {intl.formatMessage(c.key.kind === 'tensor' ? i18n.tensor : i18n.pipeline, {
                    link: linkWord(c),
                  })}
                </span>
                <span className={cx('break-words', TYPE.meta)} title={c.fit.detail}>
                  {outcomeText(intl, c)}
                </span>
              </li>
            ))}
          </ul>
        </Disclosure>
      )}
      <ConfirmationModal
        isOpen={confirmStopSplit}
        title={intl.formatMessage(i18n.stopSplitTitle)}
        message={intl.formatMessage(i18n.stopSplitMessage, {
          nodes: distributed?.nodes.map((n) => n.name).join(', ') || '—',
        })}
        confirmLabel={intl.formatMessage(i18n.stop)}
        cancelLabel={intl.formatMessage(i18n.keepRunning)}
        confirmVariant="destructive"
        onConfirm={() => void stopSplit()}
        onCancel={() => setConfirmStopSplit(false)}
      />
    </section>
  );
}

export function PlacementCard(props: PlacementCardProps) {
  return (
    <WithMacs>
      <PlacementCardBody {...props} />
    </WithMacs>
  );
}

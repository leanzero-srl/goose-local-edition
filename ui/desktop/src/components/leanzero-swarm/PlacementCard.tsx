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
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStart,
  mlxRemoteSingleStatus,
  mlxRemoteSingleStop,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { mlxErrorMessage } from './mlxErrorMessage';
import {
  cleanConfig,
  ownsTheMac,
  splitConfigFor,
  splitPlan,
  type SplitBlocker,
} from './mlxDistributed';
import { MLX_STATUS_POLL_MS } from './mlxLiveStats';
import { touchLocalNetwork } from './LocalNetworkNotice';
import { distributedStateWord } from './mlxModeLabel';
import { remotePhase, runPhase, singlePhase } from './mlxPhase';
import { formatGb } from './primitives';
import { macForPlacementNode, minutesAt, peerRefuses, SELF_KEY, type Mac } from './macs';
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
  total: {
    id: 'placementCard.total',
    defaultMessage: '~{value} tok/s total at {concurrency} at once',
  },
  range: { id: 'placementCard.range', defaultMessage: '{low}–{high}' },
  context: { id: 'placementCard.context', defaultMessage: '{tokens} context' },
  measured: {
    id: 'placementCard.measured',
    defaultMessage: '{runs, plural, one {measured} other {measured · # runs}}',
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
  badgeBoth: { id: 'placementCard.badgeBoth', defaultMessage: 'Needs both Macs' },
  badgeTooBig: { id: 'placementCard.badgeTooBig', defaultMessage: 'Too big, short {gb}' },
  badgeUnknown: { id: 'placementCard.badgeUnknown', defaultMessage: 'Fit unknown' },
  actionFailed: { id: 'placementCard.actionFailed', defaultMessage: 'The action failed' },
  refusedUnnamed: {
    id: 'placementCard.refusedUnnamed',
    defaultMessage: 'Refused, and goose named no reason',
  },
  liveMounting: { id: 'placementCard.live.mounting', defaultMessage: 'Mounting' },
  liveRunning: { id: 'placementCard.live.running', defaultMessage: 'Running' },
  liveFailed: { id: 'placementCard.live.failed', defaultMessage: 'Failed' },
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
});

type NoticeTone = Exclude<Tone, 'secondary'>;

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

function linkWord(candidate: PlacementCandidate): string {
  return candidate.key.link === 'jaccl' ? 'JACCL' : (candidate.key.link ?? '');
}

function figureText(
  intl: IntlShape,
  goal: PlacementGoal,
  figure: SpeedFigure,
  concurrency: number | null | undefined
): string {
  const value = tps(figure.estimate.value);
  if (goal === 'longDocuments') return intl.formatMessage(i18n.reads, { value });
  if (goal === 'manyRequests')
    return intl.formatMessage(i18n.total, { value, concurrency: concurrency ?? '—' });
  return intl.formatMessage(i18n.writes, { value });
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

/** The model picker's badge: where this model fits, measured just now. */
export function PlacementBadge({ badge }: { badge: PlacementBadgeDto }) {
  const intl = useIntl();
  const text =
    badge.kind === 'fitsThisMac'
      ? intl.formatMessage(i18n.badgeThisMac)
      : badge.kind === 'fitsPeer'
        ? intl.formatMessage(i18n.badgePeer, { name: badge.name })
        : badge.kind === 'needsBothMacs'
          ? intl.formatMessage(i18n.badgeBoth)
          : badge.kind === 'tooBig'
            ? intl.formatMessage(i18n.badgeTooBig, { gb: gb(badge.shortBytes) })
            : intl.formatMessage(i18n.badgeUnknown);
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
export function badgesOf(plans: Map<string, PlacementPlan>): Map<string, PlacementBadgeDto> {
  const out = new Map<string, PlacementBadgeDto>();
  for (const [id, plan] of plans) if (plan.badge) out.set(id, plan.badge);
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

/** The engine a way IS right now, in the engine-phase palette; null = not this way. */
export function wayLive(
  way: Way,
  modelId: string,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null
): { phase: EnginePhase; state: string } | null {
  if (way.kind === 'local') {
    if (single?.modelId !== modelId) return null;
    if (single.state !== 'mounting' && single.state !== 'running' && single.state !== 'failed') {
      return null;
    }
    return { phase: singlePhase(single.state, false, null), state: single.state };
  }
  if (way.kind === 'peer') {
    const remote = latestMlxRemoteSingleStatus();
    if (!way.peerNodeId || remote?.peer !== way.peerNodeId || remote.modelId !== modelId)
      return null;
    if (remote.state === 'off') return null;
    return {
      phase: remotePhase(remote.state, null),
      state: remote.state === 'ready' ? 'running' : remote.state,
    };
  }
  if (!distributed || distributed.modelId !== modelId) return null;
  if (!ownsTheMac(distributed) && distributed.state !== 'failed') return null;
  return {
    phase: runPhase(distributed.state, distributed.admissionOpen),
    state: distributed.state,
  };
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
  const [notice, setNotice] = useState<{ tone: NoticeTone; text: string; follows?: string } | null>(
    null
  );
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

  /**
   * The split for a model the saved setup does not carry (or with nothing saved): goose detects
   * the candidate's Macs for THIS model, keeps the owner's saved config where it spans the same
   * Macs, builds its Python where a node has none, and starts — preflight included. Only a
   * genuinely missing piece stops it, named by Mac.
   */
  const startSplitFor = useCallback(
    async (way: Way): Promise<string | null> => {
      // The plan's Macs for this split; with no plan, the Macs the saved setup spans.
      const peers = way.candidate
        ? way.candidate.key.nodes.filter((node) => node !== 'local')
        : (savedSplit?.nodes ?? []).flatMap((node) => (node.ssh ? [node.ssh] : []));
      if (peers.length === 0) return intl.formatMessage(i18n.splitNoPeers);
      const targets = peers
        .map((id) => macForPlacementNode(macs.macs, id))
        .filter((m): m is Mac => m != null);
      const off = targets.find((mac) => peerRefuses(mac, 'split'));
      if (off) return macs.offText(off, 'split');
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
        return splitBlockerText(intl, plan.blocker, modelId);
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
          return intl.formatMessage(i18n.splitBuildFailed, {
            node: failed?.name ?? names,
            reason: failed?.detail || failed?.lines[failed.lines.length - 1] || refused,
          });
        }
      }
      say(intl.formatMessage(i18n.splitStarting, { nodes: names, model }));
      const response = await mlxDistributedStart(config);
      return response.started ? null : (response.refusal?.message ?? refused);
    },
    [intl, macs, modelId, refused, savedSplit]
  );

  /** Start one way; the refusal/failure text, or null when it started. */
  const startWay = useCallback(
    async (way: Way): Promise<string | null> => {
      if (way.kind === 'local') {
        onMountHere();
        return null;
      }
      if (way.kind === 'peer') {
        if (!way.peerNodeId) return refused;
        const response = await mlxRemoteSingleStart(way.peerNodeId, modelId);
        return response.started ? null : (response.refusal?.message ?? refused);
      }
      const action = way.candidate?.action;
      const setUpForThisModel =
        action?.kind === 'startSplit'
          ? action.setupMatches
          : savedSplit != null && savedSplit.modelId === modelId;
      if (!setUpForThisModel) return startSplitFor(way);
      const response = await mlxDistributedStart(null);
      return response.started ? null : (response.refusal?.message ?? refused);
    },
    [modelId, onMountHere, refused, savedSplit, startSplitFor]
  );

  /**
   * Stop a way of this model and return once it has let go of its memory: the single engine's
   * unmount and the peer's unmount both return after the engine process exits; the split is
   * followed until it no longer owns the Mac — bounded by its own state, never a clock.
   * The failure text, or null.
   */
  const stopForSwitch = async (way: Way): Promise<string | null> => {
    if (way.kind === 'local') {
      await mlxEngineUnmount();
      return null;
    }
    if (way.kind === 'peer') {
      const response = await mlxRemoteSingleStop(false);
      return response.unmountError ?? null;
    }
    let status = (await mlxDistributedStop()).status;
    while (ownsTheMac(status) && status.state !== 'failed') {
      await sleep(MLX_STATUS_POLL_MS);
      status = await mlxDistributedStatus();
    }
    return null;
  };

  /**
   * Run is a SWITCH: the model runs one way at a time — chat follows one engine, and the plan
   * credits the running way's memory to the others. Starting a second copy beside the first left
   * it idle and held its memory (3.0.29: Run across both Macs read "short 5.2 GB on Work's Mac
   * Studio" while the Studio's own copy held 53 GB).
   */
  const run = async (way: Way) => {
    setNotice(null);
    const current = ways.find((w) => {
      if (w.key === way.key) return false;
      const live = wayLive(w, modelId, single, distributed);
      return live != null && live.state !== 'failed';
    });
    if (!current && way.kind === 'local') {
      onMountHere();
      return;
    }
    setBusy(`run:${way.key}`);
    try {
      if (current) {
        const model = modelId.split('/').pop() || modelId;
        const where = title(current);
        setNotice({ tone: 'accent', text: intl.formatMessage(i18n.switching, { model, where }) });
        const failed = await stopForSwitch(current);
        if (failed) {
          setNotice({
            tone: 'err',
            text: intl.formatMessage(i18n.switchStopFailed, { model, where, reason: failed }),
          });
          return;
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
          : { tone: 'err', text: way.mac ? macs.describeError(way.mac, refusal) : refusal }
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
      await mlxRemoteSingleStop(false);
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
              : { tone: 'err', text: macs.describeError(to, refusal) }
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
    const isBest = c != null && plan?.best === c.id;
    const isBestNow = c != null && plan?.bestAvailable === c.id && plan.bestAvailable !== plan.best;
    const why = c ? outcomeText(intl, c) : null;
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
                  {figureText(intl, goal, figure, c.speed.concurrency)}
                </span>
                <span className={cx(TYPE.meta, TNUM)}>
                  {intl.formatMessage(i18n.range, {
                    low: tps(figure.estimate.low),
                    high: tps(figure.estimate.high),
                  })}
                </span>
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
      {notice && (
        <ToneBanner tone={notice.tone} label={intl.formatMessage(i18n.title)} text={notice.text} />
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

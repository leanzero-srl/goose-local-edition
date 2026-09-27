import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  BookmarkPlus,
  Gauge,
  Loader2,
  Network,
  Play,
  RefreshCw,
  Square,
  Zap,
} from 'lucide-react';
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
import { cutMessage, useCutGuard, useInFlightWork } from './cutGuard';
import type { MlxEngineKind } from '../../utils/mlxInFlight';
import { ToneBanner } from './studio';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import {
  goalFigure,
  mlxMeasureSpeed,
  mlxPlacementPlan,
  type PlacementBadge as PlacementBadgeDto,
  type PlacementCandidate,
  type PlacementGoal,
  type PlacementPlan,
} from '../../acp/mlx-placement';
import { nodesRead } from '../../acp/nodes';
import { nodeHref } from '../../utils/navigationUtils';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import { THIS_MAC } from '../nodes/model';
import {
  defaultNodeName,
  mlxDef,
  nodeForWay,
  nodeIdFor,
  placementOfCandidate,
  putNode,
  whereWords,
  type PinnedPlacement,
} from '../nodes/nodeDraft';
import {
  OtherSplits,
  PlacementCandidates,
  gb,
  shortModel,
  tps,
  wayPickable,
  wayTitle,
  waysOf,
  type Way,
  type WayRowFacts,
} from './PlacementCandidates';
import type { PlacementPlansRead } from './usePlacementPlans';
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
import { MLX_STATUS_POLL_MS, type MlxActivity } from './mlxLiveStats';
import { touchLocalNetwork } from './LocalNetworkNotice';
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
  refresh: { id: 'placementCard.refresh', defaultMessage: 'Plan again' },
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
  stopSplitAction: { id: 'placementCard.stopSplitAction', defaultMessage: 'Stop the split' },
  stopRouteAction: {
    id: 'placementCard.stopRouteAction',
    defaultMessage: 'Stop serving from {name}',
  },
  tooSmallForLive: {
    id: 'placementCard.tooSmallForLive',
    defaultMessage:
      'Its {context} context is under the {live} tokens the conversation being answered holds now.',
  },
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
  saveAsNode: { id: 'placementCard.saveAsNode', defaultMessage: 'Save as node' },
  saveOffer: {
    id: 'placementCard.saveOffer',
    defaultMessage: 'Save this way as a node so chats and builds can pick it',
  },
  savedAs: { id: 'placementCard.savedAs', defaultMessage: 'Saved as the node “{name}”.' },
  alreadyNode: {
    id: 'placementCard.alreadyNode',
    defaultMessage: 'Already a node: “{name}”.',
  },
  openInNodes: { id: 'placementCard.openInNodes', defaultMessage: 'Open in Nodes' },
  saveRefused: { id: 'placementCard.saveRefused', defaultMessage: 'Not saved: {reason}' },
  saveNoPlan: {
    id: 'placementCard.saveNoPlan',
    defaultMessage: 'goose has no plan for this way, so it cannot say which Macs it runs on.',
  },
  saveNothingRunning: {
    id: 'placementCard.saveNothingRunning',
    defaultMessage: '{model} is not running on any way shown here — pick the running model above.',
  },
});

type NoticeTone = Exclude<Tone, 'secondary'>;

/** "Save as node" on one row: in flight, saved (or already a node), or refused with goose's words. */
type SaveState =
  | { kind: 'saving' }
  | { kind: 'saved'; id: string; name: string; already: boolean }
  | { kind: 'refused'; text: string };

/**
 * The way a row names, as a node stores it: the planner's key, or — with no plan — this Mac's
 * single or the peer's single by its Link id (a split with no plan names no Macs: not saveable).
 */
function pinnedPlacementOf(way: Way): PinnedPlacement | null {
  if (way.candidate) return placementOfCandidate(way.candidate);
  if (way.kind === 'local') return { kind: 'single', macs: [THIS_MAC] };
  if (way.kind === 'peer' && way.peerNodeId) {
    return { kind: 'single', macs: [`link:${way.peerNodeId}`] };
  }
  return null;
}

function SaveAsNodeButton({
  variant,
  saving,
  onClick,
  testId,
}: {
  variant: 'ghost' | 'primary';
  saving: boolean;
  onClick: () => void;
  testId: string;
}) {
  const intl = useIntl();
  return (
    <Button
      variant={variant}
      size="sm"
      icon={saving ? <Loader2 className="animate-spin" /> : <BookmarkPlus />}
      disabled={saving}
      onClick={onClick}
      data-testid={testId}
    >
      {intl.formatMessage(i18n.saveAsNode)}
    </Button>
  );
}

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
 * The picker's badge per model, from a plans read. While the read is in flight, or after it failed,
 * no model carries a badge: the picker never shows a guessed fit, and the Run it card under the
 * picker reads the same planner for the picked model and names a failure in goose's words.
 */
export function badgesOf(read: PlacementPlansRead): Map<string, PickerBadge> {
  const out = new Map<string, PickerBadge>();
  if (read.kind !== 'read') return out;
  for (const [id, plan] of read.plans) {
    const badge = pickerBadgeOf(plan);
    if (badge) out.set(id, badge);
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// The ways (the list itself is PlacementCandidates.tsx; what serves and what a switch stops is here)
// ---------------------------------------------------------------------------------------------

export { waysOf, splitTradeOff, type Way, type SplitTradeOff } from './PlacementCandidates';

/** The live read of the engine that answers chat (the state tile's read), and which engine it is. */
export interface WayActivity {
  engine: MlxEngineKind;
  activity: MlxActivity;
}

/**
 * The engine a way IS right now — whichever model it holds — in the engine-phase palette. The way
 * whose engine the live read came from takes its activity, exactly as the state tile does: a
 * writing engine is green here too, never the idle grey (Q-26).
 */
export function wayServing(
  way: Way,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  liveActivity: WayActivity | null = null
): { phase: EnginePhase; state: string; modelId: string } | null {
  const activity =
    liveActivity && liveActivity.engine === engineOfWay(way) ? liveActivity.activity : null;
  if (way.kind === 'local') {
    if (!single?.modelId) return null;
    if (single.state !== 'mounting' && single.state !== 'running' && single.state !== 'failed') {
      return null;
    }
    return {
      phase: singlePhase(single.state, false, activity),
      state: single.state,
      modelId: single.modelId,
    };
  }
  if (way.kind === 'peer') {
    const remote = latestMlxRemoteSingleStatus();
    if (!way.peerNodeId || remote?.peer !== way.peerNodeId || !remote.modelId) return null;
    if (remote.state === 'off') return null;
    return {
      phase: remotePhase(remote.state, activity),
      state: remote.state === 'ready' ? 'running' : remote.state,
      modelId: remote.modelId,
    };
  }
  if (!distributed?.modelId) return null;
  if (!ownsTheMac(distributed) && distributed.state !== 'failed') return null;
  return {
    phase: runPhase(distributed.state, distributed.admissionOpen, activity),
    state: distributed.state,
    modelId: distributed.modelId,
  };
}

/** The engine a way IS right now for `modelId`; null = not this way, or another model. */
export function wayLive(
  way: Way,
  modelId: string,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  liveActivity: WayActivity | null = null
): { phase: EnginePhase; state: string } | null {
  const serving = wayServing(way, single, distributed, liveActivity);
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

/** The engine a way runs on, in main's read's words — what a stop of that way cuts. */
export function engineOfWay(way: Way): MlxEngineKind {
  return way.kind === 'local' ? 'single' : way.kind === 'peer' ? 'remote' : 'distributed';
}

/**
 * A way whose context is under what the live conversation already holds cannot carry it: never
 * "Best" while that conversation is being answered (Q-148: "Best" fit only at 45,083 context while
 * the answer being written held ~64k). A way that serves now holds it already.
 */
export function tooSmallForLive(
  candidate: PlacementCandidate | null,
  liveContextTokens: number | null,
  servesNow: boolean
): boolean {
  if (servesNow || candidate?.fit.context == null || liveContextTokens == null) return false;
  return candidate.fit.context < liveContextTokens;
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
  /** The state tile's live read and the engine it came from: the running way's chip colour. */
  liveActivity?: WayActivity | null;
  /** The setup strip's "Save as a node" asked to save the way that runs now; handled once. */
  saveRunningPending?: boolean;
  onSaveRunningHandled?: () => void;
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
  liveActivity = null,
  saveRunningPending = false,
  onSaveRunningHandled,
}: PlacementCardProps) {
  const intl = useIntl();
  const macs = useMacs();
  const [goal, setGoal] = useState<PlacementGoal>('chat');
  const [plan, setPlan] = useState<PlacementPlan | null>(null);
  const [storeErrors, setStoreErrors] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  // Ways a start in THIS card launched: once running, that row asks to keep the way as a node.
  const [startedHere, setStartedHere] = useState<ReadonlySet<string>>(() => new Set());
  const markStarted = useCallback(
    (key: string) => setStartedHere((before) => new Set(before).add(key)),
    []
  );
  const [saves, setSaves] = useState<Record<string, SaveState>>({});
  // `follows`: the way a "Starting — this card follows it." notice speaks for; it ends when that way
  // serves or fails (it stayed 25 min after a switch — Q-36).
  // `detail`: what stands behind a refusal (a node's output, pids) — under Details, never inline.
  const [notice, setNotice] = useState<{
    tone: NoticeTone;
    text: string;
    follows?: string;
    detail?: string | null;
  } | null>(null);
  const [detailsOpen, setDetailsOpenState] = useState(splitDetailsOpenAtFirst);
  const setDetailsOpen = useCallback((open: boolean) => {
    sessionStorage.setItem(SPLIT_DETAILS_KEY, open ? 'open' : 'folded');
    setDetailsOpenState(open);
  }, []);
  const { guard, dialog: cutDialog } = useCutGuard();
  const liveWork = useInFlightWork();
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
  const run = (way: Way) => {
    // Whatever serves now stops first; while it holds work, the person is asked first (Q-148).
    const current = serving;
    if (current.length === 0) {
      void switchTo(way, current);
      return;
    }
    void guard(
      current.map((s) => engineOfWay(s.way)),
      title(way),
      () => void switchTo(way, current)
    );
  };

  const switchTo = async (way: Way, current: ServingWay[]) => {
    setNotice(null);
    // Whatever serves now stops first — the picked model on another way, or another model
    // anywhere (Q-119: Run for Flash while the Studio served the 27B).
    if (current.length === 0 && way.kind === 'local') {
      onMountHere();
      markStarted(way.key);
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
          markStarted(way.key);
          setNotice({ tone: 'accent', text: intl.formatMessage(i18n.started), follows: way.key });
          return;
        }
      }
      const refusal = await startWay(way);
      if (refusal == null) markStarted(way.key);
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

  const stop = (way: Way) => {
    if (way.kind === 'local') {
      // The view's Unmount, which asks first while the engine holds work.
      onStopHere();
      return;
    }
    if (way.kind === 'split') {
      void guard(
        ['distributed'],
        intl.formatMessage(i18n.stopSplitAction),
        () => void stopSplit(),
        {
          title: intl.formatMessage(i18n.stopSplitTitle),
          message: intl.formatMessage(i18n.stopSplitMessage, {
            nodes: distributed?.nodes.map((n) => n.name).join(', ') || '—',
          }),
          cancel: intl.formatMessage(i18n.keepRunning),
        }
      );
      return;
    }
    void guard(
      ['remote'],
      intl.formatMessage(i18n.stopRouteAction, { name: servedWhere(way) }),
      () => void stopRoute(way)
    );
  };

  const stopRoute = async (way: Way) => {
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

  /**
   * "Save as node": one def for this model on this way, through `nodes/write` (the only door to the
   * `nodes` key). A node that already names the model and the way is shown, never duplicated; a
   * refusal is goose's words.
   */
  const saveAsNode = async (way: Way) => {
    const say = (state: SaveState) => setSaves((before) => ({ ...before, [way.key]: state }));
    const placement = pinnedPlacementOf(way);
    if (!placement) {
      say({ kind: 'refused', text: intl.formatMessage(i18n.saveNoPlan) });
      return;
    }
    say({ kind: 'saving' });
    try {
      const read = await nodesRead();
      const existing = nodeForWay(read.nodes, modelId, placement);
      if (existing) {
        say({ kind: 'saved', id: existing.def.id, name: existing.def.name, already: true });
        return;
      }
      const name = defaultNodeName(
        intl,
        modelId,
        whereWords(intl, placement, macs.macs),
        read.nodes.map((n) => n.def.name)
      );
      const id = nodeIdFor(
        name,
        read.nodes.map((n) => n.def.id)
      );
      const def = mlxDef(id, {
        name,
        model: modelId,
        placement,
        goal,
        keepLoaded: false,
        origin: 'runIt',
      });
      const response = await putNode(def, read.config);
      // The engine card and the setup strip name the node at once, not at the next glance event.
      if (response.written) refreshGlanceNodes();
      say(
        response.written
          ? { kind: 'saved', id, name, already: false }
          : {
              kind: 'refused',
              text: (response.refusals ?? []).map((r) => r.message).join('; ') || refused,
            }
      );
    } catch (e) {
      say({ kind: 'refused', text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)) });
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
        .then((refusal) => {
          if (refusal == null) markStarted(way.key);
          setNotice(
            refusal == null
              ? { tone: 'accent', text: intl.formatMessage(i18n.started), follows: way.key }
              : {
                  tone: 'err',
                  text: macs.describeError(to, refusal.text),
                  detail: refusal.detail,
                }
          );
        })
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
  const serving = servingWays(ways, macs.macs, single, distributed);

  // The setup strip's "Save as a node" saves the way that runs this card's model; a model the card
  // does not show running is said, never a silent no-op.
  const saveRunning = () => {
    const runningWay = ways.find((w) => {
      const live = wayLive(w, modelId, single, distributed, liveActivity);
      return live != null && live.state !== 'failed';
    });
    if (runningWay) void saveAsNode(runningWay);
    else {
      setNotice({
        tone: 'err',
        text: intl.formatMessage(i18n.saveNothingRunning, { model: shortModel(modelId) }),
      });
    }
  };
  const saveRunningNow = useRef(saveRunning);
  saveRunningNow.current = saveRunning;
  // Handled once per ask, and only once the plan read has settled: a split's Macs come from its
  // plan row.
  const planSettled = !loading && (plan != null || error != null);
  const saveAsked = useRef(false);
  useEffect(() => {
    if (!saveRunningPending) {
      saveAsked.current = false;
      return;
    }
    if (saveAsked.current || !planSettled) return;
    saveAsked.current = true;
    onSaveRunningHandled?.();
    saveRunningNow.current();
  }, [saveRunningPending, planSettled, onSaveRunningHandled]);

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

  const title = (way: Way): string => wayTitle(intl, way);

  const rowFacts = (way: Way): WayRowFacts => {
    const servesNow = serving.some((s) => s.way.key === way.key);
    return {
      live: wayLive(way, modelId, single, distributed, liveActivity),
      tooSmall: tooSmallForLive(way.candidate, liveWork?.contextTokens ?? null, servesNow),
    };
  };

  const renderBelow = (way: Way) => {
    const c = way.candidate;
    const { live, tooSmall } = rowFacts(way);
    const running = live != null && live.state !== 'failed';
    const figure = c ? goalFigure(c, goal) : null;
    const action = c?.action ?? null;
    const isBest = c != null && plan?.best === c.id && !tooSmall;
    const needsCopy = missingOn(way);
    const copyJob = needsCopy ? macs.copies[copyKey(modelId, needsCopy.key)] : undefined;
    const copyLink = needsCopy ? macs.linkBetween(SELF_KEY, needsCopy.key) : null;
    // A way goose judged short (or could not judge) is not offered: the start would be refused.
    // While the split owns this Mac, Run on this Mac is a switch like every other way: it stops the
    // split first (servingWays lists it) and its stops-first line says so (Q-28).
    const fitsForGoose =
      c == null || (c.supported && c.fit.status !== 'short' && c.fit.status !== 'unknown');
    const startable =
      !running &&
      needsCopy == null &&
      fitsForGoose &&
      (action == null || action.kind !== 'unavailable');
    // Run on this way cuts the work the serving engines hold now: said before any click.
    const cutsLive =
      startable && liveWork != null && serving.some((s) => engineOfWay(s.way) === liveWork.engine)
        ? liveWork
        : null;
    const placementId = c?.id ?? (way.kind === 'local' ? 'single:local' : null);
    const measuring = busy === `measure:${placementId}`;
    const rate = needsCopy ? macs.copyRate(SELF_KEY, needsCopy.key) : null;
    const minutes = rate && selfModel ? minutesAt(selfModel.sizeBytes, rate) : 0;
    const pinned = pinnedPlacementOf(way);
    // A way the planner judged unrunnable (unsupported, does not fit) is not kept as a node.
    const saveable = pinned != null && (c == null || wayPickable(way));
    const saved = saves[way.key];
    // After a start here succeeds, the running row asks once to keep this way as a node (§8.6).
    const offerSave = running && startedHere.has(way.key) && saved?.kind !== 'saved';

    return (
      <>
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
        {cutsLive && (
          <p
            data-testid={`placement-cuts-live-${way.kind}`}
            className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.err)}
          >
            {cutMessage(intl, intl.formatMessage(i18n.run), cutsLive)}
          </p>
        )}
        {tooSmall && c?.fit.context != null && liveWork?.contextTokens != null && (
          <p
            data-testid={`placement-too-small-${way.kind}`}
            className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}
          >
            {intl.formatMessage(i18n.tooSmallForLive, {
              context: c.fit.context.toLocaleString(),
              live: liveWork.contextTokens.toLocaleString(),
            })}
          </p>
        )}
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
              onClick={() => run(way)}
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
              onClick={() => stop(way)}
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
          {saveable && !offerSave && saved?.kind !== 'saved' && (
            <SaveAsNodeButton
              variant="ghost"
              saving={saved?.kind === 'saving'}
              onClick={() => void saveAsNode(way)}
              testId={`placement-save-node-${way.kind}`}
            />
          )}
        </div>
        {saveable && offerSave && (
          <div
            data-testid={`placement-save-offer-${way.kind}`}
            className="flex flex-wrap items-center gap-2"
          >
            <span className={cx('break-words', TYPE.body, WEIGHT.semibold)}>
              {intl.formatMessage(i18n.saveOffer)}
            </span>
            <SaveAsNodeButton
              variant="primary"
              saving={saved?.kind === 'saving'}
              onClick={() => void saveAsNode(way)}
              testId={`placement-save-offer-button-${way.kind}`}
            />
          </div>
        )}
        {saved?.kind === 'saved' && (
          <p
            data-testid={`placement-saved-${way.kind}`}
            className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.ok)}
          >
            {intl.formatMessage(saved.already ? i18n.alreadyNode : i18n.savedAs, {
              name: saved.name,
            })}{' '}
            <a
              className="underline"
              href={`#${nodeHref(saved.id)}`}
            >
              {intl.formatMessage(i18n.openInNodes)}
            </a>
          </p>
        )}
        {saved?.kind === 'refused' && (
          <p
            data-testid={`placement-save-refused-${way.kind}`}
            className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.err)}
          >
            {intl.formatMessage(i18n.saveRefused, { reason: saved.text })}
          </p>
        )}
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
        {action?.kind === 'unavailable' && !needsCopy && (
          <p className={cx('break-words', TYPE.meta)}>
            {way.mac ? macs.describeError(way.mac, action.reason) : action.reason}
          </p>
        )}
        {way.kind === 'split' && (splitDetails || otherSplits.length > 0) && (
          <Disclosure
            variant="plain"
            title={intl.formatMessage(i18n.details)}
            meta={<span className={TYPE.meta}>{intl.formatMessage(i18n.detailsMeta)}</span>}
            open={detailsOpen}
            onOpenChange={setDetailsOpen}
            testId="placement-split-details"
          >
            <div className="flex flex-col gap-3">
              {/* Splits goose cannot start are reference, not a choice: they live here (Q-25). */}
              {otherSplits.length > 0 && <OtherSplits splits={otherSplits} />}
              {splitDetails}
            </div>
          </Disclosure>
        )}
      </>
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
        <PlacementCandidates
          plan={plan}
          ways={ways}
          goal={goal}
          rowFacts={rowFacts}
          renderBelow={renderBelow}
        />
      )}
      {(plan?.notes ?? []).map((note) => (
        <p key={note} className={TYPE.meta}>
          {note}
        </p>
      ))}
      {cutDialog}
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

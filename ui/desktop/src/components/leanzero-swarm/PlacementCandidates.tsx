import type { ReactNode } from 'react';
import type { IntlShape } from 'react-intl';
import { Chip, RADIUS, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx, type EnginePhase } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import {
  goalFigure,
  linkPeerOf,
  measuredFigure,
  type PlacementCandidate,
  type PlacementGoal,
  type PlacementPlan,
  type SpeedFigure,
} from '../../acp/mlx-placement';
import { distributedStateWord, linkText } from './mlxModeLabel';
import { macForPlacementNode, peerRefuses, type Mac } from './macs';

/**
 * THE WAYS LIST — the placement planner's candidates as a person picks between them: the way's
 * name, goose's Best / Best-you-can-start-now, the split's link, the live state, the goal's speed
 * figure with its range and whether it was measured, the context it fits, and why it lost. One
 * list, two hosts: Run it (PlacementCard, which draws its Run / Stop / Measure / Copy controls under
 * each row) and the New node dialog's "How should it run?" step (which picks a row). Extracted from
 * PlacementCard so the two can never describe the same way in different words (DESIGN-NODES-AND-
 * STRATEGIES.md §5.3, §8.3).
 */

const i18n = defineMessages({
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
  splitOver: { id: 'placementCard.splitOver', defaultMessage: 'split {link}' },
  splitPlain: { id: 'placementCard.splitPlain', defaultMessage: 'split' },
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
  contextBelowChats: {
    id: 'placementCard.contextBelowChats',
    defaultMessage: 'fits only {context} tokens of context — your chats here reach about {need}',
  },
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
  liveMounting: { id: 'placementCard.live.mounting', defaultMessage: 'Mounting' },
  liveRunning: { id: 'placementCard.live.running', defaultMessage: 'Running' },
  liveFailed: { id: 'placementCard.live.failed', defaultMessage: 'Failed' },
  liveReconnecting: { id: 'placementCard.live.reconnecting', defaultMessage: 'Reconnecting' },
});

const GIB = 1024 * 1024 * 1024;

export function gb(bytes: number): string {
  return `${(bytes / GIB).toFixed(1)} GB`;
}

export function tps(value: number): string {
  return value >= 100 ? value.toFixed(0) : value.toFixed(1);
}

/** A model as the card names it: its folder name, without the publisher. */
export function shortModel(modelId: string): string {
  return modelId.split('/').pop() || modelId;
}

function linkWord(candidate: PlacementCandidate): string {
  return candidate.key.link === 'jaccl' ? 'JACCL' : (candidate.key.link ?? '');
}

function goalFigureText(
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
    case 'contextBelowChats':
      return intl.formatMessage(i18n.contextBelowChats, {
        context: o.context.toLocaleString(),
        need: o.need.toLocaleString(),
      });
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

export function tradeOffText(intl: IntlShape, t: SplitTradeOff): string {
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

/** The splits beside the offered one, each with why goose will not run it — under its Details. */
export function OtherSplits({ splits }: { splits: readonly PlacementCandidate[] }) {
  const intl = useIntl();
  return (
    <div className="flex flex-col gap-2" data-testid="placement-others">
      <span className={cx(TYPE.meta, WEIGHT.semibold)}>
        {intl.formatMessage(i18n.others, { count: splits.length })}
      </span>
      <ul className="flex flex-col gap-2">
        {splits.map((c) => (
          <li key={c.id} className="flex flex-col gap-0.5" data-testid={`placement-other-${c.id}`}>
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
    </div>
  );
}

export function LiveChip({ live }: { live: { phase: EnginePhase; state: string } }) {
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

/** A way as its row names it: this Mac, the peer by its name, or the split across N Macs. */
export function wayTitle(intl: IntlShape, way: Way): string {
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
}

/** Why a way lost, or what the split costs against one Mac — the row's line under its figures. */
export function wayWhy(intl: IntlShape, plan: PlacementPlan | null, way: Way): string | null {
  const c = way.candidate;
  if (!c) return null;
  // The split beside a Mac the model fits on states what it costs and buys (Q-72) — that line
  // carries both writing figures, so goose's "Slower for this" would only repeat half of it.
  const tradeOff = way.kind === 'split' ? splitTradeOff(plan, c) : null;
  return tradeOff ? tradeOffText(intl, tradeOff) : outcomeText(intl, c);
}

/**
 * A row a person may pick for a node: goose planned it, can run it for this architecture, and its
 * memory fits (a way that is only unavailable TODAY — the model not copied yet, a permission off —
 * stays pickable and carries its "needs" line; design §8.3).
 */
export function wayPickable(way: Way): boolean {
  const c = way.candidate;
  return c != null && c.supported && c.outcome.code !== 'doesNotFit';
}

/** What the host knows about one row that the plan does not: its live state, and Q-148's rule. */
export interface WayRowFacts {
  live: { phase: EnginePhase; state: string } | null;
  /** Its context is under what the conversation being answered holds: never "Best". */
  tooSmall: boolean;
}

export interface PlacementCandidatesProps {
  plan: PlacementPlan | null;
  ways: readonly Way[];
  goal: PlacementGoal;
  rowFacts?: (way: Way) => WayRowFacts;
  /** What the host draws under a row's shared lines (Run it's controls, the dialog's needs line). */
  renderBelow?: (way: Way) => ReactNode;
  /** Pick mode: each pickable row is a radio, the picked one carries the selection ring. */
  pick?: { selected: string | null; onSelect: (way: Way) => void; label: string };
}

const NO_FACTS: WayRowFacts = { live: null, tooSmall: false };

/**
 * A candidate's figures as Run it says them: the goal's speed figure with its range and whether it
 * was measured, and the context it fits. The node cards show a planned way with this same row.
 */
export function CandidateFigures({
  candidate: c,
  goal,
}: {
  candidate: PlacementCandidate;
  goal: PlacementGoal;
}) {
  const intl = useIntl();
  const figure = goalFigure(c, goal);
  return (
    <div className="flex flex-wrap items-center gap-2">
      {figure && (
        <>
          <span className={cx('text-lz-body', WEIGHT.semibold, TNUM, TONE_TEXT.accent)}>
            {goalFigureText(intl, goal, figure, c)}
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
  );
}

function PickDot({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden
      className={cx(
        'inline-flex size-4 shrink-0 items-center justify-center border-2 border-current text-lz-ink',
        RADIUS.pill
      )}
    >
      {checked && <span className={cx('size-2 bg-current', RADIUS.pill)} />}
    </span>
  );
}

export function PlacementCandidates({
  plan,
  ways,
  goal,
  rowFacts,
  renderBelow,
  pick,
}: PlacementCandidatesProps) {
  const intl = useIntl();
  return (
    <ul
      className="flex flex-col gap-2"
      data-testid="placement-ways"
      role={pick ? 'radiogroup' : undefined}
      aria-label={pick?.label}
    >
      {ways.map((way) => {
        const c = way.candidate;
        const { live, tooSmall } = rowFacts?.(way) ?? NO_FACTS;
        // "Best" is goose's: the planner ranks long documents by a whole turn's expected time, so a
        // split that reads faster but writes slower wins only when the turn's answer is short enough.
        const isBest = c != null && plan?.best === c.id && !tooSmall;
        const isBestNow =
          c != null &&
          plan?.bestAvailable === c.id &&
          plan.bestAvailable !== plan.best &&
          !tooSmall;
        const why = wayWhy(intl, plan, way);
        const picked = pick != null && pick.selected === way.key;
        const onPick = pick != null && wayPickable(way) ? pick.onSelect : null;
        const lines = (
          <>
            <div className="flex flex-wrap items-center gap-2">
              {onPick && <PickDot checked={picked} />}
              <span className={TYPE.h2}>{wayTitle(intl, way)}</span>
              {isBest && <Chip tone="accent">{intl.formatMessage(i18n.best)}</Chip>}
              {isBestNow && <Chip tone="ok">{intl.formatMessage(i18n.bestNow)}</Chip>}
              {c && way.kind === 'split' && (
                // Plain words on the card; the runner and the transport ride the chip's title and
                // the Details below (Q-174: "tensor split · JACCL" on Run it).
                <Chip
                  title={intl.formatMessage(c.key.kind === 'tensor' ? i18n.tensor : i18n.pipeline, {
                    link: linkWord(c),
                  })}
                >
                  {linkText(intl, c.key.link)
                    ? intl.formatMessage(i18n.splitOver, { link: linkText(intl, c.key.link) })
                    : intl.formatMessage(i18n.splitPlain)}
                </Chip>
              )}
              {live && <LiveChip live={live} />}
            </div>
            {c && <CandidateFigures candidate={c} goal={goal} />}
            {why && (
              <p className={cx('break-words', TYPE.meta)} title={c?.fit.detail}>
                {why}
              </p>
            )}
          </>
        );
        return (
          <li
            key={way.key}
            data-testid={`placement-way-${way.kind}`}
            data-way={way.key}
            className={cx(
              'flex flex-col gap-2 p-3',
              SURFACE.inset,
              RADIUS.card,
              picked && SURFACE.selectedRing
            )}
          >
            {onPick ? (
              <button
                type="button"
                role="radio"
                aria-checked={picked}
                data-testid={`placement-pick-${way.kind}`}
                onClick={() => onPick(way)}
                className="flex flex-col gap-2 text-left"
              >
                {lines}
              </button>
            ) : (
              lines
            )}
            {renderBelow?.(way)}
          </li>
        );
      })}
    </ul>
  );
}

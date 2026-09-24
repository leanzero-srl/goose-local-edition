import { useCallback, useEffect, useRef, useState } from 'react';
import { Gauge, Loader2, Play, RefreshCw } from 'lucide-react';
import {
  Button,
  Chip,
  Disclosure,
  RADIUS,
  SURFACE,
  Segmented,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type Tone,
} from '../lz';
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
  type PlacementNode,
  type PlacementPlan,
  type SpeedFigure,
} from '../../acp/mlx-placement';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import { mlxDistributedStart, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStart,
  mlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { mlxErrorMessage } from './mlxErrorMessage';

const i18n = defineMessages({
  title: { id: 'placementCard.title', defaultMessage: 'Where to run it' },
  goalLabel: { id: 'placementCard.goalLabel', defaultMessage: 'What matters most' },
  goalChat: { id: 'placementCard.goalChat', defaultMessage: 'Chat' },
  goalLong: { id: 'placementCard.goalLong', defaultMessage: 'Long documents' },
  goalMany: { id: 'placementCard.goalMany', defaultMessage: 'Many requests' },
  planning: {
    id: 'placementCard.planning',
    defaultMessage: 'Measuring your Macs and the model…',
  },
  planFailed: { id: 'placementCard.planFailed', defaultMessage: 'Could not plan' },
  best: { id: 'placementCard.best', defaultMessage: 'Best' },
  bestNow: { id: 'placementCard.bestNow', defaultMessage: 'Best you can start now' },
  nothingFits: {
    id: 'placementCard.nothingFits',
    defaultMessage: 'Nothing fits right now',
  },
  single: { id: 'placementCard.single', defaultMessage: '{name} alone' },
  tensor: {
    id: 'placementCard.tensor',
    defaultMessage: '{count, plural, one {# Mac} other {# Macs}} · tensor split · {link}',
  },
  pipeline: {
    id: 'placementCard.pipeline',
    defaultMessage: '{count, plural, one {# Mac} other {# Macs}} · pipeline split · {link}',
  },
  thisMac: { id: 'placementCard.thisMac', defaultMessage: 'this Mac' },
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
  noFigure: { id: 'placementCard.noFigure', defaultMessage: 'no speed figure' },
  useThis: { id: 'placementCard.useThis', defaultMessage: 'Use this' },
  starting: { id: 'placementCard.starting', defaultMessage: 'Starting…' },
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
  measureLater: {
    id: 'placementCard.measureLater',
    defaultMessage: 'Measure speed becomes available once it runs.',
  },
  setupFirst: {
    id: 'placementCard.setupFirst',
    defaultMessage:
      'The distributed setup names another model — pick this one in Distributed › Set up, then start it.',
  },
  refresh: { id: 'placementCard.refresh', defaultMessage: 'Plan again' },
  others: {
    id: 'placementCard.others',
    defaultMessage: '{count, plural, one {# other way} other {# other ways}}',
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
  measuredResult: {
    id: 'placementCard.measuredResult',
    defaultMessage: 'Measured: {decode} tok/s writing, {prefill} tok/s reading',
  },
  storeErrors: {
    id: 'placementCard.storeErrors',
    defaultMessage: 'Unreadable lines in the measurement store',
  },
  nodeLine: {
    id: 'placementCard.nodeLine',
    defaultMessage: '{name}: {chip} · {bandwidth} GB/s · GPU ceiling {ceiling}',
  },
  nodeGap: { id: 'placementCard.nodeGap', defaultMessage: '{name}: {reason}' },
  badgeThisMac: { id: 'placementCard.badgeThisMac', defaultMessage: 'Fits this Mac' },
  badgePeer: { id: 'placementCard.badgePeer', defaultMessage: 'Fits {name}' },
  badgeBoth: { id: 'placementCard.badgeBoth', defaultMessage: 'Needs both Macs' },
  badgeTooBig: { id: 'placementCard.badgeTooBig', defaultMessage: 'Too big, short {gb}' },
  badgeUnknown: { id: 'placementCard.badgeUnknown', defaultMessage: 'Fit unknown' },
  gpuCores: { id: 'placementCard.gpuCores', defaultMessage: '{brand} · {cores}-core GPU' },
  actionFailed: { id: 'placementCard.actionFailed', defaultMessage: 'The action failed' },
  refusedUnnamed: {
    id: 'placementCard.refusedUnnamed',
    defaultMessage: 'Refused, and goose named no reason',
  },
});

type NoticeTone = Exclude<Tone, 'secondary'>;

const GIB = 1024 * 1024 * 1024;

function gb(bytes: number): string {
  return `${(bytes / GIB).toFixed(1)} GB`;
}

function tps(value: number): string {
  return value >= 100 ? value.toFixed(0) : value.toFixed(1);
}

/** The words a candidate is called by. */
export function candidateLabel(intl: IntlShape, candidate: PlacementCandidate): string {
  const link = candidate.key.link === 'jaccl' ? 'JACCL' : (candidate.key.link ?? '');
  if (candidate.key.kind === 'single') {
    const local = candidate.key.nodes[0] === 'local';
    const name = candidate.nodeNames[0] ?? candidate.key.nodes[0];
    return intl.formatMessage(i18n.single, {
      name: local ? `${name} (${intl.formatMessage(i18n.thisMac)})` : name,
    });
  }
  return intl.formatMessage(candidate.key.kind === 'tensor' ? i18n.tensor : i18n.pipeline, {
    count: candidate.key.nodes.length,
    link,
  });
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
      // goose's reason already opens with "not supported yet" / "not offered" in its own words.
      return o.reason;
    case 'noFigure':
      return `${intl.formatMessage(i18n.noFigureReason)}: ${o.reason}`;
  }
}

/** Is this candidate the engine running right now (so Measure speed can reach it)? */
export function candidateRunning(
  candidate: PlacementCandidate,
  modelId: string,
  single: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null
): boolean {
  if (candidate.id === 'single:local') {
    return single?.state === 'running' && single.modelId === modelId;
  }
  const peer = linkPeerOf(candidate);
  if (peer != null) {
    const remote = latestMlxRemoteSingleStatus();
    return remote?.state === 'ready' && remote.peer === peer && remote.modelId === modelId;
  }
  if (candidate.key.kind === 'single') return false;
  return (
    distributed != null &&
    (distributed.state === 'ready' || distributed.state === 'serving') &&
    distributed.modelId === modelId
  );
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
 * Every local model's badge, planned once per call of `refresh` (the Engine tab's mount and every
 * change of the model list). A failed plan leaves no badge — the card names the failure.
 */
export function usePlacementBadges(modelKey: string): Map<string, PlacementBadgeDto> {
  const [badges, setBadges] = useState<Map<string, PlacementBadgeDto>>(new Map());
  useEffect(() => {
    let cancelled = false;
    mlxPlacementPlan('chat')
      .then((response) => {
        if (cancelled) return;
        const next = new Map<string, PlacementBadgeDto>();
        for (const plan of response.plans) {
          if (plan.badge) next.set(plan.modelId, plan.badge);
        }
        setBadges(next);
      })
      .catch(() => {
        if (!cancelled) setBadges(new Map());
      });
    return () => {
      cancelled = true;
    };
  }, [modelKey]);
  return badges;
}

function NodeFacts({ nodes }: { nodes: PlacementNode[] }) {
  const intl = useIntl();
  return (
    <ul className="flex flex-col gap-0.5" data-testid="placement-nodes">
      {nodes.map((n) => {
        const gap = n.chipError ?? n.bandwidthError ?? n.memoryError ?? n.ceilingError;
        const chip = n.chip
          ? n.chip.gpuCores != null
            ? intl.formatMessage(i18n.gpuCores, { brand: n.chip.brand, cores: n.chip.gpuCores })
            : n.chip.brand
          : '—';
        return (
          <li key={n.id} className={cx(TYPE.meta, TNUM, gap && TONE_TEXT.err)}>
            {intl.formatMessage(i18n.nodeLine, {
              name: n.name,
              chip,
              bandwidth: n.bandwidthGbs != null ? n.bandwidthGbs.toFixed(0) : '—',
              ceiling: n.ceilingBytes != null ? gb(n.ceilingBytes) : '—',
            })}
            {gap && ` — ${intl.formatMessage(i18n.nodeGap, { name: n.name, reason: gap })}`}
          </li>
        );
      })}
    </ul>
  );
}

interface PlacementCardProps {
  modelId: string;
  single: MlxEngineStatus | null;
  distributed: MlxDistributedStatus | null;
  /** Mount the model on this Mac's single engine (the view's own mount path, gate and all). */
  onMountHere: () => void;
  mountBusy: boolean;
}

/**
 * The recommendation for the picked model: the best placement for the goal with its speed
 * (measured or estimated, with its range) and context, [Use this] and [Measure speed], and every
 * other placement folded with the reason it lost.
 */
export function PlacementCard({
  modelId,
  single,
  distributed,
  onMountHere,
  mountBusy,
}: PlacementCardProps) {
  const intl = useIntl();
  const [goal, setGoal] = useState<PlacementGoal>('chat');
  const [plan, setPlan] = useState<PlacementPlan | null>(null);
  const [nodes, setNodes] = useState<PlacementNode[]>([]);
  const [storeErrors, setStoreErrors] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ tone: NoticeTone; text: string } | null>(null);
  const [othersOpen, setOthersOpen] = useState(false);
  const [, setRemoteTick] = useState(0);
  const request = useRef(0);

  useEffect(() => subscribeMlxRemoteSingleStatus(() => setRemoteTick((t) => t + 1)), []);

  const load = useCallback(async () => {
    const mine = ++request.current;
    setLoading(true);
    setError(null);
    // Where chat goes now decides which candidate is running (Measure speed reaches only that one);
    // a failed read publishes "unknown", which offers no measurement.
    mlxRemoteSingleStatus().catch(() => undefined);
    try {
      const response = await mlxPlacementPlan(goal, modelId);
      if (mine !== request.current) return;
      setPlan(response.plans[0] ?? null);
      setNodes(response.nodes);
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

  const use = async (candidate: PlacementCandidate) => {
    setNotice(null);
    const action = candidate.action;
    if (action.kind === 'mountHere') {
      onMountHere();
      return;
    }
    setBusy(`use:${candidate.id}`);
    try {
      if (action.kind === 'startSplit') {
        const response = await mlxDistributedStart(null);
        setNotice(
          response.started
            ? { tone: 'accent', text: intl.formatMessage(i18n.started) }
            : {
                tone: 'err',
                text: response.refusal?.message ?? intl.formatMessage(i18n.refusedUnnamed),
              }
        );
      } else if (action.kind === 'remoteSingle') {
        const peer = linkPeerOf(candidate);
        if (peer == null) return;
        const response = await mlxRemoteSingleStart(peer, modelId);
        setNotice(
          response.started
            ? { tone: 'accent', text: intl.formatMessage(i18n.started) }
            : {
                tone: 'err',
                text: response.refusal?.message ?? intl.formatMessage(i18n.refusedUnnamed),
              }
        );
      }
    } catch (e) {
      setNotice({ tone: 'err', text: mlxErrorMessage(e, intl.formatMessage(i18n.actionFailed)) });
    } finally {
      setBusy(null);
    }
  };

  const measure = async (candidate: PlacementCandidate) => {
    setNotice(null);
    setBusy(`measure:${candidate.id}`);
    try {
      const response = await mlxMeasureSpeed(modelId, candidate.id, goal === 'longDocuments');
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

  const byId = (id: string | null | undefined) =>
    id ? (plan?.candidates?.find((c) => c.id === id) ?? null) : null;
  const best = byId(plan?.best);
  const bestNow =
    plan?.bestAvailable && plan.bestAvailable !== plan.best ? byId(plan.bestAvailable) : null;
  const others = (plan?.candidates ?? []).filter((c) => c !== best && c !== bestNow);

  const actionFor = (candidate: PlacementCandidate, primary: boolean) => {
    const running = candidateRunning(candidate, modelId, single, distributed);
    const action = candidate.action;
    const decode = candidate.speed.decode;
    const measuring = busy === `measure:${candidate.id}`;
    return (
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          {!running &&
            action.kind !== 'unavailable' &&
            !(action.kind === 'startSplit' && !action.setupMatches) && (
              <Button
                variant={primary ? 'primary' : 'secondary'}
                icon={
                  busy === `use:${candidate.id}` ? <Loader2 className="animate-spin" /> : <Play />
                }
                disabled={busy != null || (action.kind === 'mountHere' && mountBusy)}
                onClick={() => void use(candidate)}
                data-testid={`placement-use-${candidate.id}`}
              >
                {busy === `use:${candidate.id}`
                  ? intl.formatMessage(i18n.starting)
                  : intl.formatMessage(i18n.useThis)}
              </Button>
            )}
          {running && (
            <Button
              variant={decode?.measured ? 'secondary' : 'primary'}
              icon={measuring ? <Loader2 className="animate-spin" /> : <Gauge />}
              disabled={busy != null}
              onClick={() => void measure(candidate)}
              data-testid={`placement-measure-${candidate.id}`}
            >
              {intl.formatMessage(i18n.measure)}
            </Button>
          )}
        </div>
        {measuring && <p className={TYPE.meta}>{intl.formatMessage(i18n.measuring)}</p>}
        {running && !decode?.measured && !measuring && (
          <p className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.measureFirst)}</p>
        )}
        {action.kind === 'unavailable' && (
          <p className={cx('break-words', TYPE.meta)}>{action.reason}</p>
        )}
        {action.kind === 'startSplit' && !action.setupMatches && (
          <p className={cx('break-words', TYPE.meta)}>{intl.formatMessage(i18n.setupFirst)}</p>
        )}
        {!running && action.kind !== 'unavailable' && primary && (
          <p className={TYPE.meta}>{intl.formatMessage(i18n.measureLater)}</p>
        )}
      </div>
    );
  };

  const headline = (candidate: PlacementCandidate, label: string, primary: boolean) => {
    const figure = goalFigure(candidate, goal);
    const context = candidate.fit.context;
    return (
      <div
        className={cx('flex flex-col gap-2 p-3', SURFACE.inset, RADIUS.card)}
        data-testid={primary ? 'placement-best' : 'placement-best-now'}
      >
        <span
          className={cx('text-lz-meta', WEIGHT.semibold, primary ? TONE_TEXT.accent : TONE_TEXT.ok)}
        >
          {label}
        </span>
        <span className={TYPE.h2}>{candidateLabel(intl, candidate)}</span>
        <div className="flex flex-wrap items-center gap-2">
          {figure ? (
            <>
              <span className={cx('text-lz-h2', WEIGHT.semibold, TNUM, TONE_TEXT.accent)}>
                {figureText(intl, goal, figure, candidate.speed.concurrency)}
              </span>
              <span className={cx(TYPE.meta, TNUM)}>
                {intl.formatMessage(i18n.range, {
                  low: tps(figure.estimate.low),
                  high: tps(figure.estimate.high),
                })}
              </span>
              <SourceChip figure={figure} />
            </>
          ) : (
            <Chip>{intl.formatMessage(i18n.noFigure)}</Chip>
          )}
          {context != null && (
            <Chip>
              {intl.formatMessage(
                candidate.fit.status === 'smallerContext' ? i18n.smallerContext : i18n.context,
                { tokens: context.toLocaleString() }
              )}
            </Chip>
          )}
        </div>
        {actionFor(candidate, primary)}
      </div>
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
      {plan && !plan.error && (
        <>
          {best ? (
            headline(best, intl.formatMessage(i18n.best), true)
          ) : (
            <p className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}>
              {intl.formatMessage(i18n.nothingFits)}
            </p>
          )}
          {bestNow && headline(bestNow, intl.formatMessage(i18n.bestNow), false)}
          {(plan.notes ?? []).map((note) => (
            <p key={note} className={TYPE.meta}>
              {note}
            </p>
          ))}
          {others.length > 0 && (
            <Disclosure
              title={intl.formatMessage(i18n.others, { count: others.length })}
              open={othersOpen}
              onOpenChange={setOthersOpen}
              testId="placement-others"
            >
              <ul className="flex flex-col gap-2">
                {others.map((c) => {
                  const figure = goalFigure(c, goal);
                  return (
                    <li
                      key={c.id}
                      className="flex flex-col gap-0.5"
                      data-testid={`placement-other-${c.id}`}
                    >
                      <span className="flex flex-wrap items-center gap-2">
                        <span className={cx(TYPE.body, WEIGHT.semibold)}>
                          {candidateLabel(intl, c)}
                        </span>
                        {figure && (
                          <span className={cx(TYPE.meta, TNUM)}>
                            {figureText(intl, goal, figure, c.speed.concurrency)}
                          </span>
                        )}
                        {figure && <SourceChip figure={figure} />}
                      </span>
                      <span className={cx('break-words', TYPE.meta)} title={c.fit.detail}>
                        {outcomeText(intl, c)}
                      </span>
                      {c.supported && c.fit.status !== 'short' && c.fit.status !== 'unknown' && (
                        <div className="mt-1">{actionFor(c, false)}</div>
                      )}
                    </li>
                  );
                })}
              </ul>
            </Disclosure>
          )}
          <NodeFacts nodes={nodes} />
        </>
      )}
    </section>
  );
}

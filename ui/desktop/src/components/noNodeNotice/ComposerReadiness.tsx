import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ChevronRight,
  Hourglass,
  Laptop,
  Loader2,
  Network,
  Play,
  ServerOff,
  Settings2,
} from 'lucide-react';
import { mlxEngineModelsList, type MlxEngineStatus } from '../../acp/mlx-engine';
import { mlxDistributedStart, mlxDistributedStatus } from '../../acp/mlx-distributed';
import { gb1, gib } from '../leanzero-swarm/mlxDistributed';
import { errorMessage } from '../../utils/conversionUtils';
import { PeerHeldLine } from '../leanzero-swarm/PeerHeldLine';
import { dropRoute } from '../leanzero-swarm/routeSwitch';
import { routePeerName } from '../leanzero-swarm/macs';
import { formatMlxMode } from '../leanzero-swarm/mlxModeLabel';
import { compactTokens } from '../leanzero-swarm/mlxLiveStats';
import { defineMessages, useIntl } from '../../i18n';
import { Button, PHASE_FILL, RADIUS, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { RestoreActions, restoreLineText, useRestoreLine } from '../leanzero-swarm/MlxRestoreLine';
import {
  servedReady,
  type ChatBusy,
  type ChatServedBy,
  type ComposerReadiness,
  type RunHere,
} from '../chatServedBy/chatServedBy';
import type { ChatServing } from '../chatServedBy/useChatServedBy';
import { splitStopHeadline, splitStopMemory } from '../chatServedBy/splitStopText';
import {
  distributedProblem,
  distributedServedId,
  distributedServes,
  distributedStateLabel,
  distributedSummary,
  shortModelName,
  useMlxMount,
} from './mlxMount';

const i18n = defineMessages({
  unmounted: {
    id: 'composerReadiness.unmounted',
    defaultMessage: 'No model is mounted — {nodes}',
  },
  noNodes: {
    id: 'composerReadiness.noNodes',
    defaultMessage: 'No swarm node is enabled — nothing can answer a message',
  },
  failed: {
    id: 'composerReadiness.failed',
    defaultMessage: 'The last mount failed: {error}',
  },
  mismatch: {
    id: 'composerReadiness.mismatch',
    defaultMessage: 'The saved MLX model serves {served}; the node wants {wanted}.',
  },
  noTarget: {
    id: 'composerReadiness.noTarget',
    defaultMessage: 'No saved MLX model — pick one in Providers.',
  },
  distributed: {
    id: 'composerReadiness.distributed',
    defaultMessage: '{mode} · {state} — {nodes}',
  },
  distributedMismatch: {
    id: 'composerReadiness.distributedMismatch',
    defaultMessage: 'The distributed engine serves {served}; the node wants {wanted}.',
  },
  remoteLoading: {
    id: 'composerReadiness.remoteLoading',
    defaultMessage: 'Loading {model} on {peer} — a message waits until it is ready',
  },
  remoteLoadingUnnamed: {
    id: 'composerReadiness.remoteLoadingUnnamed',
    defaultMessage: 'Loading the model on {peer} — a message waits until it is ready',
  },
  remoteFailed: {
    id: 'composerReadiness.remoteFailed',
    defaultMessage: 'The engine on {peer} failed — nothing there can answer a message',
  },
  reconnecting: {
    id: 'composerReadiness.reconnecting',
    defaultMessage: 'Lost contact with {peer} — reconnecting…',
  },
  reconnectingPlain: {
    id: 'composerReadiness.reconnectingPlain',
    defaultMessage:
      'goose keeps trying; once {peer} answers, goose checks whether it still has your answer',
  },
  leaving: {
    id: 'composerReadiness.leaving',
    defaultMessage:
      '{cause, select, restart {{peer} is restarting goose} other {{peer} quit goose}}{turn, select, yes { — this answer stops} other {}}',
  },
  leavingPlain: {
    id: 'composerReadiness.leavingPlain',
    defaultMessage: 'goose reconnects when it is back',
  },
  loadHere: {
    id: 'composerReadiness.loadHere',
    defaultMessage: 'Load {model} here{size, select, none {} other { ({size} GB)}}',
  },
  details: { id: 'composerReadiness.details', defaultMessage: 'Details' },
  runHere: { id: 'composerReadiness.runHere', defaultMessage: 'Run on this Mac instead' },
  runHereHint: {
    id: 'composerReadiness.runHereHint',
    defaultMessage: 'Stops sending chat to {peer} and runs it on this Mac',
  },
  switchingHere: {
    id: 'composerReadiness.switchingHere',
    defaultMessage: 'Moving chat to this Mac…',
  },
  switchFailed: {
    id: 'composerReadiness.switchFailed',
    defaultMessage: 'Could not move chat to this Mac: {error}',
  },
  busy: {
    id: 'composerReadiness.busy',
    defaultMessage:
      '{where} is working on {count, plural, one {another request} other {# other requests}} — your message waits its turn',
  },
  busyReading: {
    id: 'composerReadiness.busyReading',
    defaultMessage:
      '{where} is reading another request’s {tokens}-token prompt — your message waits its turn',
  },
  splitStart: { id: 'composerReadiness.splitStart', defaultMessage: 'Start the split again' },
  splitStarting: { id: 'composerReadiness.splitStarting', defaultMessage: 'Starting the split…' },
  splitStartFailed: {
    id: 'composerReadiness.splitStartFailed',
    defaultMessage: 'The split did not start: {error}',
  },
  splitOneMac: { id: 'composerReadiness.splitOneMac', defaultMessage: 'Run on one Mac instead' },
  splitOneMacHint: {
    id: 'composerReadiness.splitOneMacHint',
    defaultMessage: 'Loads {model} on this Mac alone — the split stays stopped',
  },
  splitOneMacLoading: {
    id: 'composerReadiness.splitOneMacLoading',
    defaultMessage: 'Loading on this Mac…',
  },
  mount: { id: 'composerReadiness.mount', defaultMessage: 'Mount {model}' },
  mounting: { id: 'composerReadiness.mounting', defaultMessage: 'Mounting {model}' },
  openEngine: { id: 'composerReadiness.openEngine', defaultMessage: 'Open Engine' },
  theEngine: { id: 'composerReadiness.theEngine', defaultMessage: 'The engine' },
});

/** Where "Open Engine" goes: the Providers view's LeanZero MLX tab (Engine, with Run it). */
export const ENGINE_ROUTE = '/leanzero-swarm?tab=mlx';

/**
 * The composer's readiness bar: a solid bar ABOVE the input ONLY when something needs the user —
 * a model loading or failed, nothing mounted, no node, a split not answering, a relaunch bringing
 * the engine back, or the engine busy with another client's request (a new turn would queue behind
 * it). While everything is ready it renders nothing: the model chip names what serves (Q-8, Q-17).
 * It never blocks typing. Every fact comes from `serving` — the one derivation, never its own read.
 */
export function ComposerReadinessStrip({ serving }: { serving: ChatServing }) {
  return (
    <>
      <PeerHeldLine />
      <ReadinessBar serving={serving} />
    </>
  );
}

function ReadinessBar({ serving }: { serving: ChatServing }) {
  const intl = useIntl();
  const { served, single, armed } = serving;
  const { readiness } = served;
  const { requestingNodeId, mountErrors, mount } = useMlxMount(single);
  const restore = useRestoreLine();
  const restoreText = restoreLineText(intl, restore);
  const [switching, setSwitching] = useState(false);
  const [switchError, setSwitchError] = useState<string | null>(null);
  const [startingSplit, setStartingSplit] = useState(false);
  const [splitStartError, setSplitStartError] = useState<string | null>(null);

  // "Run on this Mac instead": the route withdrawn on this Mac without waiting on the Mac that is
  // not answering (routeSwitch.ts — the one switch path), then this Mac's Mount at once. Whether
  // that Mac frees its model is its own business: the quiet PeerHeldLine says so.
  const runHere = useCallback(
    async (instead: RunHere) => {
      if (instead.kind !== 'switch') return;
      setSwitching(true);
      setSwitchError(null);
      try {
        const dropped = dropRoute(true);
        // The route record goes first inside the call; the peer is only asked afterwards.
        await dropped.routeGone;
        if (instead.mount) await mount(STRIP_MOUNT_KEY, instead.mount);
      } catch (e) {
        setSwitchError(errorMessage(e, String(e)));
      } finally {
        setSwitching(false);
      }
    },
    [mount]
  );

  // "Start the split again": the saved split, the same call Run it and the relaunch restore make.
  // Its status is read at once so every surface leaves "stopped" on the run's own state.
  const startSplit = useCallback(async () => {
    setStartingSplit(true);
    setSplitStartError(null);
    try {
      const response = await mlxDistributedStart(null);
      if (!response.started) {
        setSplitStartError(response.refusal?.message ?? response.refusal?.code ?? null);
      }
      await mlxDistributedStatus().catch(() => undefined);
    } catch (e) {
      setSplitStartError(errorMessage(e, String(e)));
    } finally {
      setStartingSplit(false);
    }
  }, []);

  // A relaunch bringing back what served: that is the line, not "No model is mounted" + Mount.
  if (armed && restoreText != null && !servedReady(served)) {
    return (
      <div
        role={restore.phase === 'failed' ? 'alert' : 'status'}
        data-testid="composer-readiness"
        data-readiness={`restore-${restore.phase}`}
        className={cx(
          'mb-2 flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2',
          RADIUS.control,
          restore.phase === 'failed' ? PHASE_FILL.failed : PHASE_FILL.loading
        )}
      >
        {restore.phase === 'restoring' && (
          <Loader2 aria-hidden className="size-4 shrink-0 animate-spin" />
        )}
        <span className={cx('min-w-0 flex-1 break-words text-lz-body', WEIGHT.semibold)}>
          {restoreText}
        </span>
        {restore.phase === 'failed' && <RestoreActions />}
      </div>
    );
  }
  if (servedReady(served)) {
    return served.busyWithOthers ? <BusyBar served={served} busy={served.busyWithOthers} /> : null;
  }
  if (readiness.kind === 'unknown' || readiness.kind === 'ready') return null;
  return (
    <ReadinessStripBody
      readiness={readiness}
      model={served.model}
      status={single}
      requesting={requestingNodeId != null}
      mountError={mountErrors[STRIP_MOUNT_KEY] ?? null}
      onMount={(modelId) => void mount(STRIP_MOUNT_KEY, modelId)}
      switching={switching}
      switchError={switchError}
      onRunHere={(instead) => void runHere(instead)}
      turnInFlight={serving.turnInFlight}
      startingSplit={startingSplit}
      splitStartError={splitStartError}
      onStartSplit={() => void startSplit()}
    />
  );
}

/**
 * What "Load <model> here" would load, as this Mac's models dir says: its size, or that it is not
 * here (then Load is not offered — the mount would only fail). `unknown` while unread or when the
 * list could not be read: the label then names no size, never a guessed one.
 */
type LocalModel = { state: 'unknown' } | { state: 'here'; sizeBytes: number } | { state: 'absent' };

function useLocalModel(modelId: string | null): LocalModel {
  const [fact, setFact] = useState<{ id: string; model: LocalModel } | null>(null);
  useEffect(() => {
    if (!modelId) return undefined;
    let alive = true;
    mlxEngineModelsList()
      .then((list) => {
        const found = list.models.find((m) => m.id === modelId);
        const model: LocalModel =
          found && found.complete
            ? { state: 'here', sizeBytes: found.sizeBytes }
            : { state: 'absent' };
        if (alive) setFact({ id: modelId, model });
      })
      .catch(() => {
        if (alive) setFact({ id: modelId, model: { state: 'unknown' } });
      });
    return () => {
      alive = false;
    };
  }, [modelId]);
  return fact && fact.id === modelId ? fact.model : { state: 'unknown' };
}

/** The strip mounts ONE engine whatever the node count, so its mount state has one key. */
const STRIP_MOUNT_KEY = 'composer';

function OpenEngineButton() {
  const intl = useIntl();
  const navigate = useNavigate();
  const openEngine = useCallback(() => navigate(ENGINE_ROUTE), [navigate]);
  return (
    <Button
      size="sm"
      variant="secondary"
      icon={<Settings2 />}
      data-testid="composer-readiness-open-engine"
      onClick={openEngine}
    >
      {intl.formatMessage(i18n.openEngine)}
    </Button>
  );
}

/** The engine that serves chat is answering someone else: a turn sent now queues behind them. */
function BusyBar({ served, busy }: { served: ChatServedBy; busy: ChatBusy }) {
  const intl = useIntl();
  const where = served.where.length
    ? intl.formatList(served.where, { type: 'conjunction' })
    : intl.formatMessage(i18n.theEngine);
  const headline =
    busy.readingTokens != null
      ? intl.formatMessage(i18n.busyReading, {
          where,
          tokens: compactTokens(busy.readingTokens),
        })
      : intl.formatMessage(i18n.busy, { where, count: busy.requests });
  return (
    <div
      role="status"
      data-testid="composer-readiness"
      data-readiness="busy"
      className={cx(
        'mb-2 flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2',
        RADIUS.control,
        PHASE_FILL.held
      )}
    >
      <Hourglass aria-hidden className="size-4 shrink-0" />
      <span className={cx('min-w-0 flex-1 break-words text-lz-body', WEIGHT.semibold)}>
        {headline}
      </span>
      <div className="flex shrink-0 items-center gap-2">
        <OpenEngineButton />
      </div>
    </div>
  );
}

function ReadinessStripBody({
  readiness,
  model,
  status,
  requesting,
  mountError,
  onMount,
  switching,
  switchError,
  onRunHere,
  turnInFlight,
  startingSplit,
  splitStartError,
  onStartSplit,
}: {
  readiness: Exclude<ComposerReadiness, { kind: 'unknown' } | { kind: 'ready' }>;
  model: string | null;
  status: MlxEngineStatus | null;
  requesting: boolean;
  mountError: string | null;
  onMount: (modelId: string) => void;
  switching: boolean;
  switchError: string | null;
  onRunHere: (instead: RunHere) => void;
  turnInFlight: boolean;
  startingSplit: boolean;
  splitStartError: string | null;
  onStartSplit: () => void;
}) {
  const intl = useIntl();
  const instead =
    readiness.kind === 'reconnecting' ||
    readiness.kind === 'remote' ||
    readiness.kind === 'split-stopped'
      ? readiness.instead
      : undefined;
  const local = useLocalModel(instead?.kind === 'switch' ? instead.mount : null);

  /**
   * "Run on this Mac instead", named for what it does (Q-57): load <model> (its size from this
   * Mac's own models dir), or — this Mac already serving — just stop sending chat there.
   */
  const runHereButton = (peer: string): ReactNode => {
    if (instead?.kind !== 'switch' || (instead.mount && local.state === 'absent')) return null;
    const label = instead.mount
      ? intl.formatMessage(i18n.loadHere, {
          model: shortModelName(instead.mount),
          size: local.state === 'here' ? gb1(gib(local.sizeBytes)) : 'none',
        })
      : intl.formatMessage(i18n.runHere);
    return (
      <Button
        size="sm"
        variant="secondary"
        icon={switching ? <Loader2 className="animate-spin" /> : <Laptop />}
        disabled={switching}
        title={intl.formatMessage(i18n.runHereHint, { peer })}
        data-testid="composer-readiness-run-here"
        onClick={() => onRunHere(instead)}
      >
        {switching ? intl.formatMessage(i18n.switchingHere) : label}
      </Button>
    );
  };

  let headline: string;
  let detail: string | null = null;
  let raw: string | null = null;
  let action: ReactNode = null;
  let fill = TONE_FILL.warn;
  // Every state in progress spins in ONE place — the bar's leading icon (Q-63).
  let spinning = false;

  if (readiness.kind === 'no-nodes') {
    headline = intl.formatMessage(i18n.noNodes);
  } else if (readiness.kind === 'split-stopped') {
    // The split chat was on stopped on its own: said as the split, with why — its two ways back
    // are starting it again or this Mac alone (Q-81). The supervisor's words only behind Details.
    const { stop } = readiness;
    headline = splitStopHeadline(intl, stop);
    fill = PHASE_FILL.failed;
    spinning = startingSplit || requesting;
    detail = splitStartError
      ? intl.formatMessage(i18n.splitStartFailed, { error: splitStartError })
      : mountError
        ? intl.formatMessage(i18n.failed, { error: mountError })
        : splitStopMemory(intl, stop);
    raw = stop.raw;
    const oneMac =
      instead?.kind === 'switch' && instead.mount && local.state !== 'absent'
        ? instead.mount
        : null;
    action = (
      <>
        <Button
          size="sm"
          variant="secondary"
          icon={<Play />}
          disabled={startingSplit || requesting}
          data-testid="composer-readiness-split-start"
          onClick={onStartSplit}
        >
          {intl.formatMessage(startingSplit ? i18n.splitStarting : i18n.splitStart)}
        </Button>
        {oneMac && (
          <Button
            size="sm"
            variant="secondary"
            icon={<Laptop />}
            disabled={startingSplit || requesting}
            title={intl.formatMessage(i18n.splitOneMacHint, { model: shortModelName(oneMac) })}
            data-testid="composer-readiness-split-one-mac"
            onClick={() => onMount(oneMac)}
          >
            {intl.formatMessage(requesting ? i18n.splitOneMacLoading : i18n.splitOneMac)}
          </Button>
        )}
      </>
    );
  } else if (readiness.kind === 'reconnecting') {
    // The route's Mac stopped answering: chat still goes there, so every second until it answers
    // again — or the user moves chat here — is named (Q-47). A Mac that said it quit or is
    // restarting goose took its engine with it: the answer is gone, and the bar says so (Q-54).
    const peer = routePeerName(readiness.status);
    headline = readiness.cause
      ? intl.formatMessage(i18n.leaving, {
          peer,
          cause: readiness.cause,
          turn: turnInFlight ? 'yes' : 'no',
        })
      : intl.formatMessage(i18n.reconnecting, { peer });
    fill = PHASE_FILL.loading;
    spinning = true;
    // Plain words on the bar; the failed read's own words ("timeout: no answer within 1500 ms",
    // the mesh's URL) only behind Details, as the transcript's dropped-turn notice keeps them.
    // Nothing promises the answer continues: the relay can tell only once the Mac answers (Q-52).
    detail = switchError
      ? intl.formatMessage(i18n.switchFailed, { error: switchError })
      : readiness.cause
        ? intl.formatMessage(i18n.leavingPlain)
        : intl.formatMessage(i18n.reconnectingPlain, { peer });
    raw = readiness.why;
    action = runHereButton(peer);
  } else if (readiness.kind === 'remote') {
    const peer = routePeerName(readiness.status);
    detail = readiness.status.lastError ?? null;
    if (readiness.status.state === 'failed') {
      headline = intl.formatMessage(i18n.remoteFailed, { peer });
      fill = PHASE_FILL.failed;
    } else {
      headline = model
        ? intl.formatMessage(i18n.remoteLoading, { model: shortModelName(model), peer })
        : intl.formatMessage(i18n.remoteLoadingUnnamed, { peer });
      fill = PHASE_FILL.loading;
      spinning = true;
      if (switchError) detail = intl.formatMessage(i18n.switchFailed, { error: switchError });
      // Loading there can take long: loading here stays one click away (Q-57).
      action = runHereButton(peer);
    }
  } else if (readiness.kind === 'distributed') {
    const { status: dist, wanted } = readiness;
    headline = intl.formatMessage(i18n.distributed, {
      mode: formatMlxMode(intl, distributedSummary(dist), null),
      state: distributedStateLabel(intl, dist),
      nodes: readiness.nodes.join(', '),
    });
    const served = distributedServedId(dist);
    if (distributedServes(dist) && wanted != null && served != null && served !== wanted) {
      detail = intl.formatMessage(i18n.distributedMismatch, { served, wanted });
    } else {
      detail = distributedProblem(dist);
    }
    if (dist.state === 'preflight' || dist.state === 'starting') {
      fill = PHASE_FILL.loading;
      spinning = true;
    }
  } else {
    headline = intl.formatMessage(i18n.unmounted, { nodes: readiness.nodes.join(', ') });
    const { target, fact } = readiness;
    const failure =
      mountError ??
      (fact === 'failed'
        ? (status?.lastError ?? status?.gateMessage ?? status?.probeError ?? null)
        : null);
    if (failure) detail = intl.formatMessage(i18n.failed, { error: failure });
    if (target.kind === 'mismatch') {
      detail = intl.formatMessage(i18n.mismatch, { served: target.served, wanted: target.wanted });
    } else if (target.kind === 'none') {
      detail = intl.formatMessage(i18n.noTarget);
    } else if (fact === 'mounting' || requesting) {
      spinning = true;
      action = (
        <span
          data-testid="composer-readiness-mounting"
          title={target.modelId}
          className={cx('inline-flex items-center gap-1.5', TYPE.meta, 'text-white')}
        >
          {intl.formatMessage(i18n.mounting, { model: shortModelName(target.modelId) })}
        </span>
      );
    } else {
      action = (
        <Button
          size="sm"
          variant="secondary"
          data-testid="composer-readiness-mount"
          title={target.modelId}
          onClick={() => onMount(target.modelId)}
        >
          {intl.formatMessage(i18n.mount, { model: shortModelName(target.modelId) })}
        </Button>
      );
    }
  }

  return (
    <div
      role="status"
      data-testid="composer-readiness"
      data-readiness={readiness.kind}
      className={cx(
        'mb-2 flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2',
        RADIUS.control,
        fill
      )}
    >
      {spinning ? (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-spinner"
          data-for={readiness.kind}
          className="size-4 shrink-0 animate-spin"
        />
      ) : readiness.kind === 'remote' ? (
        <Network aria-hidden className="size-4 shrink-0" />
      ) : (
        <ServerOff aria-hidden className="size-4 shrink-0" />
      )}
      <div className="flex min-w-0 flex-1 flex-col">
        <span className={cx('text-lz-body', WEIGHT.semibold)}>{headline}</span>
        {detail && (
          <span data-testid="composer-readiness-detail" className="text-lz-meta break-words">
            {detail}
          </span>
        )}
        {raw && <RawReason label={intl.formatMessage(i18n.details)} raw={raw} />}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {action}
        <OpenEngineButton />
      </div>
    </div>
  );
}

/**
 * The failed read's own words, folded: a small toggle in the bar's own ink (the lz Disclosure draws
 * the page's ink, which the solid amber fill does not carry in the dark theme).
 */
function RawReason({ label, raw }: { label: string; raw: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-col items-start">
      <button
        type="button"
        aria-expanded={open}
        data-testid="composer-readiness-details"
        onClick={() => setOpen((o) => !o)}
        className={cx('inline-flex items-center gap-1 text-lz-meta underline', WEIGHT.semibold)}
      >
        <ChevronRight aria-hidden className={cx('size-3.5', open && 'rotate-90')} />
        {label}
      </button>
      {open && (
        <span data-testid="composer-readiness-raw" className="text-lz-meta break-words font-mono">
          {raw}
        </span>
      )}
    </div>
  );
}

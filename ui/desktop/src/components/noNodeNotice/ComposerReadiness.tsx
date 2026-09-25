import { useCallback, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { Hourglass, Laptop, Loader2, Network, ServerOff, Settings2 } from 'lucide-react';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import { mlxRemoteSingleStop } from '../../acp/mlx-remote-single';
import { toastError } from '../../toasts';
import { errorMessage } from '../../utils/conversionUtils';
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
  reconnectingWhy: {
    id: 'composerReadiness.reconnectingWhy',
    defaultMessage: 'Last read: {why}',
  },
  runHere: { id: 'composerReadiness.runHere', defaultMessage: 'Run on this Mac instead' },
  switchingHere: {
    id: 'composerReadiness.switchingHere',
    defaultMessage: 'Moving chat to this Mac…',
  },
  switchFailed: {
    id: 'composerReadiness.switchFailed',
    defaultMessage: 'Could not move chat to this Mac: {error}',
  },
  peerKeptModel: {
    id: 'composerReadiness.peerKeptModel',
    defaultMessage: '{peer} kept its model loaded',
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
  const intl = useIntl();
  const { served, single, armed } = serving;
  const { readiness } = served;
  const { requestingNodeId, mountErrors, mount } = useMlxMount(single);
  const restore = useRestoreLine();
  const restoreText = restoreLineText(intl, restore);
  const [switching, setSwitching] = useState(false);
  const [switchError, setSwitchError] = useState<string | null>(null);

  // "Run on this Mac instead": the route's own Stop (the tray's and Run it's), then this Mac's Mount
  // — the bar's Mount path. A peer that cannot be reached keeps its model, and goose says so.
  const runHere = useCallback(
    async (instead: RunHere, peer: string) => {
      if (instead.kind !== 'switch') return;
      setSwitching(true);
      setSwitchError(null);
      try {
        const { unmountError } = await mlxRemoteSingleStop(false);
        if (unmountError) {
          toastError({
            title: intl.formatMessage(i18n.peerKeptModel, { peer }),
            msg: unmountError,
          });
        }
        if (instead.mount) await mount(STRIP_MOUNT_KEY, instead.mount);
      } catch (e) {
        setSwitchError(errorMessage(e, String(e)));
      } finally {
        setSwitching(false);
      }
    },
    [intl, mount]
  );

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
      onRunHere={(instead, peer) => void runHere(instead, peer)}
    />
  );
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
}: {
  readiness: Exclude<ComposerReadiness, { kind: 'unknown' } | { kind: 'ready' }>;
  model: string | null;
  status: MlxEngineStatus | null;
  requesting: boolean;
  mountError: string | null;
  onMount: (modelId: string) => void;
  switching: boolean;
  switchError: string | null;
  onRunHere: (instead: RunHere, peer: string) => void;
}) {
  const intl = useIntl();

  let headline: string;
  let detail: string | null = null;
  let action: ReactNode = null;
  let fill = TONE_FILL.warn;

  if (readiness.kind === 'no-nodes') {
    headline = intl.formatMessage(i18n.noNodes);
  } else if (readiness.kind === 'reconnecting') {
    // The route's Mac stopped answering: chat still goes there, so every second until it answers
    // again — or the user moves chat here — is named (Q-47).
    const peer = routePeerName(readiness.status);
    headline = intl.formatMessage(i18n.reconnecting, { peer });
    fill = PHASE_FILL.loading;
    detail = switchError
      ? intl.formatMessage(i18n.switchFailed, { error: switchError })
      : readiness.why
        ? intl.formatMessage(i18n.reconnectingWhy, { why: readiness.why })
        : null;
    const { instead } = readiness;
    if (instead.kind === 'switch') {
      action = (
        <Button
          size="sm"
          variant="secondary"
          icon={switching ? <Loader2 className="animate-spin" /> : <Laptop />}
          disabled={switching}
          data-testid="composer-readiness-run-here"
          onClick={() => onRunHere(instead, peer)}
        >
          {intl.formatMessage(switching ? i18n.switchingHere : i18n.runHere)}
        </Button>
      );
    }
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
      action = (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-remote-mounting"
          className="size-4 animate-spin"
        />
      );
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
      action = (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-distributed-starting"
          className="size-4 animate-spin"
        />
      );
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
      action = (
        <span
          data-testid="composer-readiness-mounting"
          title={target.modelId}
          className={cx('inline-flex items-center gap-1.5', TYPE.meta, 'text-white')}
        >
          <Loader2 className="size-3.5 animate-spin" />
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
      {readiness.kind === 'reconnecting' ? (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-reconnecting"
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
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {action}
        <OpenEngineButton />
      </div>
    </div>
  );
}

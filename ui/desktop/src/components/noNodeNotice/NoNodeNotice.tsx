import { useCallback, useState, useSyncExternalStore } from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowLeftRight, Loader2, RotateCcw, ServerOff, Settings2 } from 'lucide-react';
import { useConfig } from '../ConfigContext';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import type { SwarmConfig } from '../settings/swarm/golden';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Chip,
  Disclosure,
  SURFACE,
  SPACE,
  StatusDot,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { splitStopAt, type SplitStop } from '../chatServedBy/splitStop';
import { refusalStopFromRecord, type SplitRecord } from '../chatServedBy/splitRecord';
import { routeServesChat } from '../chatServedBy/chatServedBy';
import { splitStopReason } from '../chatServedBy/splitStopText';
import { ENGINE_ROUTE } from './ComposerReadiness';
import type { NoNodeRow, NodeReason } from './parseNoNodeError';
import { formatMlxMode } from '../leanzero-swarm/mlxModeLabel';
import { routePeerName } from '../leanzero-swarm/macs';
import { setNodeModel } from '../leanzero-swarm/nodes';
import { errorMessage } from '../../utils/conversionUtils';
import {
  latestMlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
  type MlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import {
  distributedFact,
  distributedServedId,
  distributedServes,
  distributedStateLabel,
  distributedSummary,
  engineFact,
  nodeServedId,
  resolveMountTarget,
  shortModelName,
  useLatestMlxDistributedStatus,
  useMlxMount,
  useMountLookup,
  type EngineFact,
  type MountTarget,
} from './mlxMount';

const i18n = defineMessages({
  titleUnmounted: { id: 'noNodeNotice.titleUnmounted', defaultMessage: 'No model is mounted' },
  titleNoNode: { id: 'noNodeNotice.titleNoNode', defaultMessage: 'No node can answer' },
  summary: {
    id: 'noNodeNotice.summary',
    defaultMessage: 'The swarm checked every enabled node and none could take this message.',
  },
  mlxDown: {
    id: 'noNodeNotice.mlxDown',
    defaultMessage: 'The MLX engine is not running — nothing answers at {base}.',
  },
  mlxWrongModel: {
    id: 'noNodeNotice.mlxWrongModel',
    defaultMessage: 'The MLX engine serves {served}, but this node is set to {wanted}.',
  },
  routedRemote: {
    id: 'noNodeNotice.routedRemote',
    defaultMessage:
      'Serving from {peer} — this Mac’s own engine is set aside while chat goes there.',
  },
  remoteDown: {
    id: 'noNodeNotice.remoteDown',
    defaultMessage: 'Serving from {peer}, but its engine is not answering through LeanZero Link.',
  },
  lmUnreachable: {
    id: 'noNodeNotice.lmUnreachable',
    defaultMessage: 'Nothing answers at {url} — the server this node points to is not running.',
  },
  lmNotListed: {
    id: 'noNodeNotice.lmNotListed',
    defaultMessage: '{model} is not loaded on the server this node points to.',
  },
  busy: { id: 'noNodeNotice.busy', defaultMessage: 'Every slot on this node is busy.' },
  noDevices: { id: 'noNodeNotice.noDevices', defaultMessage: 'No enabled node is configured.' },
  mount: { id: 'noNodeNotice.mount', defaultMessage: 'Mount {model}' },
  mounting: { id: 'noNodeNotice.mounting', defaultMessage: 'Mounting' },
  mounted: { id: 'noNodeNotice.mounted', defaultMessage: 'Mounted' },
  distributedState: {
    id: 'noNodeNotice.distributedState',
    defaultMessage: '{mode} · {state}',
  },
  distributedWrongModel: {
    id: 'noNodeNotice.distributedWrongModel',
    defaultMessage: 'The distributed engine serves {served}; this node wants {wanted}.',
  },
  mountFailed: { id: 'noNodeNotice.mountFailed', defaultMessage: 'Mount failed' },
  aliasMismatch: {
    id: 'noNodeNotice.aliasMismatch',
    defaultMessage:
      'The MLX engine’s saved model serves {served}; this node wants {wanted}. Pick the model in Providers.',
  },
  noMountTarget: {
    id: 'noNodeNotice.noMountTarget',
    defaultMessage: 'No saved MLX model for this node — pick one in Providers.',
  },
  lookupFailed: {
    id: 'noNodeNotice.lookupFailed',
    defaultMessage: 'Could not read which model this node mounts: {error}',
  },
  openProviders: { id: 'noNodeNotice.openProviders', defaultMessage: 'Open Providers' },
  chatWith: { id: 'noNodeNotice.chatWith', defaultMessage: 'Chat with {model}' },
  chatWithDone: {
    id: 'noNodeNotice.chatWithDone',
    defaultMessage: 'This node now chats with {model} — retry to send your message.',
  },
  chatWithFailed: {
    id: 'noNodeNotice.chatWithFailed',
    defaultMessage: 'Could not point this node at {model}: {error}',
  },
  retry: { id: 'noNodeNotice.retry', defaultMessage: 'Retry' },
  ready: {
    id: 'noNodeNotice.ready',
    defaultMessage: 'The node is up — retry to send your message again.',
  },
  splitTitle: {
    id: 'noNodeNotice.splitTitle',
    defaultMessage:
      '{answer, select, yes {The split across your Macs stopped mid-answer} other {The split across your Macs stopped}}',
  },
  splitSummary: {
    id: 'noNodeNotice.splitSummary',
    defaultMessage:
      '{answer, select, yes {{reason} — the answer above stops there.} other {{reason} — nothing else could take this message.}}',
  },
  splitBack: {
    id: 'noNodeNotice.splitBack',
    defaultMessage: 'The model is running again — retry to send your message.',
  },
  splitSummaryCut: {
    id: 'noNodeNotice.splitSummaryCut',
    defaultMessage: '{reason} — the answer was cut before any of it was written.',
  },
  servedElsewhere: {
    id: 'noNodeNotice.servedElsewhere',
    defaultMessage: 'Chat goes to {peer} now',
  },
  openEngine: { id: 'noNodeNotice.openEngine', defaultMessage: 'Open Engine' },
  details: { id: 'noNodeNotice.details', defaultMessage: 'Details' },
});

/**
 * The refusal of a turn that met the split already stopped — or an answer the stop CUT (Q-81,
 * Q-122): said as the split and why, never as "no model is mounted" on this Mac's single engine,
 * which chat was not on, nor as a bare "Network error". Retry resends; the router's words about
 * this Mac's engine port, the stream's error and the supervisor's own words stay behind Details.
 * Starting the split again or running on one Mac is the composer bar's (one place for the actions).
 */
export function SplitStoppedNotice({
  stop,
  rows,
  hasAnswer,
  cut = false,
  errorText = null,
  live,
  back,
  retryText,
  onRetry,
}: {
  stop: SplitStop;
  rows: NoNodeRow[];
  hasAnswer: boolean;
  /** The stop cut this turn's own stream (it was not refused afterwards). */
  cut?: boolean;
  /** The failure as goose wrote it (the stream's error), for Details. */
  errorText?: string | null;
  live: boolean;
  back: boolean;
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const answer = hasAnswer || cut ? 'yes' : 'no';
  const reason = splitStopReason(intl, stop);
  const summary =
    cut && !hasAnswer
      ? intl.formatMessage(i18n.splitSummaryCut, { reason })
      : intl.formatMessage(i18n.splitSummary, { answer, reason });
  return (
    <div
      role="alert"
      data-testid="no-node-split-stopped"
      className={cx(SURFACE.card, SPACE.card, hasAnswer && 'mt-2', 'flex flex-col gap-3')}
    >
      <div className="flex items-start gap-2.5">
        <ServerOff aria-hidden className={cx('mt-0.5 size-5 shrink-0', TONE_TEXT.err)} />
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className={cx(TYPE.body, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.splitTitle, { answer })}
          </h3>
          <p data-testid="no-node-split-summary" className={TYPE.body}>
            {summary}
          </p>
        </div>
      </div>
      {live && back && (
        <p className={cx(TYPE.body, TONE_TEXT.ok)}>{intl.formatMessage(i18n.splitBack)}</p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        {live && retryText != null && (
          <Button
            variant={back ? 'primary' : 'secondary'}
            size="sm"
            icon={<RotateCcw />}
            data-testid="no-node-retry"
            onClick={() => onRetry(retryText)}
          >
            {intl.formatMessage(i18n.retry)}
          </Button>
        )}
        <Button
          variant="secondary"
          size="sm"
          icon={<Settings2 />}
          data-testid="no-node-open-engine"
          onClick={() => navigate(ENGINE_ROUTE)}
        >
          {intl.formatMessage(i18n.openEngine)}
        </Button>
      </div>
      <Disclosure variant="plain" testId="no-node-details" title={intl.formatMessage(i18n.details)}>
        <div className="flex flex-col gap-1">
          {rows.map((row, i) => (
            <p
              key={`${row.nodeId ?? 'pool'}-${i}`}
              data-testid="no-node-raw"
              className={cx(TYPE.meta, 'break-words font-mono')}
            >
              {row.nodeId ? `${row.nodeId}: ${row.raw}` : row.raw}
            </p>
          ))}
          {errorText && (
            <p
              data-testid="no-node-split-error"
              className={cx(TYPE.meta, 'whitespace-pre-wrap break-words font-mono')}
            >
              {errorText}
            </p>
          )}
          <p data-testid="no-node-split-raw" className={cx(TYPE.meta, 'break-words font-mono')}>
            {stop.raw}
          </p>
        </div>
      </Disclosure>
    </div>
  );
}

/** A message's `created` is whole seconds. */
const SECOND_MS = 1000;

/**
 * The router names a remote route's peer by its mesh hostname (its reason text is parsed on
 * whitespace); the person reads the one name — `routePeerName` over the route this window knows,
 * when it is the same Mac. A route this window no longer knows keeps the router's word.
 */
export function routedPeerName(host: string, route: MlxRemoteSingleStatus | null): string {
  return route && (route.peerHostname === host || route.peer === host)
    ? routePeerName(route)
    : host;
}

function reasonText(
  intl: ReturnType<typeof useIntl>,
  reason: NodeReason,
  route: MlxRemoteSingleStatus | null
): string | null {
  switch (reason.kind) {
    case 'mlx-down':
      return intl.formatMessage(i18n.mlxDown, { base: reason.base });
    case 'mlx-wrong-model':
      return intl.formatMessage(i18n.mlxWrongModel, {
        served: reason.served,
        wanted: reason.wanted,
      });
    case 'mlx-routed-remote':
      return intl.formatMessage(i18n.routedRemote, { peer: routedPeerName(reason.peer, route) });
    case 'mlx-remote-down':
      return intl.formatMessage(i18n.remoteDown, { peer: routedPeerName(reason.peer, route) });
    case 'lm-unreachable':
      return intl.formatMessage(i18n.lmUnreachable, { url: reason.url });
    case 'lm-not-listed':
      return intl.formatMessage(i18n.lmNotListed, { model: reason.model });
    case 'busy':
      return intl.formatMessage(i18n.busy);
    case 'no-devices':
      return intl.formatMessage(i18n.noDevices);
    case 'other':
      return null;
  }
}

/**
 * The swarm router's "no node can serve this turn" refusal, as a notice instead of raw text: one row
 * per node with its reason in plain words AND the router's own words verbatim beneath (an unknown
 * reason renders verbatim only — never hidden), a Mount action for a local MLX node whose engine is
 * down, Open Providers, and Retry (resends the last user turn's text).
 *
 * `live` is true only while this refusal is the conversation's latest message: an older notice in
 * the history is a record, so it neither polls the engine nor offers Mount/Retry. The Mount row reads
 * the engine's LIVE status — once the engine serves the node's model the row says Mounted, whatever
 * the historical reason said.
 */
export default function NoNodeNotice({
  rows,
  live,
  retryText,
  onRetry,
  createdMs = null,
  hasAnswer = false,
  splitRecord = null,
}: {
  rows: NoNodeRow[];
  live: boolean;
  /**
   * When the turn's message was written (ms); null = unknown, and no split stop is claimed. With
   * `hasAnswer` it is when the CUT answer began.
   */
  createdMs?: number | null;
  /** The model wrote part of an answer before this refusal — the notice renders below it. */
  hasAnswer?: boolean;
  /**
   * The split's record goose saved WITH this refusal (Q-121): the split stop is read from it, so
   * history says what the live notice said after the split's in-memory events are gone. null = none
   * saved (an older message, or no split in that goosed) — the live events are read, as before.
   */
  splitRecord?: SplitRecord | null;
  /** The last user turn's text; null when there is none that can be resent faithfully. */
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const { read, upsert } = useConfig();
  const [repointed, setRepointed] = useState<
    Record<string, { state: 'writing' | 'done' } | { state: 'failed'; error: string }>
  >({});
  const chatWith = useCallback(
    async (nodeId: string, served: string) => {
      setRepointed((prev) => ({ ...prev, [nodeId]: { state: 'writing' } }));
      try {
        await setNodeModel({ read, upsert }, nodeId, served);
        setRepointed((prev) => ({ ...prev, [nodeId]: { state: 'done' } }));
      } catch (e) {
        setRepointed((prev) => ({
          ...prev,
          [nodeId]: { state: 'failed', error: errorMessage(e, String(e)) },
        }));
      }
    },
    [read, upsert]
  );
  const mlxDown = rows.some((r) => r.reason.kind === 'mlx-down' && r.nodeId != null);
  const armed = live && mlxDown;

  const readSwarm = useCallback(
    () => read('swarm', false, { throwOnError: true }) as Promise<SwarmConfig | null>,
    [read]
  );
  const lookup = useMountLookup(armed, readSwarm, read);
  const { status } = useMlxEngineStatusPoll(armed, 2000);
  const distributed = useLatestMlxDistributedStatus();
  const route = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  const { requestingNodeId, mountErrors, mount: onMount } = useMlxMount(status);

  const targetOf = (nodeId: string): MountTarget | null =>
    lookup.state === 'ready' ? resolveMountTarget(nodeId, lookup.devices, lookup.settings) : null;
  /** The id the node is served under while the split answers — the router's chat rule. */
  const nodeModelOf = (nodeId: string): string | null => {
    if (lookup.state !== 'ready') return null;
    const device = lookup.devices.find((d) => d.id === nodeId);
    if (!device) return null;
    return nodeServedId(lookup, device, distributed ? distributedServedId(distributed) : null);
  };

  const liveFactOf = (row: NoNodeRow): EngineFact | null => {
    if (!armed || row.reason.kind !== 'mlx-down' || row.nodeId == null) return null;
    const owned = distributedFact(distributed, nodeModelOf(row.nodeId));
    if (owned) return owned;
    const target = targetOf(row.nodeId);
    return engineFact(status, target?.kind === 'ok' ? target.servedId : null);
  };
  const mlxFacts = rows.map(liveFactOf).filter((f): f is EngineFact => f != null);
  const anyMounting = requestingNodeId != null || mlxFacts.includes('mounting');
  const allUp = mlxFacts.length > 0 && mlxFacts.every((f) => f === 'up');
  const routeReady = routeServesChat(route, distributed) && route.state === 'ready';
  const anyRepointed = Object.values(repointed).some((r) => r.state === 'done');
  // A node set to another model than the engine serves refuses a plain retry the same way: its
  // "Chat with …" is the move until it is taken.
  const wrongModel = rows.some((r) => r.reason.kind === 'mlx-wrong-model' && r.nodeId != null);
  const retryPrimary = (!mlxDown && !wrongModel) || allUp || routeReady || anyRepointed;

  // Chat was on the split and the split had stopped (or stopped under this very answer): that is
  // the fact, not "no model is mounted" (Q-81). The message's time is floored to the second. The
  // record saved with the message wins over the live events, which a relaunch empties (Q-121).
  const deathBy = createdMs == null || hasAnswer ? null : createdMs + SECOND_MS - 1;
  const splitStop =
    !mlxDown || createdMs == null
      ? null
      : splitRecord
        ? refusalStopFromRecord(splitRecord, createdMs, deathBy)
        : splitStopAt(distributed, createdMs, deathBy);
  // Chat goes to another Mac's engine now (the Studio route): mounting THIS Mac's engine is not
  // what serves chat, so it is never offered (Q-121) — the row says where chat goes instead.
  const servedElsewhere = routeServesChat(route, distributed) ? route : null;
  if (splitStop) {
    return (
      <SplitStoppedNotice
        stop={splitStop}
        rows={rows}
        hasAnswer={hasAnswer}
        live={live}
        back={allUp}
        retryText={retryText}
        onRetry={onRetry}
      />
    );
  }

  const renderMlxAction = (row: NoNodeRow) => {
    if (!armed || row.nodeId == null) return null;
    const nodeId = row.nodeId;
    if (servedElsewhere) {
      return (
        <span data-testid={`no-node-served-elsewhere-${nodeId}`}>
          <Chip tone={servedElsewhere.state === 'ready' ? 'ok' : 'warn'}>
            {intl.formatMessage(i18n.servedElsewhere, { peer: routePeerName(servedElsewhere) })}
          </Chip>
        </span>
      );
    }
    if (lookup.state === 'loading') {
      return <Loader2 aria-label="loading" className="size-4 animate-spin text-lz-ink-3" />;
    }
    if (lookup.state === 'failed') {
      return (
        <p className={cx(TYPE.meta, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.lookupFailed, { error: lookup.error })}
        </p>
      );
    }
    // The distributed engine owns this Mac: it is the node's engine, a single mount would be
    // refused — say which engine and its state, offer no Mount.
    const owned = distributedFact(distributed, nodeModelOf(nodeId));
    if (owned && distributed) {
      const label = intl.formatMessage(i18n.distributedState, {
        mode: formatMlxMode(intl, distributedSummary(distributed), null),
        state: distributedStateLabel(intl, distributed),
      });
      const wanted = nodeModelOf(nodeId);
      const served = distributedServedId(distributed);
      const wrongModel =
        distributedServes(distributed) && served != null && wanted != null && served !== wanted;
      return (
        <div
          data-testid={`no-node-distributed-${nodeId}`}
          className="flex min-w-0 flex-col items-end gap-1.5"
        >
          <Chip
            tone={owned === 'up' ? 'ok' : 'warn'}
            icon={owned === 'mounting' ? <Loader2 className="animate-spin" /> : undefined}
            title={served ?? undefined}
          >
            {label}
          </Chip>
          {wrongModel && (
            <p className={cx(TYPE.meta, 'break-words text-right')}>
              {intl.formatMessage(i18n.distributedWrongModel, { served, wanted })}
            </p>
          )}
        </div>
      );
    }
    const target = resolveMountTarget(nodeId, lookup.devices, lookup.settings);
    if (target.kind === 'mismatch') {
      return (
        <p className={TYPE.meta}>
          {intl.formatMessage(i18n.aliasMismatch, { served: target.served, wanted: target.wanted })}
        </p>
      );
    }
    if (target.kind === 'none') {
      return <p className={TYPE.meta}>{intl.formatMessage(i18n.noMountTarget)}</p>;
    }
    const fact = engineFact(status, target.servedId);
    const failure =
      mountErrors[nodeId] ??
      (fact === 'failed'
        ? (status?.lastError ?? status?.gateMessage ?? status?.probeError ?? null)
        : null);
    if (fact === 'up') {
      return (
        <Chip tone="ok" title={target.servedId}>
          {intl.formatMessage(i18n.mounted)}
        </Chip>
      );
    }
    if (fact === 'mounting' || requestingNodeId === nodeId) {
      return (
        <Chip tone="warn" icon={<Loader2 className="animate-spin" />} title={target.modelId}>
          {intl.formatMessage(i18n.mounting)}
        </Chip>
      );
    }
    return (
      <div className="flex min-w-0 flex-col items-end gap-1.5">
        <Button
          variant="primary"
          size="sm"
          data-testid={`no-node-mount-${nodeId}`}
          title={target.modelId}
          onClick={() => onMount(nodeId, target.modelId)}
        >
          {intl.formatMessage(i18n.mount, { model: shortModelName(target.modelId) })}
        </Button>
        {(fact === 'failed' || mountErrors[nodeId]) && (
          <p
            data-testid={`no-node-mount-error-${nodeId}`}
            className={cx(TYPE.meta, TONE_TEXT.err, 'break-words text-right')}
          >
            {intl.formatMessage(i18n.mountFailed)}
            {failure ? ` — ${failure}` : ''}
          </p>
        )}
      </div>
    );
  };

  /**
   * A real mismatch the router left standing — the engine serves a model this node is not set to
   * and nobody here started it — fixed in one click: the node is pointed at what serves, through
   * the same writer as the Nodes table's model picker. The router's word, the served id, is what
   * is written, so the next turn's probe names it exactly.
   */
  const renderWrongModelAction = (row: NoNodeRow) => {
    if (!live || row.nodeId == null || row.reason.kind !== 'mlx-wrong-model') return null;
    const nodeId = row.nodeId;
    const { served } = row.reason;
    const model = shortModelName(served);
    const state = repointed[nodeId];
    if (state?.state === 'done') {
      return (
        <p
          data-testid={`no-node-chat-with-done-${nodeId}`}
          className={cx(TYPE.meta, TONE_TEXT.ok, 'break-words text-right')}
        >
          {intl.formatMessage(i18n.chatWithDone, { model })}
        </p>
      );
    }
    return (
      <div className="flex min-w-0 flex-col items-end gap-1.5">
        <Button
          variant="primary"
          size="sm"
          icon={
            state?.state === 'writing' ? <Loader2 className="animate-spin" /> : <ArrowLeftRight />
          }
          disabled={state?.state === 'writing'}
          data-testid={`no-node-chat-with-${nodeId}`}
          title={served}
          onClick={() => void chatWith(nodeId, served)}
        >
          {intl.formatMessage(i18n.chatWith, { model })}
        </Button>
        {state?.state === 'failed' && (
          <p
            data-testid={`no-node-chat-with-error-${nodeId}`}
            className={cx(TYPE.meta, TONE_TEXT.err, 'break-words text-right')}
          >
            {intl.formatMessage(i18n.chatWithFailed, { model, error: state.error })}
          </p>
        )}
      </div>
    );
  };

  return (
    <div
      data-testid="no-node-notice"
      className={cx(SURFACE.card, SPACE.card, hasAnswer && 'mt-2', 'flex flex-col gap-3')}
    >
      <div className="flex items-start gap-2.5">
        <ServerOff className={cx('mt-0.5 size-5 shrink-0', TONE_TEXT.err)} />
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className={cx(TYPE.body, WEIGHT.semibold)}>
            {intl.formatMessage(mlxDown ? i18n.titleUnmounted : i18n.titleNoNode)}
          </h3>
          <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.summary)}</p>
        </div>
      </div>

      <ul className={cx('flex flex-col divide-y divide-lz-border border-y', SURFACE.hairline)}>
        {rows.map((row, i) => {
          const plain = reasonText(intl, row.reason, route);
          const isMlx = row.reason.kind === 'mlx-down';
          const fact = liveFactOf(row);
          const dot = fact === 'up' ? 'ok' : fact === 'mounting' ? 'warn' : 'err';
          return (
            <li
              key={`${row.nodeId ?? 'pool'}-${i}`}
              data-testid={`no-node-row-${row.nodeId ?? 'pool'}`}
              className="flex items-start gap-3 py-2.5"
            >
              <StatusDot tone={dot} label={row.nodeId ?? 'pool'} className="mt-1.5" />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                {row.nodeId && <span className={cx(TYPE.mono, WEIGHT.medium)}>{row.nodeId}</span>}
                {plain && <span className={TYPE.body}>{plain}</span>}
                <span
                  data-testid="no-node-raw"
                  className={cx(plain ? TYPE.meta : TYPE.body, 'break-words font-mono')}
                >
                  {row.raw}
                </span>
              </div>
              {isMlx && (
                <div className="flex max-w-[45%] shrink-0 justify-end">{renderMlxAction(row)}</div>
              )}
              {row.reason.kind === 'mlx-wrong-model' && live && (
                <div className="flex max-w-[45%] shrink-0 justify-end">
                  {renderWrongModelAction(row)}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {live && allUp && (
        <p className={cx(TYPE.body, TONE_TEXT.ok)}>{intl.formatMessage(i18n.ready)}</p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {live && retryText != null && (
          <Button
            variant={retryPrimary ? 'primary' : 'secondary'}
            size="sm"
            icon={<RotateCcw />}
            disabled={anyMounting}
            data-testid="no-node-retry"
            onClick={() => onRetry(retryText)}
          >
            {intl.formatMessage(i18n.retry)}
          </Button>
        )}
        <Button
          variant="secondary"
          size="sm"
          icon={<Settings2 />}
          data-testid="no-node-open-providers"
          onClick={() => navigate('/leanzero-swarm')}
        >
          {intl.formatMessage(i18n.openProviders)}
        </Button>
      </div>
    </div>
  );
}

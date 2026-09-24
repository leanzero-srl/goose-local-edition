import { useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Loader2, RotateCcw, ServerOff, Settings2 } from 'lucide-react';
import { useConfig } from '../ConfigContext';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import type { SwarmConfig } from '../settings/swarm/golden';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, SURFACE, SPACE, StatusDot, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import type { NoNodeRow, NodeReason } from './parseNoNodeError';
import { formatMlxMode } from '../leanzero-swarm/mlxModeLabel';
import {
  distributedFact,
  distributedServedId,
  distributedServes,
  distributedStateLabel,
  distributedSummary,
  engineFact,
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
    defaultMessage: 'Nothing answers at {url} — LM Studio may not be running.',
  },
  lmNotListed: {
    id: 'noNodeNotice.lmNotListed',
    defaultMessage: '{model} is not loaded in LM Studio.',
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
  retry: { id: 'noNodeNotice.retry', defaultMessage: 'Retry' },
  ready: {
    id: 'noNodeNotice.ready',
    defaultMessage: 'The node is up — retry to send your message again.',
  },
});

function reasonText(intl: ReturnType<typeof useIntl>, reason: NodeReason): string | null {
  switch (reason.kind) {
    case 'mlx-down':
      return intl.formatMessage(i18n.mlxDown, { base: reason.base });
    case 'mlx-wrong-model':
      return intl.formatMessage(i18n.mlxWrongModel, {
        served: reason.served,
        wanted: reason.wanted,
      });
    case 'mlx-routed-remote':
      return intl.formatMessage(i18n.routedRemote, { peer: reason.peer });
    case 'mlx-remote-down':
      return intl.formatMessage(i18n.remoteDown, { peer: reason.peer });
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
}: {
  rows: NoNodeRow[];
  live: boolean;
  /** The last user turn's text; null when there is none that can be resent faithfully. */
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const { read } = useConfig();
  const mlxDown = rows.some((r) => r.reason.kind === 'mlx-down' && r.nodeId != null);
  const armed = live && mlxDown;

  const readSwarm = useCallback(
    () => read('swarm', false, { throwOnError: true }) as Promise<SwarmConfig | null>,
    [read]
  );
  const lookup = useMountLookup(armed, readSwarm, read);
  const { status } = useMlxEngineStatusPoll(armed, 2000);
  const distributed = useLatestMlxDistributedStatus();
  const { requestingNodeId, mountErrors, mount: onMount } = useMlxMount(status);

  const targetOf = (nodeId: string): MountTarget | null =>
    lookup.state === 'ready' ? resolveMountTarget(nodeId, lookup.devices, lookup.settings) : null;
  const nodeModelOf = (nodeId: string): string | null =>
    lookup.state === 'ready'
      ? (lookup.devices.find((d) => d.id === nodeId)?.model_id ?? null)
      : null;

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
  const retryPrimary = !mlxDown || allUp;

  const renderMlxAction = (row: NoNodeRow) => {
    if (!armed || row.nodeId == null) return null;
    const nodeId = row.nodeId;
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

  return (
    <div
      data-testid="no-node-notice"
      className={cx(SURFACE.card, SPACE.card, 'flex flex-col gap-3')}
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
          const plain = reasonText(intl, row.reason);
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

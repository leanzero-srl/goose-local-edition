import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Loader2, RotateCcw, ServerOff, Settings2 } from 'lucide-react';
import { useConfig } from '../ConfigContext';
import {
  mlxEngineMount,
  mlxEngineSettingsRead,
  type MlxEngineSettings,
  type MlxEngineStatus,
} from '../../acp/mlx-engine';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import { errorMessage } from '../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, SURFACE, SPACE, StatusDot, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import type { NoNodeRow, NodeReason } from './parseNoNodeError';

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

/**
 * What a "Mount" click would mount for a swarm device: the engine's persisted HF model, but only
 * when that model is served under the alias the device names (mlx_engine.served_model_name, else the
 * HF id itself). Anything else is stated, never guessed — mounting a different model would leave the
 * node refusing the next turn for the same reason.
 */
export type MountTarget =
  | { kind: 'ok'; modelId: string; servedId: string }
  | { kind: 'mismatch'; served: string; wanted: string }
  | { kind: 'none' };

export function resolveMountTarget(
  nodeId: string,
  devices: SwarmDeviceRow[],
  settings: MlxEngineSettings
): MountTarget {
  const device = devices.find((d) => d.id === nodeId);
  if (!device || device.engine !== 'mlx-sidecar' || device.host != null || !settings.modelId) {
    return { kind: 'none' };
  }
  const served = settings.servedModelName || settings.modelId;
  if (served !== device.model_id) {
    return { kind: 'mismatch', served, wanted: device.model_id };
  }
  return { kind: 'ok', modelId: settings.modelId, servedId: served };
}

type Lookup =
  | { state: 'loading' }
  | { state: 'failed'; error: string }
  | { state: 'ready'; devices: SwarmDeviceRow[]; settings: MlxEngineSettings };

type EngineFact = 'up' | 'mounting' | 'failed' | 'down';

function engineFact(status: MlxEngineStatus | null, servedId: string | null): EngineFact {
  if (!status) return 'down';
  if (status.state === 'mounting') return 'mounting';
  if (status.state === 'failed') return 'failed';
  const serving = status.servedModelId ?? status.modelId;
  if (status.state === 'running' && servedId != null && serving === servedId) return 'up';
  return 'down';
}

function reasonText(intl: ReturnType<typeof useIntl>, reason: NodeReason): string | null {
  switch (reason.kind) {
    case 'mlx-down':
      return intl.formatMessage(i18n.mlxDown, { base: reason.base });
    case 'mlx-wrong-model':
      return intl.formatMessage(i18n.mlxWrongModel, {
        served: reason.served,
        wanted: reason.wanted,
      });
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

  const [lookup, setLookup] = useState<Lookup>({ state: 'loading' });
  const [mountErrors, setMountErrors] = useState<Record<string, string>>({});
  /** The node whose mount was requested, held until a status poll that landed AFTER the request
   *  answers — the call returns before the engine flips to mounting, and the stale "stopped" from
   *  the previous poll must not bring the Mount button back in between. */
  const [requesting, setRequesting] = useState<{ nodeId: string; seen: unknown } | null>(null);
  const { status } = useMlxEngineStatusPoll(armed, 2000);
  const statusRef = useRef(status);
  statusRef.current = status;

  useEffect(() => {
    if (requesting && requesting.seen !== undefined && status !== requesting.seen) {
      setRequesting(null);
    }
  }, [status, requesting]);

  useEffect(() => {
    if (!armed) return undefined;
    let alive = true;
    void (async () => {
      try {
        const [raw, settings] = await Promise.all([
          read('swarm', false, { throwOnError: true }) as Promise<SwarmConfig | null>,
          mlxEngineSettingsRead(),
        ]);
        const devices = Array.isArray(raw?.devices) ? raw.devices : [];
        if (alive) setLookup({ state: 'ready', devices, settings });
      } catch (e) {
        if (alive) setLookup({ state: 'failed', error: errorMessage(e, String(e)) });
      }
    })();
    return () => {
      alive = false;
    };
  }, [armed, read]);

  const onMount = useCallback(async (nodeId: string, modelId: string) => {
    setRequesting({ nodeId, seen: undefined });
    setMountErrors((prev) => {
      const { [nodeId]: _dropped, ...rest } = prev;
      return rest;
    });
    try {
      await mlxEngineMount(modelId);
      setRequesting({ nodeId, seen: statusRef.current });
    } catch (e) {
      setMountErrors((prev) => ({ ...prev, [nodeId]: errorMessage(e, String(e)) }));
      setRequesting(null);
    }
  }, []);

  const targetOf = (nodeId: string): MountTarget | null =>
    lookup.state === 'ready' ? resolveMountTarget(nodeId, lookup.devices, lookup.settings) : null;

  const liveFactOf = (row: NoNodeRow): EngineFact | null => {
    if (!armed || row.reason.kind !== 'mlx-down' || row.nodeId == null) return null;
    const target = targetOf(row.nodeId);
    return engineFact(status, target?.kind === 'ok' ? target.servedId : null);
  };
  const mlxFacts = rows.map(liveFactOf).filter((f): f is EngineFact => f != null);
  const anyMounting = requesting != null || mlxFacts.includes('mounting');
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
    if (fact === 'mounting' || requesting?.nodeId === nodeId) {
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
          onClick={() => onMount(nodeId, target.modelId)}
        >
          {intl.formatMessage(i18n.mount, { model: target.modelId })}
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

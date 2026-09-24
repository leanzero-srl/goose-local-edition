import { useCallback, useEffect, useState, useSyncExternalStore, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { Loader2, Network, ServerOff, Settings2 } from 'lucide-react';
import { acpReadConfig } from '../../acp/config';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { foreignOwner } from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  remoteRouteUp,
  subscribeMlxRemoteSingleStatus,
  type MlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { ownsTheMac } from '../leanzero-swarm/mlxDistributed';
import { formatMlxMode } from '../leanzero-swarm/mlxModeLabel';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import { MLX_PROVIDER_ID } from '../settings/models/leanzeroSelectorPolicy';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import { defineMessages, useIntl } from '../../i18n';
import { Button, RADIUS, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import {
  distributedFact,
  distributedProblem,
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
  type MountLookup,
  type MountTarget,
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
  mlxEngine: { id: 'composerReadiness.mlxEngine', defaultMessage: 'LeanZero MLX' },
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
  remote: {
    id: 'composerReadiness.remote',
    defaultMessage: 'Serving from {peer} · {state}',
  },
  mount: { id: 'composerReadiness.mount', defaultMessage: 'Mount {model}' },
  mounting: { id: 'composerReadiness.mounting', defaultMessage: 'Mounting {model}' },
  openProviders: { id: 'composerReadiness.openProviders', defaultMessage: 'Open Providers' },
});

/**
 * Can the ACTIVE provider answer a message right now? Only what the renderer can actually know:
 *
 *  - `swarm`: the router (crates/goose/src/providers/swarm_router.rs) routes to ENABLED devices only
 *    (`enabled` absent = false). Zero enabled devices can never serve. When every enabled device is a
 *    LOCAL `mlx-sidecar` node, the engine status this app supervises is the whole truth: one of them
 *    served → ready, none → not ready. Any other device (LM Studio, cloud, a remote MLX host) is a
 *    node this surface cannot probe, so the answer is `unknown` — never a fake green, never a fake red.
 *  - the LeanZero MLX provider (`omlx`): the engine itself.
 *  - everything else: `unknown`.
 *
 * A status poll that has not answered, or failed, is `unknown`; so is a stray listener on the
 * engine's port (something serves there that this app's manager does not know about).
 *
 * While the DISTRIBUTED engine owns this Mac it is the local MLX node (the router probes it, a
 * single mount is refused): serving the node's id is `ready`, anything else is `distributed` —
 * its state and mode, never a Mount offer.
 *
 * While chat is routed to a LeanZero Link peer's engine (REMOTE SINGLE) that engine is the MLX node
 * (the router adds it and sets this Mac's sidecar aside; `omlx` follows the relay): `remote` — where
 * chat goes and how that engine is doing, shown even when it serves, so it is never a surprise.
 */
export type ComposerReadiness =
  | { kind: 'unknown' }
  | { kind: 'ready' }
  | { kind: 'no-nodes' }
  | { kind: 'unmounted'; nodes: string[]; target: MountTarget; fact: EngineFact }
  | { kind: 'distributed'; nodes: string[]; status: MlxDistributedStatus; wanted: string | null }
  | { kind: 'remote'; status: MlxRemoteSingleStatus };

const UNKNOWN: ComposerReadiness = { kind: 'unknown' };

function statusIsKnowable(status: MlxEngineStatus | null): status is MlxEngineStatus {
  return status != null && status.strayListenerPort == null;
}

export function swarmReadiness(
  lookup: MountLookup,
  status: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  remote: MlxRemoteSingleStatus | null = null
): ComposerReadiness {
  if (remote && remoteRouteUp(remote)) return { kind: 'remote', status: remote };
  if (lookup.state !== 'ready') return UNKNOWN;
  const enabled = lookup.devices.filter((d) => d.enabled === true);
  if (enabled.length === 0) return { kind: 'no-nodes' };
  const localMlx = (d: SwarmDeviceRow) =>
    d.engine === 'mlx-sidecar' &&
    d.host == null &&
    (d.provider == null || d.provider.toLowerCase() === 'lmstudio');
  if (!enabled.every(localMlx)) return UNKNOWN;
  if (distributed && (ownsTheMac(distributed) || foreignOwner(distributed))) {
    if (enabled.some((d) => distributedFact(distributed, d.model_id) === 'up')) {
      return { kind: 'ready' };
    }
    return {
      kind: 'distributed',
      nodes: enabled.map((d) => d.id),
      status: distributed,
      wanted: enabled[0].model_id,
    };
  }
  if (!statusIsKnowable(status)) return UNKNOWN;
  const targets = enabled.map((d) => resolveMountTarget(d.id, lookup.devices, lookup.settings));
  const facts = enabled.map((d) => engineFact(status, d.model_id));
  if (facts.includes('up')) return { kind: 'ready' };
  const target = targets.find((t) => t.kind === 'ok') ?? targets[0];
  const fact = facts.includes('mounting')
    ? 'mounting'
    : facts.includes('failed')
      ? 'failed'
      : 'down';
  return { kind: 'unmounted', nodes: enabled.map((d) => d.id), target, fact };
}

export function mlxProviderReadiness(
  settings: MlxEngineSettings | null,
  status: MlxEngineStatus | null,
  distributed: MlxDistributedStatus | null,
  engineLabel: string,
  remote: MlxRemoteSingleStatus | null = null
): ComposerReadiness {
  if (remote && remoteRouteUp(remote)) return { kind: 'remote', status: remote };
  if (distributed && (ownsTheMac(distributed) || foreignOwner(distributed))) {
    // The omlx provider follows the distributed engine's port while it owns the Mac — this
    // window's run or another's (mlx_engine.rs align_omlx_host_env) — and asks for whatever id
    // that engine lists.
    return distributedServes(distributed)
      ? { kind: 'ready' }
      : { kind: 'distributed', nodes: [engineLabel], status: distributed, wanted: null };
  }
  if (!settings || !statusIsKnowable(status)) return UNKNOWN;
  if (status.state === 'running') return { kind: 'ready' };
  const target: MountTarget = settings.modelId
    ? {
        kind: 'ok',
        modelId: settings.modelId,
        servedId: settings.servedModelName || settings.modelId,
      }
    : { kind: 'none' };
  return {
    kind: 'unmounted',
    nodes: [engineLabel],
    target,
    fact: engineFact(status, target.kind === 'ok' ? target.servedId : null),
  };
}

const readSwarm = () => acpReadConfig('swarm', false) as Promise<SwarmConfig | null>;

/**
 * The composer's readiness strip: a solid warning ABOVE the input when the active provider provably
 * cannot answer, with the same actions as the transcript's no-node notice (Mount, Open Providers).
 * It never blocks typing, and it renders nothing when readiness is unknowable.
 */
export function ComposerReadinessStrip({ provider }: { provider: string | null | undefined }) {
  const intl = useIntl();
  const isSwarm = provider === 'swarm';
  const isMlx = provider === MLX_PROVIDER_ID;
  const armed = isSwarm || isMlx;

  // Re-read the pool when the window regains focus: devices are edited in Providers.
  const [focusEpoch, setFocusEpoch] = useState(0);
  useEffect(() => {
    if (!armed) return undefined;
    const onFocus = () => setFocusEpoch((n) => n + 1);
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [armed]);

  const lookup = useMountLookup(armed, readSwarm, `${provider}:${focusEpoch}`);
  const pollsEngine =
    lookup.state === 'ready' &&
    (isMlx ||
      (isSwarm &&
        lookup.devices.some((d) => d.enabled === true && d.engine === 'mlx-sidecar' && !d.host)));
  const { status } = useMlxEngineStatusPoll(pollsEngine, 3000);
  const distributed = useLatestMlxDistributedStatus();
  const remote = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  const { requestingNodeId, mountErrors, mount } = useMlxMount(status);

  const readiness: ComposerReadiness = isSwarm
    ? swarmReadiness(lookup, status, distributed, remote)
    : isMlx
      ? mlxProviderReadiness(
          lookup.state === 'ready' ? lookup.settings : null,
          status,
          distributed,
          intl.formatMessage(i18n.mlxEngine),
          remote
        )
      : UNKNOWN;

  if (readiness.kind === 'unknown' || readiness.kind === 'ready') return null;
  return (
    <ReadinessStripBody
      readiness={readiness}
      status={status}
      requesting={requestingNodeId != null}
      mountError={mountErrors[STRIP_MOUNT_KEY] ?? null}
      onMount={(modelId) => void mount(STRIP_MOUNT_KEY, modelId)}
    />
  );
}

/** The strip mounts ONE engine whatever the node count, so its mount state has one key. */
const STRIP_MOUNT_KEY = 'composer';

function ReadinessStripBody({
  readiness,
  status,
  requesting,
  mountError,
  onMount,
}: {
  readiness: Exclude<ComposerReadiness, { kind: 'unknown' } | { kind: 'ready' }>;
  status: MlxEngineStatus | null;
  requesting: boolean;
  mountError: string | null;
  onMount: (modelId: string) => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const openProviders = useCallback(() => navigate('/leanzero-swarm'), [navigate]);

  const headline =
    readiness.kind === 'no-nodes'
      ? intl.formatMessage(i18n.noNodes)
      : readiness.kind === 'remote'
        ? intl.formatMessage(i18n.remote, {
            peer: readiness.status.peerHostname ?? readiness.status.peer ?? '',
            state: readiness.status.state,
          })
        : readiness.kind === 'distributed'
          ? intl.formatMessage(i18n.distributed, {
              mode: formatMlxMode(intl, distributedSummary(readiness.status), null),
              state: distributedStateLabel(intl, readiness.status),
              nodes: readiness.nodes.join(', '),
            })
          : intl.formatMessage(i18n.unmounted, { nodes: readiness.nodes.join(', ') });

  let detail: string | null = null;
  let action: ReactNode = null;
  const remoteServing = readiness.kind === 'remote' && readiness.status.state === 'ready';
  if (readiness.kind === 'remote') {
    detail = readiness.status.lastError ?? null;
    if (readiness.status.state === 'mounting') {
      action = (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-remote-mounting"
          className="size-4 animate-spin text-white"
        />
      );
    }
  }
  if (readiness.kind === 'distributed') {
    const { status: dist, wanted } = readiness;
    const served = distributedServedId(dist);
    if (distributedServes(dist) && wanted != null && served != null && served !== wanted) {
      detail = intl.formatMessage(i18n.distributedMismatch, { served, wanted });
    } else {
      detail = distributedProblem(dist);
    }
    if (dist.state === 'preflight' || dist.state === 'starting') {
      action = (
        <Loader2
          aria-hidden
          data-testid="composer-readiness-distributed-starting"
          className="size-4 animate-spin text-white"
        />
      );
    }
  }
  if (readiness.kind === 'unmounted') {
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
        remoteServing ? TONE_FILL.ok : TONE_FILL.warn
      )}
    >
      {readiness.kind === 'remote' ? (
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
        <Button
          size="sm"
          variant="secondary"
          icon={<Settings2 />}
          data-testid="composer-readiness-open-providers"
          onClick={openProviders}
        >
          {intl.formatMessage(i18n.openProviders)}
        </Button>
      </div>
    </div>
  );
}

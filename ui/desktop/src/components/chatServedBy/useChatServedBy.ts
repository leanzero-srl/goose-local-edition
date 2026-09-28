import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import type { NodeServedTurnDto } from '@aaif/goose-sdk';
import { acpReadConfig } from '../../acp/config';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import {
  latestMlxRemoteSingleReadError,
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import {
  MLX_ENGINE_SNAPSHOT_CHANNEL,
  isMlxEngineSnapshot,
  type MlxEngineSnapshot,
} from '../../utils/mlxEngineMonitor';
import { defineMessages, useIntl } from '../../i18n';
import { MLX_STATUS_POLL_MS } from '../leanzero-swarm/mlxLiveStats';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import { MLX_PROVIDER_ID } from '../settings/models/leanzeroSelectorPolicy';
import type { SwarmConfig } from '../settings/swarm/golden';
import {
  distributedServedId,
  singleServedId,
  useLatestMlxDistributedStatus,
  useMountLookup,
} from '../noNodeNotice/mlxMount';
import { useGlanceNodes } from '../engineGlance/glanceStore';
import { nodesServedLast } from '../../acp/nodes';
import { deriveChatServedBy, type ChatNodesFacts, type ChatServedBy } from './chatServedBy';

const i18n = defineMessages({
  thisMac: { id: 'chatServedBy.thisMac', defaultMessage: 'This Mac' },
  mlxEngine: { id: 'composerReadiness.mlxEngine', defaultMessage: 'LeanZero MLX' },
});

const readSwarm = () => acpReadConfig('swarm', false) as Promise<SwarmConfig | null>;

/**
 * main's latest read of the engine that serves chat (utils/mlxEngineMonitor.ts): what it is doing
 * and who it serves. main reads it the whole time it answers; this only asks main, every poll,
 * while `enabled`. null = no bridge, or the read failed — never an assumed idle engine.
 */
function useMainEngineSnapshot(enabled: boolean): MlxEngineSnapshot | null {
  const [snapshot, setSnapshot] = useState<MlxEngineSnapshot | null>(null);
  useEffect(() => {
    if (!enabled) {
      setSnapshot(null);
      return undefined;
    }
    const bridge = (
      window as unknown as { electron?: { mlxEngineActivity?: () => Promise<MlxEngineSnapshot> } }
    ).electron?.mlxEngineActivity;
    if (!bridge) return undefined;
    let alive = true;
    const read = async () => {
      try {
        const next = await bridge();
        if (alive) setSnapshot(next);
      } catch {
        if (alive) setSnapshot(null);
      }
    };
    void read();
    const timer = setInterval(() => void read(), MLX_STATUS_POLL_MS);
    // main pushes each read as it lands (Q-59); the poll stays for a main without the push.
    const electron = (
      window as unknown as {
        electron?: {
          on?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
          off?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
        };
      }
    ).electron;
    const onPush = (_event: unknown, ...args: unknown[]) => {
      if (alive && isMlxEngineSnapshot(args[0])) setSnapshot(args[0]);
    };
    electron?.on?.(MLX_ENGINE_SNAPSHOT_CHANNEL, onPush);
    return () => {
      alive = false;
      clearInterval(timer);
      electron?.off?.(MLX_ENGINE_SNAPSHOT_CHANNEL, onPush);
    };
  }, [enabled]);
  return snapshot;
}

/**
 * The router's record of this `node:`/`strategy:` chat's last turn (design §8.5's
 * `nodes/servedLast`), read when the chat opens and each time a turn ends — an event, never a poll.
 * record null = read, no turn served yet; undefined = not read, or the read failed (no node is then
 * named).
 */
function useServedLast(
  sessionId: string | null,
  model: string | null,
  turnInFlight: boolean
): NodeServedTurnDto | null | undefined {
  const routed = model != null && (model.startsWith('node:') || model.startsWith('strategy:'));
  const [read, setRead] = useState<{
    sessionId: string;
    record: NodeServedTurnDto | null;
  } | null>(null);
  useEffect(() => {
    if (!routed || !sessionId || turnInFlight) return undefined;
    let alive = true;
    nodesServedLast(sessionId)
      .then((r) => {
        if (alive) setRead({ sessionId, record: r.record ?? null });
      })
      .catch(() => {
        if (alive) setRead(null);
      });
    return () => {
      alive = false;
    };
  }, [routed, sessionId, turnInFlight]);
  return read && read.sessionId === sessionId ? read.record : undefined;
}

export interface ChatServing {
  served: ChatServedBy;
  /** This Mac's single engine as the composer polled it — the readiness bar's Mount follows it. */
  single: MlxEngineStatus | null;
  /** The provider rides an MLX engine this renderer reads (`swarm`, `omlx`). */
  armed: boolean;
  /** This chat has a turn in flight (what the bar's words about "this answer" hang on). */
  turnInFlight: boolean;
}

/**
 * The reads `deriveChatServedBy` needs, gathered ONCE per composer (ChatInput) and handed to every
 * chat surface as props — the chip, the readiness bar and the context counter never read on their
 * own. Nothing is read for a provider whose engine this renderer cannot see (a cloud provider).
 */
export function useChatServedBy(
  provider: string | null | undefined,
  sessionId: string | null,
  turnInFlight: boolean,
  model: string | null = null
): ChatServing {
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

  // ...and when what this Mac serves changes: a Run writes the serving intent the pool's chat rule
  // follows (`chatNodeOf`), so the lookup that carries it is read again.
  const [servedKey, setServedKey] = useState('|');
  const lookup = useMountLookup(armed, readSwarm, `${provider}:${focusEpoch}:${servedKey}`);
  const pollsEngine =
    lookup.state === 'ready' &&
    (isMlx ||
      (isSwarm &&
        lookup.devices.some((d) => d.enabled === true && d.engine === 'mlx-sidecar' && !d.host)));
  const { status } = useMlxEngineStatusPoll(pollsEngine, MLX_STATUS_POLL_MS);
  const distributed = useLatestMlxDistributedStatus();
  const remote = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  const remoteReadError = useSyncExternalStore(
    subscribeMlxRemoteSingleStatus,
    latestMlxRemoteSingleReadError
  );
  const main = useMainEngineSnapshot(armed);
  // goosed's nodes read — the loader's marks and the node names (Q-254, Q-255): the glance store's
  // one read per window, re-read on each engine change and while the loader is at work.
  const glanceNodes = useGlanceNodes();
  const servedRecord = useServedLast(sessionId, model, turnInFlight);
  const nodes: ChatNodesFacts | null = useMemo(
    () =>
      glanceNodes.kind === 'read'
        ? {
            read: glanceNodes.read,
            residency: glanceNodes.residency,
            servedNode: servedRecord === undefined ? undefined : (servedRecord?.node ?? null),
            servedRecord,
          }
        : null,
    [glanceNodes, servedRecord]
  );
  const nowServed = `${singleServedId(status) ?? ''}|${distributed ? (distributedServedId(distributed) ?? '') : ''}`;
  useEffect(() => setServedKey(nowServed), [nowServed]);

  // The Mac answers main again while the route's last word is still "reconnecting" (or its last
  // read failed): read the route now, so the bar clears with main's read instead of the next poll.
  const mainBack = main?.engine === 'remote' && main.mode === 'running';
  const routeStale = remote?.state === 'reconnecting' || remoteReadError != null;
  useEffect(() => {
    if (mainBack && routeStale) mlxRemoteSingleStatus().catch(() => undefined);
  }, [mainBack, routeStale]);

  const thisMac = intl.formatMessage(i18n.thisMac);
  const engineLabel = intl.formatMessage(i18n.mlxEngine);
  const served = useMemo(
    () =>
      deriveChatServedBy({
        provider,
        lookup,
        single: status,
        distributed,
        remote,
        remoteReadError,
        main,
        sessionId,
        turnInFlight,
        thisMac,
        engineLabel,
        model,
        nodes,
      }),
    [
      provider,
      lookup,
      status,
      distributed,
      remote,
      remoteReadError,
      main,
      sessionId,
      turnInFlight,
      thisMac,
      engineLabel,
      model,
      nodes,
    ]
  );
  return { served, single: status, armed, turnInFlight };
}

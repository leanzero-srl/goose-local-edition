import { useEffect, useState } from 'react';
import { useConfig } from '../ConfigContext';
import { DEFAULTS, type SwarmConfig, type SwarmDeviceRow } from '../settings/swarm/golden';
import { chipFor, LOCAL_CHIP, MLX_CHIP } from '../leanzero-swarm/cloud';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import { defineMessages, useIntl } from '../../i18n';
import {
  Chip,
  NODE_INDEXES,
  Panel,
  ROW,
  StatusDot,
  TYPE,
  WEIGHT,
  cx,
  type NodeIndex,
  type Tone,
} from '../lz';

const i18nMsg = defineMessages({
  title: { id: 'nodesStrip.title', defaultMessage: 'Nodes' },
  serving: { id: 'nodesStrip.serving', defaultMessage: 'serving' },
  idle: { id: 'nodesStrip.idle', defaultMessage: 'idle' },
  mounting: { id: 'nodesStrip.mounting', defaultMessage: 'mounting' },
  failed: { id: 'nodesStrip.failed', defaultMessage: 'failed' },
});

/** Occupancy is STATE, so it renders in the status triad — never a node hue, never a hand-written hex. */
const OCCUPANCY_TONE = {
  serving: 'ok',
  idle: 'stopped',
  mounting: 'warn',
  failed: 'err',
} as const satisfies Record<string, Tone>;

type Occupancy = keyof typeof OCCUPANCY_TONE;

/**
 * What serves a configured node. The engine/provider is metadata (a quiet chip); the LeanZero MLX
 * engine keeps its violet through the Studio's secondary tone, the one secondary emphasis here.
 */
function engineChipOf(d: SwarmDeviceRow): { label: string; tone?: Tone } {
  const cloud = chipFor(d.provider);
  if (cloud) return { label: cloud.seg };
  if (d.engine === 'mlx-sidecar') return { label: MLX_CHIP.seg, tone: 'secondary' };
  return { label: LOCAL_CHIP.seg };
}

/** Node identity follows the configured order — the same ramp the formation ribbon walks. */
const nodeHue = (i: number): NodeIndex => NODE_INDEXES[i % NODE_INDEXES.length];

/**
 * Pass E (owner): a compact "Nodes" strip for a session's BLANK state — YOUR configured swarm
 * devices and whether they are occupied, before any run exists. Rows come from the same swarm
 * config the LeanZero Swarm nodes tab reads (`read('swarm')` → devices) — NEVER from LM Studio
 * discovery (those rows are legacy and live behind the showLmStudioFleet setting elsewhere).
 *
 * Occupancy is shown only where a LIVE signal exists: the local machine's mlx-sidecar node reads
 * the MLX engine status (running+model = serving, mounting = amber, stopped = idle, failed = red).
 * Cloud rows and remote/LM Studio rows get their provider chip and NO invented state — a missing
 * signal renders nothing rather than a guess.
 */
export default function NodesStrip({ className = '' }: { className?: string }) {
  const intl = useIntl();
  const { read } = useConfig();
  const [devices, setDevices] = useState<SwarmDeviceRow[] | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const raw = (await read('swarm', false)) as SwarmConfig | null;
        const cfg: SwarmConfig = { ...DEFAULTS, ...(raw ?? {}) };
        if (alive) setDevices(Array.isArray(cfg.devices) ? cfg.devices : []);
      } catch {
        if (alive) setDevices([]);
      }
    })();
    return () => {
      alive = false;
    };
  }, [read]);

  // Poll the MLX engine only when a LOCAL mlx-sidecar node is actually configured.
  const hasLocalMlx = (devices ?? []).some((d) => d.engine === 'mlx-sidecar' && d.host == null);
  const { status: mlxStatus } = useMlxEngineStatusPoll(hasLocalMlx);

  if (!devices || devices.length === 0) return null;

  const occupancyFor = (d: SwarmDeviceRow): { kind: Occupancy; detail?: string } | null => {
    if (d.engine !== 'mlx-sidecar' || d.host != null) return null;
    if (!mlxStatus) return null; // no live signal — show nothing, never a guess
    switch (mlxStatus.state) {
      case 'running': {
        const model = mlxStatus.servedModelId ?? mlxStatus.modelId;
        return model ? { kind: 'serving', detail: model } : { kind: 'idle' };
      }
      case 'mounting':
        return { kind: 'mounting' };
      case 'failed':
        return { kind: 'failed' };
      case 'stopped':
        return { kind: 'idle' };
      default:
        return null;
    }
  };

  const occupancyLabel: Record<Occupancy, string> = {
    serving: intl.formatMessage(i18nMsg.serving),
    idle: intl.formatMessage(i18nMsg.idle),
    mounting: intl.formatMessage(i18nMsg.mounting),
    failed: intl.formatMessage(i18nMsg.failed),
  };

  return (
    <div data-testid="nodes-strip" className={className}>
      <Panel title={intl.formatMessage(i18nMsg.title)} count={devices.length} padded={false}>
        <div className="flex flex-col divide-y divide-lz-border">
          {devices.map((d, i) => {
            const engine = engineChipOf(d);
            const occ = occupancyFor(d);
            return (
              <div
                key={d.id}
                data-testid={`nodes-strip-row-${d.id}`}
                className={cx('flex items-center gap-2.5 px-4', ROW.dense)}
              >
                <StatusDot node={nodeHue(i)} live={occ?.kind === 'serving'} label={d.id} />
                <span className={cx('min-w-0 truncate', TYPE.body, WEIGHT.medium)}>{d.id}</span>
                <Chip tone={engine.tone} title={engine.label}>
                  {engine.label}
                </Chip>
                <span className={cx('min-w-0 flex-1 truncate', TYPE.meta)} title={d.model_id}>
                  {d.model_id}
                </span>
                {occ && (
                  <span
                    data-testid={`nodes-strip-occupancy-${d.id}`}
                    className="flex min-w-0 shrink-0 items-center gap-1.5"
                    title={occ.detail}
                  >
                    <Chip tone={OCCUPANCY_TONE[occ.kind]}>{occupancyLabel[occ.kind]}</Chip>
                    {occ.detail && (
                      <span className="max-w-[16rem] truncate font-mono text-lz-mono text-lz-ink-3">
                        {occ.detail}
                      </span>
                    )}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      </Panel>
    </div>
  );
}

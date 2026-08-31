import { useEffect, useState } from 'react';
import { useConfig } from '../ConfigContext';
import { DEFAULTS, type SwarmConfig, type SwarmDeviceRow } from '../settings/swarm/golden';
import { chipFor, LOCAL_CHIP, MLX_CHIP } from '../leanzero-swarm/cloud';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import { defineMessages, useIntl } from '../../i18n';

const i18nMsg = defineMessages({
  title: { id: 'nodesStrip.title', defaultMessage: 'Nodes' },
  serving: { id: 'nodesStrip.serving', defaultMessage: 'serving' },
  idle: { id: 'nodesStrip.idle', defaultMessage: 'idle' },
  mounting: { id: 'nodesStrip.mounting', defaultMessage: 'mounting' },
  failed: { id: 'nodesStrip.failed', defaultMessage: 'failed' },
});

// Benchmark/SwarmRunPanel status palette — solid saturated hues, never tints.
const OCCUPANCY_COLORS = {
  serving: '#2ecc71',
  idle: '#64748b',
  mounting: '#f5a623',
  failed: '#e5484d',
} as const;

type Occupancy = keyof typeof OCCUPANCY_COLORS;

/** What serves a configured node, as a solid chip — same rule as the LeanZero Swarm nodes tab. */
function providerChipOf(d: SwarmDeviceRow): { seg: string; chip: string } {
  const cloud = chipFor(d.provider);
  if (cloud) return { seg: cloud.seg, chip: cloud.chip };
  if (d.engine === 'mlx-sidecar') return MLX_CHIP;
  return LOCAL_CHIP;
}

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
    <div
      data-testid="nodes-strip"
      className={`overflow-hidden rounded border border-border-primary ${className}`}
    >
      <div className="flex items-center gap-2 border-b border-border-primary bg-background-secondary px-3 py-1.5">
        <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
          {intl.formatMessage(i18nMsg.title)}
        </span>
        <span className="text-xs text-text-secondary">{devices.length}</span>
      </div>
      <div className="flex flex-col gap-1.5 px-3 py-2">
        {devices.map((d) => {
          const chip = providerChipOf(d);
          const occ = occupancyFor(d);
          return (
            <div
              key={d.id}
              data-testid={`nodes-strip-row-${d.id}`}
              className="flex items-center justify-between gap-3 rounded border border-border-primary px-2.5 py-1.5"
            >
              <span className="min-w-0 flex items-center gap-2">
                <span
                  className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold text-background-primary"
                  style={{ backgroundColor: chip.chip }}
                >
                  {chip.seg.toUpperCase()}
                </span>
                <span className="truncate font-mono text-sm text-text-primary" title={d.model_id}>
                  {d.id}
                </span>
              </span>
              {occ && (
                <span
                  data-testid={`nodes-strip-occupancy-${d.id}`}
                  className="flex shrink-0 items-center gap-1.5 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-white"
                  style={{ backgroundColor: OCCUPANCY_COLORS[occ.kind] }}
                  title={occ.detail}
                >
                  {occupancyLabel[occ.kind]}
                  {occ.detail && (
                    <span className="max-w-[16rem] truncate normal-case font-mono font-normal">
                      {occ.detail}
                    </span>
                  )}
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

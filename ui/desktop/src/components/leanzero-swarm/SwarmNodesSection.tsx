import { useCallback, useEffect, useState } from 'react';
import { ArrowLeftRight, Plus, Server, Trash2 } from 'lucide-react';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { useConfig } from '../ConfigContext';
import { useFleet, deviceFromModelId } from '../swarm/useFleet';
import { useLmStudioFleetVisible } from '../../hooks/useLmStudioFleetVisible';
import {
  type SwarmConfig,
  type SwarmDeviceRow,
  type NodeRow,
  nodeRows,
  DEFAULTS,
} from '../settings/swarm/golden';
import { chipFor, cloudCliErr, LOCAL_CHIP, MLX_CHIP } from './cloud';
import AddNodeDialog, { type ReassignTarget } from './AddNodeDialog';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  Panel,
  StatusDot,
  SURFACE,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type NodeIndex,
} from '../lz';
import { ToneBanner, WeightStepper, nodeHue } from './studio';
import { defineMessages, useIntl } from '../../i18n';

const i18nMsg = defineMessages({
  nodesTitle: { id: 'swarmSettings.nodesTitle', defaultMessage: 'Nodes' },
  nodesDesc: {
    id: 'swarmSettings.nodesDesc',
    defaultMessage:
      'Share: relative share of work across nodes — higher gets more tasks.',
  },
  shareLabel: { id: 'swarmSettings.shareLabel', defaultMessage: 'Share' },
  addNode: { id: 'swarmSettings.addNode', defaultMessage: 'Add node' },
  autoChip: { id: 'swarmSettings.autoChip', defaultMessage: 'auto' },
  autoChipTitle: {
    id: 'swarmSettings.autoChipTitle',
    defaultMessage: 'Discovered live from LM Studio — joins the pool automatically',
  },
  awaitingChip: { id: 'swarmSettings.awaitingChip', defaultMessage: 'awaiting fleet routing' },
  awaitingChipTitle: {
    id: 'swarmSettings.awaitingChipTitle',
    defaultMessage:
      'A remote machine’s MLX node — saved in the pool, served once per-node engine routing ships',
  },
  reassignAria: { id: 'swarmSettings.reassignAria', defaultMessage: 'Reassign provider' },
  removeAria: { id: 'swarmSettings.removeAria', defaultMessage: 'Remove node' },
  removeTitle: { id: 'swarmSettings.removeTitle', defaultMessage: 'Remove node' },
  removeMessage: {
    id: 'swarmSettings.removeMessage',
    defaultMessage:
      '{id} leaves the flock pool. No model files are touched; you can add it again any time.',
  },
  removeConfirm: { id: 'swarmSettings.removeConfirm', defaultMessage: 'Remove' },
  reassignTitle: { id: 'swarmSettings.reassignTitle', defaultMessage: 'Reassign provider' },
  reassignMessage: {
    id: 'swarmSettings.reassignMessage',
    defaultMessage:
      'Changing what serves {id} works by REMOVING the node and RE-ADDING it under the provider you pick next. Nothing changes until the new add commits.',
  },
  reassignConfirm: { id: 'swarmSettings.reassignConfirm', defaultMessage: 'Pick new provider' },
});

/** What serves a node, as its label: cloud provider > LeanZero MLX engine > LM Studio. */
function providerLabelOf(row: NodeRow): string {
  const cloud = chipFor(row.provider);
  if (cloud) return cloud.seg;
  if (row.engine === 'mlx-sidecar') return MLX_CHIP.seg;
  return LOCAL_CHIP.seg;
}

/** A node row with its identity hue by list position — the hue is identity only, never state. */
interface NodeView {
  row: NodeRow;
  hue: NodeIndex;
}

/**
 * The Swarm Settings tab — NODES ONLY, per the owner's simplification: "I just want Add node, for
 * each node choose provider, and then for all nodes choose weights. That is it." Per row: label,
 * provider (click = the remove+re-add reassign flow), model id, ONE stepper — the ROUTING SHARE
 * (SwarmDevice.speed_weight), how much of the work a node gets — remove. Concurrency
 * (SwarmDevice.weight) is left at its default and is NOT edited here.
 *
 * SHARE PERSISTENCE mirrors the engine's read (`d.speed_weight.unwrap_or_else(|| speed_weight_for(&d.id))`
 * in swarm.rs — the node's own field wins, the `speed_weights` substring map is the fallback):
 *  - local/discovered rows own a device row, so the share writes to that row's `speed_weight`;
 *  - cloud rows are CLI-owned and the CLI `add` has no share flag, so their share writes to the
 *    top-level `speed_weights` map keyed by the device id — which never touches the CLI-owned device
 *    LIST, keeping the invariant intact and the engine's fallback coherent.
 * An untouched node persists nothing: it shows the default share (1) until the user changes it.
 */
export default function SwarmNodesSection({
  onOpenCloudProviders,
}: {
  /** Deep-link target for the add-dialog's "no key" state — the parent view opens its Cloud
   *  Providers tab. */
  onOpenCloudProviders?: () => void;
} = {}) {
  const intl = useIntl();
  const { read, upsert } = useConfig();
  // LEGACY surface (pass E follow-up): LM Studio-DISCOVERED rows join this list only when the
  // 'showLmStudioFleet' setting is on (default off) — same switch as every other LM Studio surface.
  // Configured rows are config truth and always render.
  const lmStudioVisible = useLmStudioFleetVisible();
  const [cfg, setCfg] = useState<SwarmConfig>(DEFAULTS);
  // Discovery probes the configured `swarm.endpoint` — the host the engine builds against.
  const fleet = useFleet(5000, cfg.endpoint, lmStudioVisible);

  const reloadSwarm = useCallback(async () => {
    try {
      const raw = (await read('swarm', false)) as SwarmConfig | null;
      setCfg({ ...DEFAULTS, ...(raw ?? {}) });
    } catch {
      // keep the current state; the next mount re-reads
    }
  }, [read]);

  useEffect(() => {
    void reloadSwarm();
  }, [reloadSwarm]);

  const [addOpen, setAddOpen] = useState(false);
  const [reassignTarget, setReassignTarget] = useState<ReassignTarget | null>(null);
  const [pendingRemove, setPendingRemove] = useState<NodeRow | null>(null);
  const [pendingReassign, setPendingReassign] = useState<NodeRow | null>(null);
  const [removing, setRemoving] = useState(false);
  const [nodeError, setNodeError] = useState<string | null>(null);

  /** Fresh-read the whole swarm config and mutate it — never from possibly-stale panel state. */
  const mutateConfigFresh = useCallback(
    async (mutate: (base: SwarmConfig) => SwarmConfig) => {
      const raw = (await read('swarm', false)) as SwarmConfig | null;
      const base: SwarmConfig = { ...DEFAULTS, ...(raw ?? {}) };
      const next = mutate(base);
      await upsert('swarm', next, false);
      setCfg(next);
    },
    [read, upsert]
  );

  /** Fresh-read the config and mutate its device list. */
  const mutateDevicesFresh = useCallback(
    (mutate: (devices: SwarmDeviceRow[]) => SwarmDeviceRow[]) =>
      mutateConfigFresh((base) => ({
        ...base,
        devices: mutate(Array.isArray(base.devices) ? [...base.devices] : []),
      })),
    [mutateConfigFresh]
  );

  /** Remove one node by its identity — cloud rows via the CLI, local rows via the config. */
  const removeNodeRow = useCallback(
    async (node: { id: string; modelId: string; provider: string | null }) => {
      if (node.provider) {
        const r = await window.electron.swarmCloud(node.provider, ['rm', node.modelId]);
        if (!r.ok) throw new Error(cloudCliErr(r));
        await reloadSwarm();
      } else {
        await mutateDevicesFresh((devices) => devices.filter((d) => d.id !== node.id));
      }
    },
    [mutateDevicesFresh, reloadSwarm]
  );

  const confirmRemove = useCallback(async () => {
    if (!pendingRemove) return;
    setRemoving(true);
    setNodeError(null);
    try {
      await removeNodeRow(pendingRemove);
    } catch (e) {
      setNodeError(e instanceof Error ? e.message : String(e));
    } finally {
      setRemoving(false);
      setPendingRemove(null);
    }
  }, [pendingRemove, removeNodeRow]);

  /** The MLX add path commits here: reassign removes the OLD row in the same motion. */
  const commitLocalRow = useCallback(
    async (row: SwarmDeviceRow) => {
      const old = reassignTarget;
      if (old && old.provider) {
        const r = await window.electron.swarmCloud(old.provider, ['rm', old.modelId]);
        if (!r.ok) throw new Error(cloudCliErr(r));
      }
      await mutateDevicesFresh((devices) => [
        ...devices.filter((d) => !(old && d.id === old.id)),
        row,
      ]);
      setReassignTarget(null);
    },
    [reassignTarget, mutateDevicesFresh]
  );

  /** A cloud ADD landed through the CLI — finish a reassign by dropping the old row. */
  const onCloudAdded = useCallback(async () => {
    const old = reassignTarget;
    if (!old) return;
    setReassignTarget(null);
    setAddOpen(false);
    try {
      await removeNodeRow(old);
    } catch (e) {
      setNodeError(e instanceof Error ? e.message : String(e));
    }
  }, [reassignTarget, removeNodeRow]);

  /**
   * ONE routing SHARE per node = how much of the work it gets. Persisted where the engine reads it
   * (device field wins over the `speed_weights` map), by node kind:
   *  - configured local rows: write `speed_weight` on the row in place, leaving `weight` (concurrency)
   *    untouched;
   *  - discovered LM Studio rows: materialize a device row so the share has somewhere to live
   *    (id = the machine short name when free, else the model id — the pool CLI's own shape), with
   *    `weight` at its default;
   *  - cloud rows: the CLI owns the device list and its `add` has no share flag, so the share lands in
   *    the top-level `speed_weights` map keyed by the device id (updating an existing matching key in
   *    place, else adding one). No CLI call, no device-row mutation — the invariant holds.
   */
  const setNodeShare = useCallback(
    (row: NodeRow, share: number) => {
      setNodeError(null);
      const fail = (e: unknown) => setNodeError(e instanceof Error ? e.message : String(e));
      if (row.provider) {
        void mutateConfigFresh((base) => {
          const sw = { ...(base.speed_weights ?? {}) };
          // The engine substring-matches these keys (id.contains(key)); a full device id contains
          // itself, so a newly written full-id key always matches its node. Update an existing
          // matching key in place so the map never carries two values for one node.
          const existing =
            row.id in sw ? row.id : Object.keys(sw).find((k) => row.id.includes(k));
          sw[existing ?? row.id] = share;
          return { ...base, speed_weights: sw };
        }).catch(fail);
        return;
      }
      void mutateDevicesFresh((devices) => {
        if (row.configured) {
          return devices.map((d) => (d.id === row.id ? { ...d, speed_weight: share } : d));
        }
        const machine = deviceFromModelId(row.modelId) || row.modelId;
        const id = devices.some((d) => d.id === machine) ? row.modelId : machine;
        return [
          ...devices,
          { id, model_id: row.modelId, weight: 1, speed_weight: share, enabled: true },
        ];
      }).catch(fail);
    },
    [mutateConfigFresh, mutateDevicesFresh]
  );

  const configuredDevices: SwarmDeviceRow[] = Array.isArray(cfg.devices) ? cfg.devices : [];
  // EVERY node the swarm would actually run, in one list. `nodeRows` (golden.ts) owns the union so
  // the test exercises the shipped rule rather than a copy of it.
  const rows = nodeRows(configuredDevices, fleet.models);
  // The node's routing SHARE, exactly as the engine resolves it: the device's own `speed_weight`
  // wins, then the `speed_weights` substring map (id.contains(key)), then 1. Reading the map keeps
  // a legacy map-only config honest — the stepper shows the share the engine actually routes by.
  const speedWeights = cfg.speed_weights ?? {};
  const shareFromMap = (id: string): number | undefined => {
    if (id in speedWeights) return speedWeights[id];
    const key = Object.keys(speedWeights).find((k) => id.includes(k));
    return key ? speedWeights[key] : undefined;
  };
  const shareOf = (row: NodeRow): number => {
    const deviceId = row.configured ? row.id : deviceFromModelId(row.modelId);
    const device = configuredDevices.find((d) => d.id === deviceId);
    return device?.speed_weight ?? shareFromMap(row.id) ?? 1;
  };

  const endpoint = cfg.endpoint ?? DEFAULTS.endpoint ?? '';

  const view: NodeView[] = rows.map((row, i) => ({ row, hue: nodeHue(i) }));

  const columns: DataTableColumn<NodeView>[] = [
    {
      key: 'node',
      header: 'Node',
      cell: ({ row, hue }) => {
        const isCloud = row.provider != null;
        const name = isCloud ? row.modelId : deviceFromModelId(row.modelId) || row.id;
        return (
          <span className="flex items-center gap-2">
            <StatusDot node={hue} label={`node ${name}`} />
            <span className={cx('truncate', WEIGHT.semibold)} title={row.modelId}>
              {name}
            </span>
          </span>
        );
      },
    },
    {
      key: 'provider',
      header: 'Provider',
      cell: ({ row }) => {
        const label = providerLabelOf(row);
        const isRemoteMlx = row.engine === 'mlx-sidecar' && row.host != null;
        return (
          <span className="flex items-center gap-2">
            {row.configured ? (
              <Button
                variant="ghost"
                size="sm"
                icon={<ArrowLeftRight />}
                onClick={() => setPendingReassign(row)}
                title={intl.formatMessage(i18nMsg.reassignAria)}
                aria-label={`${label}: ${row.id}`}
              >
                {label}
              </Button>
            ) : (
              <span className={TYPE.body}>{label}</span>
            )}
            {!row.configured && (
              <Chip title={intl.formatMessage(i18nMsg.autoChipTitle)}>
                {intl.formatMessage(i18nMsg.autoChip)}
              </Chip>
            )}
            {isRemoteMlx && (
              <span data-testid={`awaiting-routing-${row.id}`}>
                <Chip tone="stopped" title={intl.formatMessage(i18nMsg.awaitingChipTitle)}>
                  {intl.formatMessage(i18nMsg.awaitingChip)}
                </Chip>
              </span>
            )}
          </span>
        );
      },
    },
    {
      key: 'model',
      header: 'Model',
      cell: ({ row }) =>
        row.provider != null ? (
          <span className="text-lz-ink-4">—</span>
        ) : (
          <span className="block max-w-[28ch] truncate font-mono text-lz-mono text-lz-ink-3" title={row.modelId}>
            {row.modelId}
          </span>
        ),
    },
    {
      key: 'share',
      header: intl.formatMessage(i18nMsg.shareLabel),
      numeric: true,
      cell: ({ row }) => (
        <WeightStepper value={shareOf(row)} onChange={(v) => setNodeShare(row, v)} label={row.id} />
      ),
    },
  ];

  return (
    <section id="swarm-nodes" className="flex flex-col gap-4 pb-8">
      <Panel
        title={intl.formatMessage(i18nMsg.nodesTitle)}
        count={rows.length}
        padded={false}
        headerRight={
          <Button
            variant="primary"
            size="sm"
            icon={<Plus />}
            data-testid="swarm-add-node"
            onClick={() => {
              setReassignTarget(null);
              setAddOpen(true);
            }}
          >
            {intl.formatMessage(i18nMsg.addNode)}
          </Button>
        }
      >
        {nodeError && (
          <div className="px-4 pt-4">
            <ToneBanner tone="err" label="Nodes" text={nodeError} />
          </div>
        )}

        <DataTable
          aria-label={intl.formatMessage(i18nMsg.nodesTitle)}
          columns={columns}
          rows={view}
          rowKey={(v) => v.row.id}
          rowAction={({ row }) =>
            row.configured ? (
              <Button
                variant="ghost"
                size="sm"
                icon={<Trash2 />}
                onClick={() => {
                  setNodeError(null);
                  setPendingRemove(row);
                }}
                aria-label={`${intl.formatMessage(i18nMsg.removeAria)}: ${row.id}`}
              />
            ) : null
          }
          empty={
            <EmptyState
              icon={<Server />}
              title="No nodes yet"
              body="Add one with “Add node” — it joins the pool the moment the add commits."
            />
          }
        />

        <p className={cx('border-t px-4 py-3', SURFACE.hairline, TYPE.bodyMuted)}>
          {intl.formatMessage(i18nMsg.nodesDesc)}
        </p>
      </Panel>

      <AddNodeDialog
        open={addOpen}
        onClose={() => {
          setAddOpen(false);
          setReassignTarget(null);
        }}
        devices={configuredDevices}
        fleetModels={fleet.models}
        fleetEndpoint={endpoint}
        fleetOnline={fleet.online}
        fleetCount={fleet.lanes.length}
        reassign={reassignTarget}
        onCommitLocal={commitLocalRow}
        onCloudChanged={reloadSwarm}
        onCloudAdded={onCloudAdded}
        onOpenCloudProviders={onOpenCloudProviders}
      />

      <ConfirmationModal
        isOpen={pendingRemove !== null}
        title={intl.formatMessage(i18nMsg.removeTitle)}
        message={
          pendingRemove ? intl.formatMessage(i18nMsg.removeMessage, { id: pendingRemove.id }) : ''
        }
        confirmLabel={intl.formatMessage(i18nMsg.removeConfirm)}
        confirmVariant="destructive"
        isSubmitting={removing}
        onConfirm={() => void confirmRemove()}
        onCancel={() => setPendingRemove(null)}
      />

      {/* Reassignment IS remove + re-add — the confirm names that before the dialog opens. */}
      <ConfirmationModal
        isOpen={pendingReassign !== null}
        title={intl.formatMessage(i18nMsg.reassignTitle)}
        message={
          pendingReassign
            ? intl.formatMessage(i18nMsg.reassignMessage, { id: pendingReassign.id })
            : ''
        }
        confirmLabel={intl.formatMessage(i18nMsg.reassignConfirm)}
        onConfirm={() => {
          if (!pendingReassign) return;
          setReassignTarget({
            id: pendingReassign.id,
            modelId: pendingReassign.modelId,
            provider: pendingReassign.provider,
            engine: pendingReassign.engine,
            // Reassign re-adds the node via the CLI, which carries CONCURRENCY (weight), not share.
            weight: pendingReassign.weight,
          });
          setPendingReassign(null);
          setAddOpen(true);
        }}
        onCancel={() => setPendingReassign(null)}
      />
    </section>
  );
}

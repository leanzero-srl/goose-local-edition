import { useCallback, useEffect, useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { useConfig } from '../ConfigContext';
import { useFleet, deviceFromModelId } from '../swarm/useFleet';
import {
  type SwarmConfig,
  type SwarmDeviceRow,
  type NodeRow,
  nodeRows,
  DEFAULTS,
} from '../settings/swarm/golden';
import { WeightStepper } from './primitives';
import { chipFor, cloudCliErr, LOCAL_CHIP, MLX_CHIP } from './cloud';
import AddNodeDialog, { type ReassignTarget } from './AddNodeDialog';
import { defineMessages, useIntl } from '../../i18n';

const i18nMsg = defineMessages({
  nodesTitle: { id: 'swarmSettings.nodesTitle', defaultMessage: 'Nodes' },
  nodesDesc: {
    id: 'swarmSettings.nodesDesc',
    defaultMessage:
      'Every node the swarm runs — configured rows plus whatever LM Studio has resident right now. Weight: higher gets a bigger share of the tasks.',
  },
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
      '{id} leaves the swarm pool. No model files are touched; you can add it again any time.',
  },
  removeConfirm: { id: 'swarmSettings.removeConfirm', defaultMessage: 'Remove' },
  reassignTitle: { id: 'swarmSettings.reassignTitle', defaultMessage: 'Reassign provider' },
  reassignMessage: {
    id: 'swarmSettings.reassignMessage',
    defaultMessage:
      'Changing what serves {id} works by REMOVING the node and RE-ADDING it under the provider you pick next. Nothing changes until the new add commits.',
  },
  reassignConfirm: { id: 'swarmSettings.reassignConfirm', defaultMessage: 'Pick new provider' },
  noNodes: {
    id: 'swarmSettings.noNodes',
    defaultMessage: 'No nodes yet — add one, or start LM Studio at {endpoint} to be discovered.',
  },
});

const NODE_LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';

/** What serves a node, as a solid chip: cloud provider > LeanZero MLX engine > LM Studio. */
function providerChipOf(row: NodeRow): { seg: string; chip: string } {
  const cloud = chipFor(row.provider);
  if (cloud) return { seg: cloud.seg, chip: cloud.chip };
  if (row.engine === 'mlx-sidecar') return MLX_CHIP;
  return LOCAL_CHIP;
}

/**
 * The Swarm Settings tab — NODES ONLY, per the owner's simplification: "I just want Add node, for
 * each node choose provider, and then for all nodes choose weights. That is it." Per row: label,
 * provider chip (click = the remove+re-add reassign flow), model id, ONE weight (SwarmDevice.weight),
 * remove. No tunables, no toggles — the full lever panel stays in Settings until the golden-formula
 * strip retires it. Cloud rows are mutated ONLY through the engine CLI (the invariant); a weight
 * change on one is therefore CLI rm→add with the new weight.
 */
export default function SwarmNodesSection() {
  const intl = useIntl();
  const { read, upsert } = useConfig();
  const fleet = useFleet();
  const [cfg, setCfg] = useState<SwarmConfig>(DEFAULTS);

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
  const [busyRow, setBusyRow] = useState<string | null>(null);
  const [nodeError, setNodeError] = useState<string | null>(null);

  /** Fresh-read the config and mutate its device list — never from possibly-stale panel state. */
  const mutateDevicesFresh = useCallback(
    async (mutate: (devices: SwarmDeviceRow[]) => SwarmDeviceRow[]) => {
      const raw = (await read('swarm', false)) as SwarmConfig | null;
      const base: SwarmConfig = { ...DEFAULTS, ...(raw ?? {}) };
      const devices = Array.isArray(base.devices) ? [...base.devices] : [];
      const next = { ...base, devices: mutate(devices) };
      await upsert('swarm', next, false);
      setCfg(next);
    },
    [read, upsert]
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
   * ONE weight per node = SwarmDevice.weight.
   *  - configured local rows: rewrite the row in place;
   *  - discovered LM Studio rows: materialize a device row so the weight has somewhere to live
   *    (id = the machine short name when free, else the model id — the pool CLI's own shape);
   *  - cloud rows: the CLI owns them (the invariant) — rm then re-add with the new weight; a
   *    failed re-add is reported LOUDLY because the node is then out of the pool.
   */
  const setNodeWeight = useCallback(
    (row: NodeRow, w: number) => {
      setNodeError(null);
      if (row.provider) {
        setBusyRow(row.id);
        void (async () => {
          try {
            const rm = await window.electron.swarmCloud(row.provider as string, [
              'rm',
              row.modelId,
            ]);
            if (!rm.ok) throw new Error(cloudCliErr(rm));
            const add = await window.electron.swarmCloud(row.provider as string, [
              'add',
              row.modelId,
              '--weight',
              String(w),
            ]);
            if (!add.ok) {
              throw new Error(
                `${cloudCliErr(add)} — the node was removed and the re-add failed; add it back from “Add node”.`
              );
            }
            await reloadSwarm();
          } catch (e) {
            setNodeError(e instanceof Error ? e.message : String(e));
            await reloadSwarm();
          } finally {
            setBusyRow(null);
          }
        })();
        return;
      }
      void mutateDevicesFresh((devices) => {
        if (row.configured) {
          return devices.map((d) => (d.id === row.id ? { ...d, weight: w } : d));
        }
        const machine = deviceFromModelId(row.modelId) || row.modelId;
        const id = devices.some((d) => d.id === machine) ? row.modelId : machine;
        return [...devices, { id, model_id: row.modelId, weight: w, enabled: true }];
      }).catch((e: unknown) => setNodeError(e instanceof Error ? e.message : String(e)));
    },
    [mutateDevicesFresh, reloadSwarm]
  );

  const configuredDevices: SwarmDeviceRow[] = Array.isArray(cfg.devices) ? cfg.devices : [];
  // EVERY node the swarm would actually run, in one list. `nodeRows` (golden.ts) owns the union so
  // the test exercises the shipped rule rather than a copy of it.
  const rows = nodeRows(configuredDevices, fleet.models);
  const weightOf = (row: NodeRow): number => {
    if (row.configured) return row.weight;
    const machine = deviceFromModelId(row.modelId);
    return configuredDevices.find((d) => d.id === machine)?.weight ?? row.weight;
  };

  const endpoint = cfg.endpoint ?? DEFAULTS.endpoint ?? '';

  return (
    <section id="swarm-nodes" className="flex flex-col gap-4 pb-8">
      <div className="overflow-hidden rounded border border-border-primary">
        <div className="flex flex-wrap items-center gap-2 border-b border-border-primary bg-background-secondary px-3 py-2">
          <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
            {intl.formatMessage(i18nMsg.nodesTitle)}
          </span>
          <span className="text-xs text-text-secondary">{intl.formatMessage(i18nMsg.nodesDesc)}</span>
          <button
            type="button"
            data-testid="swarm-add-node"
            onClick={() => {
              setReassignTarget(null);
              setAddOpen(true);
            }}
            className="ml-auto flex shrink-0 items-center gap-1.5 rounded px-3 py-1.5 text-xs font-bold text-white hover:opacity-90"
            style={{ backgroundColor: '#2e8bff' }}
          >
            <Plus className="h-3.5 w-3.5" />
            {intl.formatMessage(i18nMsg.addNode)}
          </button>
        </div>

        <div className="flex flex-col gap-1.5 px-3 py-3">
          {nodeError && (
            <div
              className="rounded px-3 py-2 text-xs font-semibold text-white"
              style={{ backgroundColor: '#e5484d' }}
              role="alert"
            >
              {nodeError}
            </div>
          )}

          {rows.length === 0 ? (
            <div className="rounded border border-border-primary px-3 py-4 text-center text-sm text-text-secondary">
              {intl.formatMessage(i18nMsg.noNodes, { endpoint })}
            </div>
          ) : (
            rows.map((row, idx) => {
              const chip = providerChipOf(row);
              const isCloud = row.provider != null;
              const isRemoteMlx = row.engine === 'mlx-sidecar' && row.host != null;
              const name = isCloud ? row.modelId : deviceFromModelId(row.modelId) || row.id;
              return (
                <div
                  key={row.id}
                  data-testid={`swarm-node-${row.id}`}
                  className="flex items-center justify-between gap-3 rounded border border-border-primary px-2.5 py-1.5"
                >
                  <span className="min-w-0 flex items-center gap-2">
                    <span className="w-12 shrink-0 text-[10px] font-bold uppercase tracking-wide text-text-secondary">
                      Node {NODE_LETTERS[idx] ?? '+'}
                    </span>
                    <button
                      type="button"
                      disabled={!row.configured}
                      onClick={() => {
                        if (!row.configured) return;
                        setPendingReassign(row);
                      }}
                      title={row.configured ? intl.formatMessage(i18nMsg.reassignAria) : undefined}
                      aria-label={`${chip.seg}: ${row.id}`}
                      className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold text-background-primary"
                      style={{ backgroundColor: chip.chip }}
                    >
                      {chip.seg.toUpperCase()}
                    </button>
                    {!row.configured && (
                      <span
                        className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-background-primary"
                        style={{ backgroundColor: '#64748b' }}
                        title={intl.formatMessage(i18nMsg.autoChipTitle)}
                      >
                        {intl.formatMessage(i18nMsg.autoChip)}
                      </span>
                    )}
                    {isRemoteMlx && (
                      <span
                        className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide"
                        style={{ backgroundColor: '#f5a623', color: '#1a1a1a' }}
                        title={intl.formatMessage(i18nMsg.awaitingChipTitle)}
                        data-testid={`awaiting-routing-${row.id}`}
                      >
                        {intl.formatMessage(i18nMsg.awaitingChip)}
                      </span>
                    )}
                    <span className="truncate font-mono text-sm text-text-primary" title={row.modelId}>
                      {name}
                    </span>
                    {!isCloud && (
                      <span
                        className="hidden truncate font-mono text-xs text-text-secondary md:inline"
                        title={row.modelId}
                      >
                        {row.modelId}
                      </span>
                    )}
                  </span>
                  <span className="flex shrink-0 items-center gap-2">
                    {busyRow === row.id ? (
                      <span className="text-xs text-text-secondary">…</span>
                    ) : (
                      <WeightStepper
                        value={weightOf(row)}
                        onChange={(v) => setNodeWeight(row, v)}
                        label={row.id}
                      />
                    )}
                    {row.configured && (
                      <button
                        type="button"
                        onClick={() => {
                          setNodeError(null);
                          setPendingRemove(row);
                        }}
                        aria-label={`${intl.formatMessage(i18nMsg.removeAria)}: ${row.id}`}
                        className="flex h-6 w-6 items-center justify-center rounded text-white hover:opacity-90"
                        style={{ backgroundColor: '#e5484d' }}
                      >
                        <Trash2 className="h-3 w-3" />
                      </button>
                    )}
                  </span>
                </div>
              );
            })
          )}
        </div>
      </div>

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
            weight: weightOf(pendingReassign),
          });
          setPendingReassign(null);
          setAddOpen(true);
        }}
        onCancel={() => setPendingReassign(null)}
      />
    </section>
  );
}

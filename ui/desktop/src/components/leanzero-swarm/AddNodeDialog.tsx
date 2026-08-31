import { useCallback, useEffect, useMemo, useState } from 'react';
import { Loader2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Select } from '../ui/Select';
import { errorMessage } from '../../utils/conversionUtils';
import {
  mlxEngineModelsList,
  mlxEngineSettingsRead,
  mlxEngineSettingsUpdate,
  type MlxLocalModel,
} from '../../acp/mlx-engine';
import { sanitizeSettingsForWrite } from './MlxEngineView';
import { AMBER, AZURE, GREEN, INK_DARK, SolidBanner, WeightStepper } from './primitives';
import { CLOUD_PROVIDERS, CloudPane, MLX_CHIP, LOCAL_CHIP, type CloudProviderDef } from './cloud';
import {
  addableMlxMachines,
  mlxDeviceRow,
  mlxRemoteDeviceRow,
  mlxServedAlias,
  sanitizeNodeLabel,
  type SwarmMachine,
} from './nodes';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { acpListProviderDetails } from '../../acp/providers';
import { useLmStudioFleetVisible } from '../../hooks/useLmStudioFleetVisible';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  title: { id: 'addNode.title', defaultMessage: 'Add node' },
  reassignTitle: { id: 'addNode.reassignTitle', defaultMessage: 'Reassign {id}' },
  description: {
    id: 'addNode.description',
    defaultMessage: 'A node is made by choosing what serves it: pick a provider, then a model.',
  },
  reassignNotice: {
    id: 'addNode.reassignNotice',
    defaultMessage:
      'Reassigning changes what serves this node: {id} is REMOVED from the pool and re-added under the provider you pick here.',
  },
  providerLabel: { id: 'addNode.providerLabel', defaultMessage: 'Provider' },
  providerPlaceholder: { id: 'addNode.providerPlaceholder', defaultMessage: 'Pick a provider…' },
  providersCaption: {
    id: 'addNode.providersCaption',
    defaultMessage:
      'Node providers are the ones the swarm engine supports — more cloud families arrive with engine support.',
  },
  noKeyBadge: { id: 'addNode.noKeyBadge', defaultMessage: 'no key' },
  noKeyPane: {
    id: 'addNode.noKeyPane',
    defaultMessage:
      '{label} has no API key on this machine yet. Add it under Cloud Providers, then come back to add the node.',
  },
  configureCloud: {
    id: 'addNode.configureCloud',
    defaultMessage: 'Configure in Cloud Providers',
  },
  mlxOneModel: {
    id: 'addNode.mlxOneModel',
    defaultMessage:
      'One LeanZero MLX engine serves ONE model: every MLX node shares whatever the engine has mounted, so a different model per node is not possible yet. Adding this node points the engine at the model you pick here.',
  },
  machineLabel: { id: 'addNode.machineLabel', defaultMessage: 'Machine — one MLX node each' },
  machinePlaceholder: { id: 'addNode.machinePlaceholder', defaultMessage: 'Pick a machine…' },
  machineCap: {
    id: 'addNode.machineCap',
    defaultMessage:
      '{count, plural, one {# swarm machine} other {# swarm machines}} without an MLX node yet — each machine can carry exactly one.',
  },
  machineLocalTag: { id: 'addNode.machineLocalTag', defaultMessage: 'this machine' },
  machineRemoteTag: { id: 'addNode.machineRemoteTag', defaultMessage: 'remote' },
  machineNoneDiscovered: {
    id: 'addNode.machineNoneDiscovered',
    defaultMessage:
      'No swarm machines discovered (LM Studio / lms unreachable) — the node is created for THIS machine; name it below.',
  },
  machineAllTaken: {
    id: 'addNode.machineAllTaken',
    defaultMessage:
      'Every discovered swarm machine already has its MLX node — remove one first, or add cloud nodes (those are unlimited).',
  },
  remoteAwaiting: {
    id: 'addNode.remoteAwaiting',
    defaultMessage:
      'A remote machine’s MLX node is saved to the pool but AWAITS FLEET ROUTING — the per-node engine endpoints ship in a later phase. Only this machine’s node is served today.',
  },
  mlxLabelLabel: { id: 'addNode.mlxLabelLabel', defaultMessage: 'Node label' },
  mlxLabelPlaceholder: { id: 'addNode.mlxLabelPlaceholder', defaultMessage: 'e.g. workhorse' },
  mlxModelLabel: {
    id: 'addNode.mlxModelLabel',
    defaultMessage: 'Model — from the engine’s models folder',
  },
  mlxModelPlaceholder: { id: 'addNode.mlxModelPlaceholder', defaultMessage: 'Pick a model…' },
  mlxNoModels: {
    id: 'addNode.mlxNoModels',
    defaultMessage:
      'No complete models in the engine’s models folder yet — download one in the LeanZero MLX tab first.',
  },
  mlxAliasPreview: {
    id: 'addNode.mlxAliasPreview',
    defaultMessage: 'Served model id: {alias}',
  },
  duplicateId: {
    id: 'addNode.duplicateId',
    defaultMessage: 'A node named {id} already exists — pick a different label.',
  },
  weightLabel: {
    id: 'addNode.weightLabel',
    defaultMessage: 'Weight — higher gets a bigger share of the tasks',
  },
  lmstudioAuto: {
    id: 'addNode.lmstudioAuto',
    defaultMessage:
      'LM Studio nodes are discovered automatically: every model resident on the fleet at {endpoint} joins the pool by itself, so there is nothing to add by hand. Load a model in LM Studio (or LM Link) and it appears in the Nodes list.',
  },
  lmstudioLive: {
    id: 'addNode.lmstudioLive',
    defaultMessage: '{count, plural, one {# node} other {# nodes}} live right now',
  },
  lmstudioOffline: { id: 'addNode.lmstudioOffline', defaultMessage: 'fleet offline' },
  addButton: { id: 'addNode.addButton', defaultMessage: 'Add node' },
  reassignButton: { id: 'addNode.reassignButton', defaultMessage: 'Remove & re-add' },
  cancel: { id: 'addNode.cancel', defaultMessage: 'Cancel' },
  done: { id: 'addNode.done', defaultMessage: 'Done' },
});

export interface ReassignTarget {
  id: string;
  modelId: string;
  provider: string | null;
  engine: string | null;
  weight: number;
}

export interface ProviderOption {
  value: string; // 'mlx' | 'lmstudio' | cloud cli name
  label: string;
  chip: string;
  /** Cloud rows only: does this machine hold the provider's key (acpListProviderDetails joined on
   *  CLOUD_PROVIDERS.registry)? false renders the explicit "no key — configure in Cloud Providers"
   *  state instead of the add pane. Non-cloud rows and an unreadable provider list are true — the
   *  CloudPane's own engine-side check remains the last word, so nothing dead-ends on a stale read. */
  configured: boolean;
}

// Pass E follow-up (owner): LM Studio leaves the add-node provider list — discovery was automatic
// anyway, so the entry only ever explained itself. Hidden, not deleted: the pane and this flag stay,
// and even when re-enabled the entry still rides the runtime showLmStudioFleet setting.
export const SHOW_LMSTUDIO_PROVIDER = false;

/**
 * The provider choices, DERIVED — never a hardcoded list (owner): [LeanZero MLX] first, then every
 * engine-supported cloud family from the ONE CLOUD_PROVIDERS mirror, each joined with this
 * machine's actual configuration state. Configure a new key for an engine-supported provider and
 * its row flips to selectable with no code change; a cloud family the engine does not support
 * cannot be offered at all (the pool CLI would refuse it).
 *
 * `configuredRegistryIds` = provider-registry ids reported configured by acpListProviderDetails
 * (null = the list could not be read; rows stay selectable and the engine-side check governs).
 */
export function deriveProviderOptions(
  configuredRegistryIds: ReadonlySet<string> | null,
  includeLmStudio: boolean
): ProviderOption[] {
  return [
    { value: 'mlx', label: MLX_CHIP.seg, chip: MLX_CHIP.chip, configured: true },
    ...(includeLmStudio
      ? [{ value: 'lmstudio', label: LOCAL_CHIP.seg, chip: LOCAL_CHIP.chip, configured: true }]
      : []),
    ...CLOUD_PROVIDERS.map((c) => ({
      value: c.cli,
      label: c.label,
      chip: c.chip,
      configured: configuredRegistryIds == null ? true : configuredRegistryIds.has(c.registry),
    })),
  ];
}

interface MachineOption {
  value: string;
  label: string;
  local: boolean;
}

interface MlxModelOption {
  value: string;
  label: string;
  model: MlxLocalModel;
}

/**
 * The "+ Add node" flow — a custom dialog (never a native primitive) that walks provider → model:
 *
 *  - LeanZero MLX: MACHINE-CAPPED (owner amendment). The swarm's machines are enumerated live
 *    (`lms ps` via the fleet-machines IPC ∪ the LM Link model-id prefixes), each machine can carry
 *    exactly ONE MLX node, and the picker offers exactly the machines that lack one. The LOCAL
 *    machine's node is fully served: the add aligns mlx_engine.model_id/served_model_name AND
 *    writes the device row (engine:'mlx-sidecar', model_id = the served alias) in one motion. A
 *    REMOTE machine's node writes the same row plus host = the machine, and the pool renders it
 *    "awaiting fleet routing" — never as reachable. With no machines discovered at all, the node
 *    is created for THIS machine under a hand-typed label (the pre-discovery behavior).
 *  - Cloud providers: the existing CLI-driven pane (key if missing → live roster → add) — the
 *    desktop NEVER upserts a cloud device row itself. Cloud nodes are unlimited.
 *  - LM Studio: NOT OFFERED any more (SHOW_LMSTUDIO_PROVIDER) — discovery was automatic, so the
 *    entry only ever explained itself; its pane stays in code behind the flag.
 *
 * Reassignment reuses this dialog: the old row is removed and the new one added when the new
 * provider's add commits — never before, so a cancelled reassign changes nothing.
 */
export default function AddNodeDialog({
  open,
  onClose,
  devices,
  fleetModels,
  fleetEndpoint,
  fleetOnline,
  fleetCount,
  reassign,
  onCommitLocal,
  onCloudChanged,
  onCloudAdded,
  onOpenCloudProviders,
}: {
  open: boolean;
  onClose: () => void;
  devices: SwarmDeviceRow[];
  fleetModels: string[];
  fleetEndpoint: string;
  fleetOnline: boolean;
  fleetCount: number;
  reassign?: ReassignTarget | null;
  /** Write a LOCAL (mlx-sidecar) device row into the swarm config; in reassign mode the parent
   *  removes the old row in the same write. Throws on failure. */
  onCommitLocal: (row: SwarmDeviceRow) => Promise<void>;
  /** Re-read the swarm config after a successful engine-side cloud mutation. */
  onCloudChanged: () => Promise<void>;
  /** A cloud ADD landed — in reassign mode the parent removes the old row here. */
  onCloudAdded: (modelId: string) => Promise<void>;
  /** Deep-link for the "no key" state: closes the dialog and opens the Cloud Providers tab. */
  onOpenCloudProviders?: () => void;
}) {
  const intl = useIntl();
  const [provider, setProvider] = useState<string | null>(null);
  const [machines, setMachines] = useState<SwarmMachine[] | null>(null);
  const [machine, setMachine] = useState<string | null>(null);
  const [label, setLabel] = useState('');
  const [mlxModels, setMlxModels] = useState<MlxLocalModel[] | null>(null);
  const [mlxModelsError, setMlxModelsError] = useState<string | null>(null);
  const [mlxModelId, setMlxModelId] = useState<string | null>(null);
  const [weight, setWeight] = useState(2);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Provider-registry ids this machine has configured — read fresh at each dialog open; null
   *  means the read failed (rows stay selectable, the engine-side check governs). */
  const [configuredProviders, setConfiguredProviders] = useState<ReadonlySet<string> | null>(null);
  const lmStudioVisible = useLmStudioFleetVisible();

  // Reset per open; a reassign seeds the label/weight from the node being reassigned.
  useEffect(() => {
    if (!open) return;
    setProvider(null);
    setError(null);
    setBusy(false);
    setMlxModelId(null);
    setMachine(null);
    setMachines(null);
    if (reassign) {
      setLabel(reassign.id.replace(/-mlx$/, ''));
      setWeight(Math.max(1, Math.min(9, reassign.weight)));
    } else {
      setLabel('');
      setWeight(2);
    }
  }, [open, reassign]);

  // The configured-provider join, fresh per open: which engine-supported cloud families actually
  // hold a key on this machine right now.
  useEffect(() => {
    if (!open) return undefined;
    let alive = true;
    void (async () => {
      try {
        const details = await acpListProviderDetails();
        if (alive) {
          setConfiguredProviders(
            new Set(details.filter((d) => d.is_configured).map((d) => d.name))
          );
        }
      } catch {
        if (alive) setConfiguredProviders(null);
      }
    })();
    return () => {
      alive = false;
    };
  }, [open]);

  // MLX pane data: the engine's local models + the swarm's machines, loaded when the pane opens.
  useEffect(() => {
    if (!open || provider !== 'mlx') return;
    if (mlxModels == null) {
      void (async () => {
        try {
          const list = await mlxEngineModelsList();
          setMlxModels(list.models);
          setMlxModelsError(null);
        } catch (e) {
          setMlxModels([]);
          setMlxModelsError(errorMessage(e, 'Could not list the engine’s local models.'));
        }
      })();
    }
    if (machines == null) {
      void (async () => {
        try {
          setMachines(await window.electron.fleetMachines());
        } catch {
          setMachines([]);
        }
      })();
    }
  }, [open, provider, mlxModels, machines]);

  const providerOptions = useMemo(
    () => deriveProviderOptions(configuredProviders, SHOW_LMSTUDIO_PROVIDER && lmStudioVisible),
    [configuredProviders, lmStudioVisible]
  );
  const selectedProvider = providerOptions.find((o) => o.value === provider) ?? null;
  const activeCloud: CloudProviderDef | undefined = CLOUD_PROVIDERS.find(
    (c) => c.cli === provider
  );
  // The explicit no-key STATE: the row is pickable, and picking it explains + deep-links instead
  // of pretending an add pane could work without a key.
  const activeCloudUnconfigured = !!activeCloud && selectedProvider?.configured === false;

  // The machine cap: one MLX node per swarm machine, minus those already added. In reassign mode
  // the node being reassigned does not block its own machine.
  const capDevices = useMemo(
    () => (reassign ? devices.filter((d) => d.id !== reassign.id) : devices),
    [devices, reassign]
  );
  const anyDiscovery = (machines?.length ?? 0) > 0 || fleetModels.length > 0;
  const addable = useMemo(
    () => addableMlxMachines(machines ?? [], fleetModels, capDevices),
    [machines, fleetModels, capDevices]
  );
  const machineOptions: MachineOption[] = addable.map((m) => ({
    value: m.machine,
    label: m.machine,
    local: m.local,
  }));
  const selectedMachine = machineOptions.find((o) => o.value === machine) ?? null;
  const selectedMachineIsLocal = selectedMachine?.local ?? false;
  // No discovery at all -> the manual local-label path keeps the local node creatable.
  const manualLocalPath = machines != null && !anyDiscovery;

  const effectiveLabel = manualLocalPath ? sanitizeNodeLabel(label) : (machine ?? '');
  const effectiveIsLocal = manualLocalPath ? true : selectedMachineIsLocal;
  const aliasPreview =
    provider === 'mlx' && effectiveLabel && mlxModelId
      ? mlxServedAlias(effectiveLabel, mlxModelId)
      : null;
  const duplicateMlxId =
    provider === 'mlx' && effectiveLabel
      ? capDevices.find((d) => d.id === `${effectiveLabel}-mlx`)
      : undefined;

  const addMlx = useCallback(async () => {
    if (!effectiveLabel || !mlxModelId || duplicateMlxId) return;
    setBusy(true);
    setError(null);
    try {
      if (effectiveIsLocal) {
        const row = mlxDeviceRow(effectiveLabel, mlxModelId, weight);
        // Align the ENGINE first: the alias only means something once the engine serves it. A
        // failed settings write leaves the swarm config untouched — no half-state.
        const settings = await mlxEngineSettingsRead();
        const next = sanitizeSettingsForWrite(settings);
        next.modelId = mlxModelId;
        next.servedModelName = row.model_id;
        await mlxEngineSettingsUpdate(next);
        await onCommitLocal(row);
      } else {
        // REMOTE machine: the row is pool state only (host = machine, awaiting fleet routing);
        // the local engine's alias contract is never touched.
        await onCommitLocal(mlxRemoteDeviceRow(effectiveLabel, mlxModelId, weight));
      }
      onClose();
    } catch (e) {
      setError(errorMessage(e, 'Could not add the node.'));
    } finally {
      setBusy(false);
    }
  }, [effectiveLabel, effectiveIsLocal, mlxModelId, duplicateMlxId, weight, onCommitLocal, onClose]);

  const mlxOptions: MlxModelOption[] = (mlxModels ?? [])
    .filter((m) => m.complete)
    .map((m) => ({ value: m.id, label: m.id, model: m }));
  const selectedMlxModel = mlxOptions.find((o) => o.value === mlxModelId) ?? null;

  const cloudDevices = activeCloud
    ? devices.filter((d) => d.provider === activeCloud.cli)
    : [];

  const mlxReady =
    provider === 'mlx' && !!effectiveLabel && !!mlxModelId && !duplicateMlxId && !busy;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[640px]">
        <DialogHeader>
          <DialogTitle>
            {reassign
              ? intl.formatMessage(i18n.reassignTitle, { id: reassign.id })
              : intl.formatMessage(i18n.title)}
          </DialogTitle>
          <DialogDescription>{intl.formatMessage(i18n.description)}</DialogDescription>
        </DialogHeader>

        {reassign && (
          <SolidBanner
            color={AMBER}
            label="reassign"
            text={intl.formatMessage(i18n.reassignNotice, { id: reassign.id })}
          />
        )}

        <div className="flex flex-col gap-1">
          <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
            {intl.formatMessage(i18n.providerLabel)}
          </span>
          <Select
            aria-label={intl.formatMessage(i18n.providerLabel)}
            options={providerOptions}
            value={selectedProvider}
            placeholder={intl.formatMessage(i18n.providerPlaceholder)}
            formatOptionLabel={(o) => {
              const opt = o as ProviderOption;
              return (
                <span className="flex items-center gap-2">
                  <span
                    className="inline-block h-3 w-3 shrink-0 rounded-sm"
                    style={{ backgroundColor: opt.chip }}
                  />
                  <span className="text-sm">{opt.label}</span>
                  {!opt.configured && (
                    <span
                      className="rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide"
                      style={{ backgroundColor: AMBER, color: INK_DARK }}
                      data-testid={`provider-no-key-${opt.value}`}
                    >
                      {intl.formatMessage(i18n.noKeyBadge)}
                    </span>
                  )}
                </span>
              );
            }}
            onChange={(o) => {
              setProvider(o ? (o as ProviderOption).value : null);
              setError(null);
            }}
          />
          <span className="text-xs text-text-secondary">
            {intl.formatMessage(i18n.providersCaption)}
          </span>
        </div>

        {provider === 'mlx' && (
          <div className="flex flex-col gap-3" data-testid="add-node-mlx-pane">
            <SolidBanner
              color={MLX_CHIP.chip}
              label={MLX_CHIP.seg}
              text={intl.formatMessage(i18n.mlxOneModel)}
            />

            {machines == null ? (
              <span className="text-sm text-text-secondary">
                <Loader2 className="mr-1 inline h-3.5 w-3.5 animate-spin" />…
              </span>
            ) : manualLocalPath ? (
              <>
                <span className="text-sm text-text-secondary">
                  {intl.formatMessage(i18n.machineNoneDiscovered)}
                </span>
                <div className="flex flex-col gap-1">
                  <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                    {intl.formatMessage(i18n.mlxLabelLabel)}
                  </span>
                  <Input
                    value={label}
                    onChange={(e) => setLabel(e.target.value)}
                    placeholder={intl.formatMessage(i18n.mlxLabelPlaceholder)}
                    className="rounded font-mono text-sm"
                    aria-label={intl.formatMessage(i18n.mlxLabelLabel)}
                  />
                </div>
              </>
            ) : machineOptions.length === 0 ? (
              <span className="text-sm font-semibold" style={{ color: AMBER }}>
                {intl.formatMessage(i18n.machineAllTaken)}
              </span>
            ) : (
              <div className="flex flex-col gap-1">
                <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                  {intl.formatMessage(i18n.machineLabel)}
                </span>
                <Select
                  aria-label={intl.formatMessage(i18n.machineLabel)}
                  options={machineOptions}
                  value={selectedMachine}
                  placeholder={intl.formatMessage(i18n.machinePlaceholder)}
                  formatOptionLabel={(o) => {
                    const opt = o as MachineOption;
                    return (
                      <span className="flex items-center gap-2">
                        <span className="font-mono text-sm">{opt.label}</span>
                        <span
                          className="rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide"
                          style={{
                            backgroundColor: opt.local ? GREEN : AMBER,
                            color: opt.local ? '#fff' : INK_DARK,
                          }}
                        >
                          {opt.local
                            ? intl.formatMessage(i18n.machineLocalTag)
                            : intl.formatMessage(i18n.machineRemoteTag)}
                        </span>
                      </span>
                    );
                  }}
                  onChange={(o) => setMachine(o ? (o as MachineOption).value : null)}
                />
                <span className="text-xs text-text-secondary">
                  {intl.formatMessage(i18n.machineCap, { count: machineOptions.length })}
                </span>
              </div>
            )}

            {machine != null && !selectedMachineIsLocal && (
              <SolidBanner
                color={AMBER}
                label="awaiting fleet routing"
                text={intl.formatMessage(i18n.remoteAwaiting)}
              />
            )}

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className="flex flex-col gap-1">
                <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                  {intl.formatMessage(i18n.mlxModelLabel)}
                </span>
                {mlxModels != null && mlxOptions.length === 0 && !mlxModelsError ? (
                  <span className="text-sm text-text-secondary">
                    {intl.formatMessage(i18n.mlxNoModels)}
                  </span>
                ) : (
                  <Select
                    aria-label={intl.formatMessage(i18n.mlxModelLabel)}
                    options={mlxOptions}
                    value={selectedMlxModel}
                    isLoading={mlxModels == null}
                    placeholder={intl.formatMessage(i18n.mlxModelPlaceholder)}
                    onChange={(o) => setMlxModelId(o ? (o as MlxModelOption).value : null)}
                  />
                )}
                {mlxModelsError && (
                  <span className="text-xs font-semibold" style={{ color: '#e5484d' }}>
                    {mlxModelsError}
                  </span>
                )}
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                  {intl.formatMessage(i18n.weightLabel)}
                </span>
                <WeightStepper value={weight} onChange={setWeight} />
              </div>
            </div>

            {aliasPreview && (
              <span className="font-mono text-xs" style={{ color: AZURE }}>
                {intl.formatMessage(i18n.mlxAliasPreview, { alias: aliasPreview })}
              </span>
            )}
            {duplicateMlxId && (
              <span className="text-xs font-semibold" style={{ color: '#e5484d' }}>
                {intl.formatMessage(i18n.duplicateId, { id: duplicateMlxId.id })}
              </span>
            )}
          </div>
        )}

        {provider === 'lmstudio' && (
          <div className="flex flex-col gap-2">
            <p className="text-sm text-text-secondary">
              {intl.formatMessage(i18n.lmstudioAuto, { endpoint: fleetEndpoint })}
            </p>
            <span
              className="self-start rounded px-2 py-0.5 text-xs font-bold"
              style={{
                backgroundColor: fleetOnline ? GREEN : AMBER,
                color: fleetOnline ? '#fff' : INK_DARK,
              }}
            >
              {fleetOnline
                ? intl.formatMessage(i18n.lmstudioLive, { count: fleetCount })
                : intl.formatMessage(i18n.lmstudioOffline)}
            </span>
          </div>
        )}

        {activeCloudUnconfigured && activeCloud && (
          <div className="flex flex-col gap-3" data-testid="add-node-no-key-pane">
            <SolidBanner
              color={AMBER}
              label={intl.formatMessage(i18n.noKeyBadge)}
              text={intl.formatMessage(i18n.noKeyPane, { label: activeCloud.label })}
            />
            <Button
              onClick={() => {
                onClose();
                onOpenCloudProviders?.();
              }}
              className="self-start rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: activeCloud.chip }}
              data-testid="add-node-configure-cloud"
            >
              {intl.formatMessage(i18n.configureCloud)}
            </Button>
          </div>
        )}

        {activeCloud && !activeCloudUnconfigured && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-3">
              <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                {intl.formatMessage(i18n.weightLabel)}
              </span>
              <WeightStepper value={weight} onChange={setWeight} />
            </div>
            <CloudPane
              key={activeCloud.cli}
              def={activeCloud}
              devices={cloudDevices}
              addWeight={weight}
              onChanged={onCloudChanged}
              onAdded={onCloudAdded}
            />
          </div>
        )}

        {error && <SolidBanner color="#e5484d" label="failed" text={error} />}

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>
            {activeCloud ? intl.formatMessage(i18n.done) : intl.formatMessage(i18n.cancel)}
          </Button>
          {provider === 'mlx' && (
            <Button
              onClick={() => void addMlx()}
              disabled={!mlxReady}
              className="rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: MLX_CHIP.chip }}
              data-testid="add-node-mlx-submit"
            >
              {busy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : null}
              {reassign
                ? intl.formatMessage(i18n.reassignButton)
                : intl.formatMessage(i18n.addButton)}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

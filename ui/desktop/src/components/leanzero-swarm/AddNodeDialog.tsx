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
import { errorMessage } from '../../utils/conversionUtils';
import {
  mlxEngineModelsList,
  mlxEngineSettingsRead,
  mlxEngineSettingsUpdate,
  type MlxLocalModel,
} from '../../acp/mlx-engine';
import { sanitizeSettingsForWrite } from './MlxEngineView';
import { Button, Chip, TYPE, WEIGHT, cx } from '../lz';
import { FIELD_LABEL, INPUT, StudioSelect, ToneBanner, WeightStepper } from './studio';
import { CLOUD_PROVIDERS, CloudPane, MLX_CHIP, LOCAL_CHIP, type CloudProviderDef } from './cloud';
import {
  addableMlxMachines,
  mlxDeviceRow,
  mlxRemoteDeviceRow,
  mlxServedAlias,
  sanitizeNodeLabel,
  swarmMachinesFromLink,
  type SwarmMachine,
} from './nodes';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { acpListProviderDetails } from '../../acp/providers';
import { leanzeroLinkNodes, linkErrorText } from '../../acp/leanzero-link';
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
  configureCloud: {
    id: 'addNode.configureCloud',
    defaultMessage: 'Configure in Cloud Providers',
  },
  mlxOneModel: {
    id: 'addNode.mlxOneModel',
    defaultMessage:
      'One LeanZero MLX engine serves one model at a time, shared by every MLX node on that Mac. Adding a node for this Mac loads nothing and stops nothing: it sets the model swarm builds use on this node, and this Mac answers under the node’s name the next time it serves that model. Chat answers from what this Mac serves now — a model you start in Run it — and the node follows it.',
  },
  machineLabel: { id: 'addNode.machineLabel', defaultMessage: 'Mac — one MLX node each' },
  machinePlaceholder: { id: 'addNode.machinePlaceholder', defaultMessage: 'Pick a Mac…' },
  machineCap: {
    id: 'addNode.machineCap',
    defaultMessage:
      '{count, plural, one {# Mac} other {# Macs}} without an MLX node yet — each Mac carries exactly one.',
  },
  machineLocalTag: { id: 'addNode.machineLocalTag', defaultMessage: 'this Mac' },
  machineRemoteTag: { id: 'addNode.machineRemoteTag', defaultMessage: 'remote' },
  machineNoneDiscovered: {
    id: 'addNode.machineNoneDiscovered',
    defaultMessage:
      'LeanZero Link could not name this Mac ({error}) — the node is created for this Mac; name it below.',
  },
  machineAllTaken: {
    id: 'addNode.machineAllTaken',
    defaultMessage:
      'Every linked Mac already has its MLX node — change its model in the Nodes table or remove it first; cloud nodes are unlimited.',
  },
  machineOnlyThisMac: {
    id: 'addNode.machineOnlyThisMac',
    defaultMessage:
      'Only this Mac is linked — your other Macs appear here once they join LeanZero Link (Providers › My Macs).',
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
  configured: boolean;
}

// Pass E follow-up (owner): LM Studio leaves the add-node provider list — discovery was automatic
// anyway, so the entry only ever explained itself. Hidden, not deleted: the pane and this flag stay,
// and even when re-enabled the entry still rides the runtime showLmStudioFleet setting.
export const SHOW_LMSTUDIO_PROVIDER = false;

/** Only configured cloud adapters are offered; an unreadable list offers none. */
export function deriveProviderOptions(
  configuredRegistryIds: ReadonlySet<string> | null,
  includeLmStudio: boolean
): ProviderOption[] {
  return [
    { value: 'mlx', label: MLX_CHIP.seg, configured: true },
    ...(includeLmStudio ? [{ value: 'lmstudio', label: LOCAL_CHIP.seg, configured: true }] : []),
    ...CLOUD_PROVIDERS.filter((c) => configuredRegistryIds?.has(c.registry)).map((c) => ({
      value: c.cli,
      label: c.label,
      configured: true,
    })),
  ];
}

interface MachineOption {
  value: string;
  label: string;
  name: string;
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
 *  - LeanZero MLX: MAC-CAPPED (owner amendment). The Macs are the LeanZero Link roster, read on
 *    open: this Mac (Link names it even when not connected) and every linked peer; each Mac carries
 *    exactly ONE MLX node, and the picker offers exactly the Macs that lack one. This Mac's add
 *    writes mlx_engine.model_id/served_model_name (the alias this Mac answers under the next time
 *    it serves that model — nothing is loaded or stopped) AND the device row (engine:'mlx-sidecar',
 *    model_id = the alias). Chat does not wait for that: the router follows what this Mac serves
 *    now (Q-128). A peer's node writes the same row plus host = the peer, and the pool renders it
 *    "awaiting fleet routing" — never as reachable. When Link cannot be read at all, the node is
 *    created for this Mac under a hand-typed label, and the dialog says why.
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
  const [machinesError, setMachinesError] = useState<string | null>(null);
  const [machine, setMachine] = useState<string | null>(null);
  const [label, setLabel] = useState('');
  const [mlxModels, setMlxModels] = useState<MlxLocalModel[] | null>(null);
  const [mlxModelsError, setMlxModelsError] = useState<string | null>(null);
  const [mlxModelId, setMlxModelId] = useState<string | null>(null);
  const [weight, setWeight] = useState(2);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Refreshed on each open; null never admits a cloud provider. */
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
    setMachinesError(null);
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
    setConfiguredProviders(null);
    void (async () => {
      try {
        const details = await acpListProviderDetails();
        if (alive) {
          setConfiguredProviders(
            new Set(details.filter((d) => d.is_configured).map((d) => d.name))
          );
        }
      } catch (e) {
        if (alive) {
          setConfiguredProviders(null);
          setError(errorMessage(e, 'Could not load configured providers.'));
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [open]);

  // MLX pane data: the engine's local models + the linked Macs, loaded when the pane opens.
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
    if (machines == null && machinesError == null) {
      void (async () => {
        try {
          setMachines(swarmMachinesFromLink(await leanzeroLinkNodes()));
        } catch (e) {
          setMachinesError(linkErrorText(e));
        }
      })();
    }
  }, [open, provider, mlxModels, machines, machinesError]);

  const providerOptions = useMemo(
    () => deriveProviderOptions(configuredProviders, SHOW_LMSTUDIO_PROVIDER && lmStudioVisible),
    [configuredProviders, lmStudioVisible]
  );
  const selectedProvider = providerOptions.find((o) => o.value === provider) ?? null;
  const activeCloud: CloudProviderDef | undefined = CLOUD_PROVIDERS.find((c) => c.cli === provider);
  // The machine cap: one MLX node per swarm machine, minus those already added. In reassign mode
  // the node being reassigned does not block its own machine.
  const capDevices = useMemo(
    () => (reassign ? devices.filter((d) => d.id !== reassign.id) : devices),
    [devices, reassign]
  );
  const addable = useMemo(
    () => addableMlxMachines(machines ?? [], capDevices),
    [machines, capDevices]
  );
  const machineOptions: MachineOption[] = addable.map((m) => ({
    value: m.machine,
    label: m.machine,
    name: m.name,
    local: m.local,
  }));
  const selectedMachine = machineOptions.find((o) => o.value === machine) ?? null;
  const selectedMachineIsLocal = selectedMachine?.local ?? false;
  const onlyThisMac = machines != null && machines.every((m) => m.local);
  // Link could not be read -> the manual label path keeps this Mac's node creatable.
  const manualLocalPath = machinesError != null;

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
  }, [
    effectiveLabel,
    effectiveIsLocal,
    mlxModelId,
    duplicateMlxId,
    weight,
    onCommitLocal,
    onClose,
  ]);

  const mlxOptions: MlxModelOption[] = (mlxModels ?? [])
    .filter((m) => m.complete)
    .map((m) => ({ value: m.id, label: m.id, model: m }));
  const selectedMlxModel = mlxOptions.find((o) => o.value === mlxModelId) ?? null;

  const cloudDevices = activeCloud ? devices.filter((d) => d.provider === activeCloud.cli) : [];

  const mlxReady =
    provider === 'mlx' && !!effectiveLabel && !!mlxModelId && !duplicateMlxId && !busy;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[640px]">
        <DialogHeader>
          <DialogTitle className={TYPE.h1}>
            {reassign
              ? intl.formatMessage(i18n.reassignTitle, { id: reassign.id })
              : intl.formatMessage(i18n.title)}
          </DialogTitle>
          <DialogDescription className={TYPE.bodyMuted}>
            {intl.formatMessage(i18n.description)}
          </DialogDescription>
        </DialogHeader>

        {reassign && (
          <ToneBanner
            tone="warn"
            label="Reassign"
            text={intl.formatMessage(i18n.reassignNotice, { id: reassign.id })}
          />
        )}

        <div className="flex flex-col gap-1.5">
          <span className={FIELD_LABEL}>{intl.formatMessage(i18n.providerLabel)}</span>
          <StudioSelect
            aria-label={intl.formatMessage(i18n.providerLabel)}
            options={providerOptions}
            value={selectedProvider}
            placeholder={intl.formatMessage(i18n.providerPlaceholder)}
            renderOption={(opt) => <span>{opt.label}</span>}
            onChange={(o) => {
              setProvider(o ? o.value : null);
              setError(null);
            }}
          />
          <span className={TYPE.meta}>{intl.formatMessage(i18n.providersCaption)}</span>
          {onOpenCloudProviders && (
            <Button
              variant="secondary"
              className="self-start"
              data-testid="add-node-configure-cloud"
              onClick={() => {
                onClose();
                onOpenCloudProviders();
              }}
            >
              {intl.formatMessage(i18n.configureCloud)}
            </Button>
          )}
        </div>

        {provider === 'mlx' && (
          <div className="flex flex-col gap-3" data-testid="add-node-mlx-pane">
            <ToneBanner
              tone="accent"
              label={MLX_CHIP.seg}
              text={intl.formatMessage(i18n.mlxOneModel)}
            />

            {manualLocalPath ? (
              <>
                <span className={TYPE.bodyMuted} data-testid="add-node-machine-manual">
                  {intl.formatMessage(i18n.machineNoneDiscovered, { error: machinesError })}
                </span>
                <div className="flex flex-col gap-1.5">
                  <span className={FIELD_LABEL}>{intl.formatMessage(i18n.mlxLabelLabel)}</span>
                  <input
                    value={label}
                    onChange={(e) => setLabel(e.target.value)}
                    placeholder={intl.formatMessage(i18n.mlxLabelPlaceholder)}
                    className={cx(INPUT, 'w-full font-mono')}
                    aria-label={intl.formatMessage(i18n.mlxLabelLabel)}
                  />
                </div>
              </>
            ) : machines == null ? (
              <span className={cx('flex items-center gap-2', TYPE.bodyMuted)}>
                <Loader2 className="size-3.5 animate-spin text-lz-accent" />…
              </span>
            ) : machineOptions.length === 0 ? (
              <span className={cx('text-lz-body text-lz-warn', WEIGHT.medium)}>
                {intl.formatMessage(i18n.machineAllTaken)}
              </span>
            ) : (
              <div className="flex flex-col gap-1.5">
                <span className={FIELD_LABEL}>{intl.formatMessage(i18n.machineLabel)}</span>
                <StudioSelect
                  aria-label={intl.formatMessage(i18n.machineLabel)}
                  options={machineOptions}
                  value={selectedMachine}
                  placeholder={intl.formatMessage(i18n.machinePlaceholder)}
                  renderOption={(opt) => (
                    <span className="flex items-center gap-2">
                      <span>{opt.name}</span>
                      <span className="font-mono text-lz-mono text-lz-ink-3">{opt.label}</span>
                      {opt.local ? (
                        <Chip tone="ok">{intl.formatMessage(i18n.machineLocalTag)}</Chip>
                      ) : (
                        <Chip>{intl.formatMessage(i18n.machineRemoteTag)}</Chip>
                      )}
                    </span>
                  )}
                  onChange={(o) => setMachine(o ? o.value : null)}
                />
                <span className={TYPE.meta}>
                  {intl.formatMessage(i18n.machineCap, { count: machineOptions.length })}
                </span>
              </div>
            )}

            {onlyThisMac && (
              <span className={TYPE.meta} data-testid="add-node-only-this-mac">
                {intl.formatMessage(i18n.machineOnlyThisMac)}
              </span>
            )}

            {machine != null && !selectedMachineIsLocal && (
              <ToneBanner
                tone="stopped"
                label="awaiting fleet routing"
                text={intl.formatMessage(i18n.remoteAwaiting)}
              />
            )}

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className="flex flex-col gap-1.5">
                <span className={FIELD_LABEL}>{intl.formatMessage(i18n.mlxModelLabel)}</span>
                {mlxModels != null && mlxOptions.length === 0 && !mlxModelsError ? (
                  <span className={TYPE.bodyMuted}>{intl.formatMessage(i18n.mlxNoModels)}</span>
                ) : (
                  <StudioSelect
                    aria-label={intl.formatMessage(i18n.mlxModelLabel)}
                    options={mlxOptions}
                    value={selectedMlxModel}
                    loading={mlxModels == null}
                    placeholder={intl.formatMessage(i18n.mlxModelPlaceholder)}
                    renderOption={(opt) => (
                      <span className="truncate font-mono text-lz-mono">{opt.label}</span>
                    )}
                    onChange={(o) => setMlxModelId(o ? o.value : null)}
                  />
                )}
                {mlxModelsError && (
                  <span className={cx('text-lz-meta text-lz-err', WEIGHT.medium)}>
                    {mlxModelsError}
                  </span>
                )}
              </div>
              <div className="flex flex-col gap-1.5">
                <span className={FIELD_LABEL}>{intl.formatMessage(i18n.weightLabel)}</span>
                <WeightStepper value={weight} onChange={setWeight} />
              </div>
            </div>

            {aliasPreview && (
              <span className="font-mono text-lz-mono text-lz-accent">
                {intl.formatMessage(i18n.mlxAliasPreview, { alias: aliasPreview })}
              </span>
            )}
            {duplicateMlxId && (
              <span className={cx('text-lz-meta text-lz-err', WEIGHT.medium)}>
                {intl.formatMessage(i18n.duplicateId, { id: duplicateMlxId.id })}
              </span>
            )}
          </div>
        )}

        {provider === 'lmstudio' && (
          <div className="flex flex-col gap-2">
            <p className={TYPE.bodyMuted}>
              {intl.formatMessage(i18n.lmstudioAuto, { endpoint: fleetEndpoint })}
            </p>
            <Chip tone={fleetOnline ? 'ok' : 'warn'} className="self-start">
              {fleetOnline
                ? intl.formatMessage(i18n.lmstudioLive, { count: fleetCount })
                : intl.formatMessage(i18n.lmstudioOffline)}
            </Chip>
          </div>
        )}

        {activeCloud && selectedProvider && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-3">
              <span className={FIELD_LABEL}>{intl.formatMessage(i18n.weightLabel)}</span>
              <WeightStepper value={weight} onChange={setWeight} />
            </div>
            <CloudPane
              allowKeySetup={false}
              key={activeCloud.cli}
              def={activeCloud}
              devices={cloudDevices}
              addWeight={weight}
              onChanged={onCloudChanged}
              onAdded={onCloudAdded}
            />
          </div>
        )}

        {error && <ToneBanner tone="err" label="Failed" text={error} />}

        <DialogFooter>
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {activeCloud ? intl.formatMessage(i18n.done) : intl.formatMessage(i18n.cancel)}
          </Button>
          {provider === 'mlx' && (
            <Button
              variant="primary"
              onClick={() => void addMlx()}
              disabled={!mlxReady}
              data-testid="add-node-mlx-submit"
              icon={busy ? <Loader2 className="animate-spin" /> : undefined}
            >
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

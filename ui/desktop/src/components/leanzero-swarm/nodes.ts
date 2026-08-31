import type { SwarmDeviceRow } from '../settings/swarm/golden';

/**
 * Derivations for LeanZero MLX swarm nodes — the shape proven in the swarm E2E and running live
 * on this machine's config.yaml:
 *
 *   swarm.devices[]: { id: 'workhorse-mlx', model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
 *                      weight: 2, enabled: true, instances: 1, engine: 'mlx-sidecar' }
 *   mlx_engine: { model_id: 'mlx-community/Qwen3.5-9B-MLX-4bit',
 *                 served_model_name: 'workhorse-qwen3.5-9b-4bit-mlx' }
 *
 * The engine serves the ALIAS: the device's model_id must equal mlx_engine.served_model_name,
 * so adding an MLX node writes both sides from one derivation (never hand-copied twice).
 */

/** Node labels feed device ids and the served alias: lowercase, dash-separated, nothing exotic. */
export function sanitizeNodeLabel(raw: string): string {
  return raw
    .trim()
    .toLowerCase()
    .replace(/[\s_]+/g, '-')
    .replace(/[^a-z0-9.-]/g, '')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
}

/**
 * Short model tag from an HF repo id: the repo name, lowercased, with any 'mlx' token dropped
 * (the alias re-appends '-mlx' as its engine marker, and 'qwen3.5-9b-mlx-4bit-mlx' is noise).
 * 'mlx-community/Qwen3.5-9B-MLX-4bit' -> 'qwen3.5-9b-4bit'.
 */
export function mlxModelShort(hfModelId: string): string {
  const repo = hfModelId.split('/').pop() ?? hfModelId;
  return repo
    .toLowerCase()
    .split('-')
    .filter((t) => t !== 'mlx' && t !== '')
    .join('-');
}

/** The served alias — what the engine advertises and what the device row must name as model_id. */
export function mlxServedAlias(label: string, hfModelId: string): string {
  return `${sanitizeNodeLabel(label)}-${mlxModelShort(hfModelId)}-mlx`;
}

export function mlxDeviceId(label: string): string {
  return `${sanitizeNodeLabel(label)}-mlx`;
}

/** The full device row an MLX add writes into swarm.devices (the LOCAL machine's node). */
export function mlxDeviceRow(label: string, hfModelId: string, weight: number): SwarmDeviceRow {
  return {
    id: mlxDeviceId(label),
    model_id: mlxServedAlias(label, hfModelId),
    weight,
    enabled: true,
    instances: 1,
    engine: 'mlx-sidecar',
  };
}

/**
 * A REMOTE machine's MLX node: same row shape plus `host` = the machine name. Addable per the
 * spec, but the per-node engine endpoints are a queued backend phase — the Nodes list wears a
 * solid amber "awaiting fleet routing" chip on these rather than rendering them reachable.
 * No mlx_engine settings alignment happens for a remote row (the local engine's alias contract
 * belongs to the local node alone).
 */
export function mlxRemoteDeviceRow(
  machine: string,
  hfModelId: string,
  weight: number
): SwarmDeviceRow {
  return { ...mlxDeviceRow(machine, hfModelId, weight), host: sanitizeNodeLabel(machine) };
}

export interface SwarmMachine {
  machine: string;
  local: boolean;
}

/** True when this machine already has an MLX node in the pool (by id convention or host). */
export function machineHasMlxNode(machine: string, devices: SwarmDeviceRow[]): boolean {
  const m = sanitizeNodeLabel(machine);
  return devices.some(
    (d) => d.engine === 'mlx-sidecar' && (d.id === `${m}-mlx` || (d.host ?? null) === m)
  );
}

/**
 * The machines a LeanZero MLX node can still be added FOR — the amendment's cap: one MLX node per
 * swarm machine, so a 5-machine swarm offers exactly 5 (minus the ones already added). Machines
 * come from `lms ps` (the fleet-machines IPC) unioned with the LM Link model-id prefixes the HTTP
 * fleet reports; the IPC's local flag wins when both name a machine.
 */
export function addableMlxMachines(
  ipcMachines: SwarmMachine[],
  fleetModels: string[],
  devices: SwarmDeviceRow[]
): SwarmMachine[] {
  const byName = new Map<string, boolean>();
  for (const m of ipcMachines) {
    const name = sanitizeNodeLabel(m.machine);
    if (name) byName.set(name, (byName.get(name) ?? false) || m.local);
  }
  for (const model of fleetModels) {
    const bare = model.split('/').pop() ?? model;
    const dash = bare.indexOf('-');
    const name = sanitizeNodeLabel(dash > 0 ? bare.slice(0, dash) : bare);
    if (name && !byName.has(name)) byName.set(name, false);
  }
  return [...byName.entries()]
    .filter(([name]) => !machineHasMlxNode(name, devices))
    .map(([machine, local]) => ({ machine, local }));
}

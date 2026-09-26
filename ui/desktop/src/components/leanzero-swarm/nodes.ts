import type { SwarmDeviceRow } from '../settings/swarm/golden';
import type { NodeState, NodesResponse } from '../../acp/leanzero-link';
import { macName } from './macs';

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

/**
 * A Mac a LeanZero MLX node can be made for. `machine` is the node label (the Mac's name, made
 * label-safe); `name` is the Mac's name as every other surface shows it (`macName`); `names` are the
 * labels an existing row may already carry for this Mac (its name and its hostname).
 */
export interface SwarmMachine {
  machine: string;
  name: string;
  local: boolean;
  names: string[];
}

function swarmMachineOf(node: NodeState, local: boolean): SwarmMachine | null {
  const names = [...new Set([sanitizeNodeLabel(macName(node)), sanitizeNodeLabel(node.hostname)])];
  const labels = names.filter((n) => n !== '');
  if (labels.length === 0) return null;
  return { machine: labels[0], name: macName(node), local, names: labels };
}

/**
 * The Macs the product actually knows, from the LeanZero Link roster: this Mac first (Link answers
 * with this Mac alone when it is not connected), then every linked peer in the roster's order.
 */
export function swarmMachinesFromLink(nodes: NodesResponse): SwarmMachine[] {
  const self = swarmMachineOf(nodes.self, true);
  const peers = nodes.peers.map((p) => swarmMachineOf(p, false));
  return [self, ...peers].filter((m): m is SwarmMachine => m !== null);
}

/**
 * True when this Mac already has its MLX node in the pool. A row with no `host` is served by THIS
 * Mac's engine whatever its label, so any such row takes this Mac's slot; a peer's row carries the
 * peer as `host`.
 */
export function machineHasMlxNode(machine: SwarmMachine, devices: SwarmDeviceRow[]): boolean {
  return devices.some((d) => {
    if (d.engine !== 'mlx-sidecar') return false;
    const host = d.host ?? null;
    if (host === null) return machine.local;
    return machine.names.includes(host);
  });
}

/**
 * The Macs a LeanZero MLX node can still be added FOR — the owner's cap: one MLX node per Mac, so
 * three linked Macs offer exactly three (minus the ones already added).
 */
export function addableMlxMachines(
  machines: SwarmMachine[],
  devices: SwarmDeviceRow[]
): SwarmMachine[] {
  return machines.filter((m) => !machineHasMlxNode(m, devices));
}

export interface NodeConfigAccess {
  read: (key: string, isSecret: boolean, options?: { throwOnError?: boolean }) => Promise<unknown>;
  upsert: (key: string, value: unknown, isSecret: boolean) => Promise<void>;
}

/**
 * The ONE writer of a pool node's model once it was added: the no-node notice's "Chat with …" and
 * the Nodes table's model picker both land here. It is the user's own pick, so it is written as
 * they chose it; the swarm block is read fresh (a concurrent edit is kept), and a node that is no
 * longer in the pool is an error, never a silent add.
 */
export async function setNodeModel(
  config: NodeConfigAccess,
  nodeId: string,
  modelId: string
): Promise<void> {
  const raw = (await config.read('swarm', false, { throwOnError: true })) as {
    devices?: SwarmDeviceRow[];
  } | null;
  const devices = Array.isArray(raw?.devices) ? raw.devices : [];
  if (!devices.some((d) => d.id === nodeId)) {
    throw new Error(`${nodeId} is no longer in the swarm pool — its model was not changed`);
  }
  await config.upsert(
    'swarm',
    {
      ...raw,
      devices: devices.map((d) => (d.id === nodeId ? { ...d, model_id: modelId } : d)),
    },
    false
  );
}

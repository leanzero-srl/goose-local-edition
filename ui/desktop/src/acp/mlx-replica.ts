import { getAcpClient } from './acpConnection';

/**
 * Client surface for copying a model between LeanZero Link devices
 * (`_goose/unstable/mlxEngine/{replicaTargets,replicate,replicaProgress,replicaCancel}`).
 *
 * A model already on one device is copied to a peer over the best DIRECT path the two share:
 * a Thunderbolt cable (both ends' Thunderbolt ports in one subnet) first, else a shared LAN.
 * The sending device offers the model on a listener bound to that one interface and the
 * receiving device pulls it file by file, verifying every file's SHA-256 against the sender.
 * Progress lives on the RECEIVING device — poll it with `nodeId = the target`.
 *
 * Local types (the repo rule: never import generated API types); camelCase wire fields.
 * Its own module so the mlxEngine client's test mocks stay exactly as they were.
 */

export type ReplicaLinkKind = 'thunderbolt' | 'network';

export interface ReplicaInterface {
  device: string;
  /** macOS hardware-port name ("Thunderbolt 3", "Wi-Fi"). */
  hardwarePort?: string;
  kind: 'thunderbolt' | 'ethernet' | 'wifi' | 'other';
  ipv4: string;
  prefixLen: number;
  /** Negotiated Thunderbolt link speed as the OS reports it ("80 Gb/s"). */
  linkSpeed?: string;
}

export interface ReplicaLink {
  kind: ReplicaLinkKind;
  local: ReplicaInterface;
  peer: ReplicaInterface;
}

export interface ReplicaTarget {
  nodeId: string;
  hostname: string;
  /** Present exactly when a copy can go to this device now. */
  link?: ReplicaLink;
  /** Why no copy can go to this device now — shown verbatim. */
  unavailable?: string;
}

export interface ReplicaTargets {
  /** False when this device is not on the mesh: no peers, no copy action anywhere. */
  meshConnected: boolean;
  targets: ReplicaTarget[];
  warning?: string;
}

export type ReplicaState = 'queued' | 'copying' | 'done' | 'failed' | 'cancelled';

export interface ReplicaProgress {
  state: ReplicaState;
  sourceUrl: string;
  link: ReplicaLinkKind;
  linkDetail: string;
  totalBytes: number;
  copiedBytes: number;
  filesTotal: number;
  filesDone: number;
  currentFile?: string;
  /** "verifying" = the file's bytes are in and its SHA-256 is compared with the sender's. */
  phase?: 'transferring' | 'verifying';
  resumedFiles?: string[];
  restartedFiles?: string[];
  skippedFiles?: string[];
  /** Bytes over the link this attempt and the milliseconds receiving them. */
  wireBytes: number;
  wireMillis: number;
  elapsedMillis: number;
  error?: string;
  /** macOS refused the RECEIVING device's app the local network (the engine's named verdict). */
  localNetworkBlocked?: boolean;
  releaseError?: string;
}

export interface ReplicateResult {
  link: ReplicaLink;
  sourceUrl: string;
}

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

function withNode(params: Record<string, unknown>, nodeId?: string): Record<string, unknown> {
  if (nodeId != null) params.nodeId = nodeId;
  return params;
}

/** Which peers `nodeId` (default: this device) could copy a model to, and over which path. */
export async function mlxEngineReplicaTargets(nodeId?: string): Promise<ReplicaTargets> {
  return await call<ReplicaTargets>(
    '_goose/unstable/mlxEngine/replicaTargets',
    withNode({}, nodeId)
  );
}

/** Copy `modelId` from `nodeId` (default: this device) to `targetNodeId`. */
export async function mlxEngineReplicate(
  modelId: string,
  targetNodeId: string,
  nodeId?: string
): Promise<ReplicateResult> {
  return await call<ReplicateResult>(
    '_goose/unstable/mlxEngine/replicate',
    withNode({ modelId, targetNodeId }, nodeId)
  );
}

/** `null` when the receiving device tracks no copy of this model. */
export async function mlxEngineReplicaProgress(
  modelId: string,
  receiverNodeId: string
): Promise<ReplicaProgress | null> {
  const response = await call<{ progress?: ReplicaProgress }>(
    '_goose/unstable/mlxEngine/replicaProgress',
    withNode({ modelId }, receiverNodeId)
  );
  return response.progress ?? null;
}

/** Stop the copy on the receiving device AND delete its partial copy there. */
export async function mlxEngineReplicaCancel(
  modelId: string,
  receiverNodeId: string
): Promise<void> {
  await call('_goose/unstable/mlxEngine/replicaCancel', withNode({ modelId }, receiverNodeId));
}

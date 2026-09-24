import type {
  MlxDistributedCheckDto,
  MlxDistributedConfigDto,
  MlxDistributedDiscoveredModelDto,
  MlxDistributedDiscoveredNodeDto,
  MlxDistributedDiscoveryDto,
  MlxDistributedEventDto,
  MlxDistributedGapDto,
  MlxDistributedPeerCandidateDto,
  MlxDistributedProvisionDto,
  MlxDistributedProvisionNodeDto,
  MlxDistributedNodeConfigDto,
  MlxDistributedNodePreflightDto,
  MlxDistributedNodeStatusDto,
  MlxDistributedPreflightDto,
  MlxDistributedRankPlanDto,
  MlxDistributedStatusDto,
  MlxEngineDistributedStartResponse_unstable,
  MlxEngineDistributedStopResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';
import { toMlxDistributedReport } from '../utils/mlxDistributedReport';

/**
 * Client surface for the DISTRIBUTED MLX engine (`_goose/unstable/mlxEngine/distributed*`): one
 * model split across several Macs, supervised by THIS goosed (this Mac is rank 0), so no call
 * carries a mesh `nodeId`. Types are the generated SDK DTOs.
 *
 * Calls go through the raw `extMethod`, NOT the generated typed methods: those `zod.parse` the
 * response, and zod strips every key the generated schema does not know. The config is read, edited
 * and written back whole, so a field a newer backend added (a diagnostic knob the SDK has not been
 * regenerated for) would be silently DROPPED on the next save. Raw keeps it.
 */

export type MlxDistributedStatus = MlxDistributedStatusDto;
export type MlxDistributedConfig = MlxDistributedConfigDto;
export type MlxDistributedNodeConfig = MlxDistributedNodeConfigDto;
export type MlxDistributedNodeStatus = MlxDistributedNodeStatusDto;
export type MlxDistributedPreflight = MlxDistributedPreflightDto;
export type MlxDistributedNodePreflight = MlxDistributedNodePreflightDto;
export type MlxDistributedRankPlan = MlxDistributedRankPlanDto;
export type MlxDistributedCheck = MlxDistributedCheckDto;
export type MlxDistributedEvent = MlxDistributedEventDto;
export type MlxDistributedStartResponse = MlxEngineDistributedStartResponse_unstable;
export type MlxDistributedStopResponse = MlxEngineDistributedStopResponse_unstable;
export type MlxDistributedDiscovery = MlxDistributedDiscoveryDto;
export type MlxDistributedDiscoveredNode = MlxDistributedDiscoveredNodeDto;
export type MlxDistributedDiscoveredModel = MlxDistributedDiscoveredModelDto;
export type MlxDistributedGap = MlxDistributedGapDto;
export type MlxDistributedPeerCandidate = MlxDistributedPeerCandidateDto;
export type MlxDistributedProvision = MlxDistributedProvisionDto;
export type MlxDistributedProvisionNode = MlxDistributedProvisionNodeDto;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/**
 * Every status this window reads goes to MAIN too, so the menu-bar tray follows the distributed
 * engine on the same fact the view saw (main has no ACP client of its own).
 */
function reportToMain(status: MlxDistributedStatus): void {
  const report = (
    window as unknown as {
      electron?: { mlxDistributedReport?: (r: unknown) => void };
    }
  ).electron?.mlxDistributedReport;
  report?.(toMlxDistributedReport(status));
}

/**
 * The latest distributed status ANY read in this window saw — the Engine tab's poll, the tray
 * reporter's loop, a stop's response — for surfaces that must know which engine owns this Mac
 * without a poll of their own (the composer's readiness strip, the no-node notice). A failed read
 * clears it: a state claim never outlives the read that ended it.
 */
let latestStatus: MlxDistributedStatus | null = null;
const latestListeners = new Set<() => void>();

function publishLatest(status: MlxDistributedStatus | null): void {
  latestStatus = status;
  for (const listener of latestListeners) listener();
}

export function latestMlxDistributedStatus(): MlxDistributedStatus | null {
  return latestStatus;
}

export function subscribeMlxDistributedStatus(listener: () => void): () => void {
  latestListeners.add(listener);
  return () => {
    latestListeners.delete(listener);
  };
}

export async function mlxDistributedStatus(): Promise<MlxDistributedStatus> {
  let response: { status: MlxDistributedStatus };
  try {
    response = await call<{ status: MlxDistributedStatus }>(
      '_goose/unstable/mlxEngine/distributedStatus',
      {}
    );
  } catch (e) {
    publishLatest(null);
    throw e;
  }
  reportToMain(response.status);
  publishLatest(response.status);
  return response.status;
}

/** Dry run: every check on every node, nothing launched. `config` absent = the persisted one. */
export async function mlxDistributedPreflight(
  config: MlxDistributedConfig | null,
  repairLink: boolean
): Promise<MlxDistributedPreflight> {
  const params: Record<string, unknown> = { repairLink };
  if (config) params.config = config;
  const response = await call<{ preflight: MlxDistributedPreflight }>(
    '_goose/unstable/mlxEngine/distributedPreflight',
    params
  );
  return response.preflight;
}

/** Returns once launching began (`started`) or with a named `refusal`; poll status for the outcome. */
export async function mlxDistributedStart(
  config: MlxDistributedConfig | null
): Promise<MlxDistributedStartResponse> {
  return await call<MlxDistributedStartResponse>(
    '_goose/unstable/mlxEngine/distributedStart',
    config ? { config } : {}
  );
}

/** Returns after the verified stop; the report says per pid what was signalled and observed. */
export async function mlxDistributedStop(): Promise<MlxDistributedStopResponse> {
  const response = await call<MlxDistributedStopResponse>(
    '_goose/unstable/mlxEngine/distributedStop',
    {}
  );
  reportToMain(response.status);
  publishLatest(response.status);
  return response;
}

export async function mlxDistributedConfigUpdate(
  config: MlxDistributedConfig
): Promise<MlxDistributedConfig> {
  const response = await call<{ config: MlxDistributedConfig }>(
    '_goose/unstable/mlxEngine/distributedConfigUpdate',
    { config }
  );
  return response.config;
}

/** Every non-wildcard `Host` alias in ~/.ssh/config, each probed with a non-interactive ssh. */
export async function mlxDistributedPeerCandidates(): Promise<MlxDistributedPeerCandidate[]> {
  const response = await call<{ candidates: MlxDistributedPeerCandidate[] }>(
    '_goose/unstable/mlxEngine/distributedPeerCandidates',
    {}
  );
  return response.candidates;
}

/**
 * Probe this Mac and each peer (read-only) and get a filled config back: every value with its
 * evidence, every value not found as a named gap. `modelId` = prefer that model when it is on
 * every node.
 */
export async function mlxDistributedDiscover(
  peers: string[],
  modelId: string | null
): Promise<MlxDistributedDiscovery> {
  const params: Record<string, unknown> = { peers };
  if (modelId) params.modelId = modelId;
  const response = await call<{ discovery: MlxDistributedDiscovery }>(
    '_goose/unstable/mlxEngine/distributedDiscover',
    params
  );
  return response.discovery;
}

/** Build every node's goose-managed Python in the background; progress rides `status.provision`. */
export async function mlxDistributedProvision(
  config: MlxDistributedConfig | null
): Promise<MlxDistributedProvision> {
  const response = await call<{ provision: MlxDistributedProvision }>(
    '_goose/unstable/mlxEngine/distributedProvision',
    config ? { config } : {}
  );
  return response.provision;
}

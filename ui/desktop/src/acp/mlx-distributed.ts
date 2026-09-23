import type {
  MlxDistributedCheckDto,
  MlxDistributedConfigDto,
  MlxDistributedEventDto,
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

export async function mlxDistributedStatus(): Promise<MlxDistributedStatus> {
  const response = await call<{ status: MlxDistributedStatus }>(
    '_goose/unstable/mlxEngine/distributedStatus',
    {}
  );
  reportToMain(response.status);
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

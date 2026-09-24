import type {
  MlxEngineRemoteSingleStartResponse_unstable,
  MlxEngineRemoteSingleStopResponse_unstable,
  MlxRemoteSingleRefusalDto,
  MlxRemoteSingleStatusDto,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';
import { toMlxRemoteReport } from '../utils/mlxRemoteReport';

/**
 * Client surface for REMOTE SINGLE (`_goose/unstable/mlxEngine/remoteSingle*`): the single MLX
 * engine on a LeanZero Link peer, this Mac's chat routed to it through the peer's chat proxy.
 * Raw `extMethod` like the distributed surface, so a field a newer backend adds is never stripped.
 *
 * `state`: `off` · `mounting` · `ready` · `failed`. A start that did not happen carries a named
 * `refusal.code` (`chatServingDisabled`, `remoteManagementDisabled`, `peerMountFailed`,
 * `peerTooOld`, `peerUnreachable`, `unknownPeer`, `linkNotConnected`, `distributedOwnsThisMac`,
 * `remoteSingleActive`) and its message, shown verbatim.
 */

export type MlxRemoteSingleStatus = MlxRemoteSingleStatusDto;
export type MlxRemoteSingleRefusal = MlxRemoteSingleRefusalDto;
export type MlxRemoteSingleStartResponse = MlxEngineRemoteSingleStartResponse_unstable;
export type MlxRemoteSingleStopResponse = MlxEngineRemoteSingleStopResponse_unstable;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/** Every status this window reads goes to MAIN too, so the menu-bar tray says where chat goes. */
function reportToMain(status: MlxRemoteSingleStatus | null): void {
  const report = (
    window as unknown as {
      electron?: { mlxRemoteReport?: (r: unknown) => void };
    }
  ).electron?.mlxRemoteReport;
  report?.(toMlxRemoteReport(status));
}

/**
 * The latest remote-single status any read in this window saw, for surfaces that must know where
 * chat goes without a poll of their own. A failed read clears it.
 */
let latestStatus: MlxRemoteSingleStatus | null = null;
const latestListeners = new Set<() => void>();

function publishLatest(status: MlxRemoteSingleStatus | null): void {
  latestStatus = status;
  reportToMain(status);
  for (const listener of latestListeners) listener();
}

export function latestMlxRemoteSingleStatus(): MlxRemoteSingleStatus | null {
  return latestStatus;
}

export function subscribeMlxRemoteSingleStatus(listener: () => void): () => void {
  latestListeners.add(listener);
  return () => {
    latestListeners.delete(listener);
  };
}

/** True while chat goes to a peer's engine (mounting there, serving, or failed there). */
export function remoteRouteUp(status: MlxRemoteSingleStatus | null): boolean {
  return status != null && status.state !== 'off';
}

export async function mlxRemoteSingleStatus(): Promise<MlxRemoteSingleStatus> {
  let response: { status: MlxRemoteSingleStatus };
  try {
    response = await call<{ status: MlxRemoteSingleStatus }>(
      '_goose/unstable/mlxEngine/remoteSingleStatus',
      {}
    );
  } catch (e) {
    publishLatest(null);
    throw e;
  }
  publishLatest(response.status);
  return response.status;
}

/** Mount `modelId` on Link peer `peer` (a node id) and route this Mac's MLX chat to it. */
export async function mlxRemoteSingleStart(
  peer: string,
  modelId: string
): Promise<MlxRemoteSingleStartResponse> {
  const response = await call<MlxRemoteSingleStartResponse>(
    '_goose/unstable/mlxEngine/remoteSingleStart',
    { peer, modelId }
  );
  publishLatest(response.status);
  return response;
}

/** Drop the route; unless `keepMounted`, unmount the model on the peer too. */
export async function mlxRemoteSingleStop(
  keepMounted = false
): Promise<MlxRemoteSingleStopResponse> {
  const response = await call<MlxRemoteSingleStopResponse>(
    '_goose/unstable/mlxEngine/remoteSingleStop',
    { keepMounted }
  );
  publishLatest(response.status);
  return response;
}

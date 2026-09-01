import { getAcpClient } from './acpConnection';
import { errorMessage } from '../utils/conversionUtils';

/**
 * Client surface for LeanZero Link — passwordless account identity + a goose-owned
 * Tailscale mesh (Phases 1-3: email/OTP login, connect-to-mesh, the live peer view).
 *
 * These are custom `_goose/unstable/leanzeroLink/*` extension methods; they are not part
 * of the generated SDK, so the types live here (local types, per the repo rule to never
 * import generated API types) and calls go through the generic `extMethod` dispatcher —
 * the same wire path the mlxEngine client uses.
 *
 * TWO wire dialects, and they DO NOT mix (this is load-bearing — do not "normalize" it):
 *  - LinkState / AuthState / MeshStatus are the desktop's own camelCase contract
 *    (serde `rename_all = "camelCase"` on the Rust DTOs).
 *  - The `nodes` payload (`self` + `peers`) is the SEPARATE snake_case `/v1/swarm`
 *    `NodeState` shared verbatim with peer nodes and the iOS companion. It is passed
 *    through untouched by goosed and MUST be consumed snake_case here.
 */

// ---------------------------------------------------------------------------
// camelCase desktop contract.
// ---------------------------------------------------------------------------

/** Worker `audienceSync` verdict, returned by `verify`. */
export type AudienceSync = 'synced' | 'skipped' | 'failed';

/** Which auth-worker capabilities the deployment has configured (from env presence). */
export interface LinkCapabilities {
  mail: boolean;
  audience: boolean;
  mesh: boolean;
}

export interface LinkHealth {
  ok: boolean;
  version: string;
  capabilities: LinkCapabilities;
}

/**
 * The auth lifecycle, internally tagged on `state`. A `codeSent` carries the absolute
 * `expiresAt` (RFC3339) — the durable countdown target; a `connected` carries the mesh IP.
 */
export type AuthState =
  | { state: 'loggedOut' }
  | { state: 'codeSent'; email: string; expiresAt: string }
  | { state: 'loggedIn'; email: string }
  | { state: 'connecting'; email: string }
  | { state: 'connected'; email: string; meshIp: string };

/** One raw Tailscale peer the local mesh daemon sees (status-panel diagnostics). */
export interface MeshPeer {
  hostname: string;
  ip?: string;
  online: boolean;
  lastSeen?: string;
}

/** Live mesh status from the goose-owned tailscaled. */
export interface MeshStatus {
  selfIp?: string;
  selfHostname?: string;
  backendState: string;
  online: boolean;
  peers: MeshPeer[];
}

/** The composed auth + live mesh state goosed surfaces for the Link tab. */
export interface LinkState {
  auth: AuthState;
  mesh?: MeshStatus;
  nodeCount: number;
  lastError?: string;
}

export interface RequestCodeResult {
  /** The worker-normalized email the code was sent to. */
  email: string;
  expiresInSeconds: number;
}

export interface VerifyResult {
  /** The auth state after a successful verify (`"loggedIn"`). */
  state: string;
  email: string;
  audienceSync: AudienceSync;
}

// ---------------------------------------------------------------------------
// snake_case swarm contract (shared with peer nodes + the iOS companion).
// ---------------------------------------------------------------------------

/** Wire: `{"type":"Idle"}`, `{"type":"Busy","session_id":"…"}`, `{"type":"Offline"}`. */
export type NodeStatus =
  | { type: 'Idle' }
  | { type: 'Busy'; session_id?: string }
  | { type: 'Offline' };

/** The `/v1/swarm` `NodeState` — snake_case, consumed verbatim. */
export interface NodeState {
  node_id: string;
  hostname: string;
  mesh_ip?: string;
  status: NodeStatus;
  sessions_active: number;
  updated_at: string;
}

/** Body of `leanzeroLink/nodes`: the local node (`self`) plus its reachable peers. */
export interface NodesResponse {
  self: NodeState;
  peers: NodeState[];
}

// ---------------------------------------------------------------------------
// Dispatch.
// ---------------------------------------------------------------------------

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

export async function leanzeroLinkHealth(): Promise<LinkHealth> {
  return await call<LinkHealth>('_goose/unstable/leanzeroLink/health', {});
}

/**
 * Request an email OTP. On success the manager moves to `codeSent`; a subsequent
 * `status()` reflects it. Throws with the worker's message on rate-limit / mail-not-
 * configured, or a transport error when the worker is unreachable — see `linkErrorText`.
 */
export async function leanzeroLinkRequestCode(email: string): Promise<RequestCodeResult> {
  return await call<RequestCodeResult>('_goose/unstable/leanzeroLink/requestCode', { email });
}

export async function leanzeroLinkVerify(email: string, code: string): Promise<VerifyResult> {
  return await call<VerifyResult>('_goose/unstable/leanzeroLink/verify', { email, code });
}

/** Bring up the mesh + control service; returns the state after the attempt. */
export async function leanzeroLinkConnect(): Promise<LinkState> {
  return await call<LinkState>('_goose/unstable/leanzeroLink/connect', {});
}

export async function leanzeroLinkStatus(): Promise<LinkState> {
  return await call<LinkState>('_goose/unstable/leanzeroLink/status', {});
}

/** Tear down + clear identity. `wipe` also removes the mesh state dir (slower re-login). */
export async function leanzeroLinkLogout(wipe: boolean): Promise<LinkState> {
  return await call<LinkState>('_goose/unstable/leanzeroLink/logout', { wipe });
}

export async function leanzeroLinkNodes(): Promise<NodesResponse> {
  return await call<NodesResponse>('_goose/unstable/leanzeroLink/nodes', {});
}

// ---------------------------------------------------------------------------
// Error surfacing.
// ---------------------------------------------------------------------------

/**
 * A LinkError reaches the UI as a JSON-RPC `invalid_params` whose `data` carries the
 * backend's message string VERBATIM (rate-limit retry wording, invalid-code, mail-not-
 * configured, …). The SDK's `RequestError.message` is the generic "Invalid params", so
 * the real sentence is in `.data` — prefer it, fall back to the message.
 */
export function linkErrorText(error: unknown): string {
  const data = (error as { data?: unknown } | null | undefined)?.data;
  if (typeof data === 'string' && data.trim() !== '') {
    return data;
  }
  return errorMessage(error, 'The LeanZero Link request failed.');
}

/**
 * True when the backend text is a WorkerError transport/build failure — the worker could
 * not be reached at all (the honest state on a machine with no auth worker deployed). The
 * UI renders a plain "couldn't reach the service" line for these rather than the raw URL.
 */
export function isLinkServiceUnreachable(text: string): boolean {
  return /failed to send:/.test(text) || /cannot build the worker HTTP client/.test(text);
}

/** The unreachable case as an honest, human line (no raw URL/stack). */
export const LINK_SERVICE_UNREACHABLE_MESSAGE =
  "Couldn't reach the LeanZero Link service — the auth worker may not be deployed yet.";

/** Map a thrown Link error to the exact banner text: honest line for transport, else verbatim. */
export function linkBannerText(error: unknown): string {
  const text = linkErrorText(error);
  return isLinkServiceUnreachable(text) ? LINK_SERVICE_UNREACHABLE_MESSAGE : text;
}

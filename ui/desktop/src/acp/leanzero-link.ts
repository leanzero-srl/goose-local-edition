import { getAcpClient } from './acpConnection';
import { errorMessage } from '../utils/conversionUtils';
import { toLinkTrayReport } from '../utils/linkTrayReport';

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

/**
 * The user's persisted mesh intent: `connected` = goosed brings the mesh back at every launch
 * with no click; `disconnected` = it stays off until the user connects. Written only by the
 * user's Connect / Disconnect / Log out (`migrated` = derived once from an install that predates
 * the record; `noRecord` = never connected here, not persisted).
 */
export interface LinkIntent {
  intent: 'connected' | 'disconnected';
  cause: 'userConnect' | 'userDisconnect' | 'userLogout' | 'migrated' | 'noRecord';
  updatedAt: string;
}

/**
 * What goosed did about a `connected` intent at launch, with no user action. `failed` is the
 * loud one — the mesh should be up and is not; `reason` is shown verbatim and Retry = connect.
 */
export type ReconnectState =
  | { state: 'idle' }
  | { state: 'skipped'; reason: string }
  | { state: 'reconnecting'; startedAt: string }
  | { state: 'reconnected'; at: string; meshIp: string }
  | { state: 'failed'; reason: string; at: string };

/**
 * The road Link last took to one of its servers. A `*.ts.net` server is reached at its tailnet
 * address (`tailnet`, `ip`) when this Mac is on that node's tailnet, else by public DNS — which for a
 * `*.ts.net` name is Tailscale Funnel (`public`, `reason` says why the tailnet was not used).
 */
export interface LinkRoute {
  host: string;
  path: 'tailnet' | 'public';
  ip?: string;
  reason?: string;
  decidedAt: string;
  /** The last request on this road failed with this; absent once one succeeds. */
  lastFailure?: string;
}

/** How Link last reached its auth worker and its mesh's control plane; absent = no request yet. */
export interface LinkRoutes {
  worker?: LinkRoute;
  control?: LinkRoute;
}

/** The composed auth + live mesh state goosed surfaces for the Link tab. */
export interface LinkState {
  auth: AuthState;
  mesh?: MeshStatus;
  nodeCount: number;
  lastError?: string;
  /** Absent only when the intent record is unreadable (`intentError` says why) or goosed predates it. */
  intent?: LinkIntent;
  intentError?: string;
  /** Absent only from a goosed that predates the launch reconnect. */
  reconnect?: ReconnectState;
  /** Absent only from a goosed that predates the route report (Q-137). */
  routes?: LinkRoutes;
  /** "Load and download models" as configured now (`LEANZERO_LINK_ALLOW_REMOTE_EXECUTION`). */
  remoteExecutionAllowed?: boolean;
  /** What the RUNNING control service enforces (connected only); differs until a reconnect. */
  remoteExecutionAllowedLive?: boolean;
  /** "Answer chat" (`LEANZERO_LINK_ALLOW_CHAT_SERVING`), read by goose on every request. */
  chatServingAllowed?: boolean;
  /** "Run part of a split model" (`LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE`), read on every request. */
  distributedNodeAllowed?: boolean;
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

/** The owner's three switches, as a node reports them about itself (snake_case, verbatim). */
export interface NodeAllows {
  /** Load, download, copy and delete models there (`LEANZERO_LINK_ALLOW_REMOTE_EXECUTION`). */
  manage_models: boolean;
  /** Its single engine answers another Mac's chat (`LEANZERO_LINK_ALLOW_CHAT_SERVING`). */
  answer_chat: boolean;
  /** It runs part of another Mac's split model (`LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE`). */
  run_split: boolean;
  /** It takes mesh jobs (`LEANZERO_LINK_ALLOW_REMOTE_JOBS`); absent from a goose that predates jobs. */
  run_jobs?: boolean;
  /** Another Mac's Free up memory may quit apps there (`GOOSE_LINK_ALLOW_REMOTE_APP_QUIT`, default off). */
  quit_apps?: boolean;
}

/** What the Mac's owner is doing with it and what the hardware reads (leanzero-link `OwnLoad`). */
export interface OwnLoad {
  user_idle_s?: number | null;
  user_active: boolean;
  /** 1-minute load average divided by the core count. */
  cpu_load_per_core?: number | null;
  ncpu: number;
  /** Available memory (free + file cache + purgeable), percent of total. */
  mem_free_pct?: number | null;
  mem_total_gb?: number | null;
  gpu_util_pct?: number | null;
  on_ac?: boolean | null;
  /** macOS thermal pressure, 0 nominal .. 4. */
  thermal_limit?: number | null;
  lid_closed_sleeps: boolean;
  /** True when EVERY heavy slot is held (3.0.131; before, the one slot). */
  heavy_slot_held: boolean;
  /** Heavy slots held / this Mac's slot count (0 max = an older goose that reports the flag only). */
  heavy_slots_used?: number;
  heavy_slots_max?: number;
  /** How many more default-size (8 GB) jobs fit above the memory floor. */
  can_take?: number | null;
  /** The measured verdict with its numbers: "ok: 1/3 heavy, 120 GB free, load 0.28/core, can take ~11 more 8 GB jobs" or "busy: ...". */
  verdict?: string | null;
  /** Why a benchmark counts as running there; absent = none. */
  benchmark?: string;
}

/** AI inference the Mac serves now (leanzero-link `InferenceLoad`). */
export interface InferenceLoad {
  /** Engine requests running + waiting, plus relayed chat streams. */
  engine_active: number;
  /** goose swarm-router slots this Mac's goosed holds. */
  router_slots_used: number;
}

export type JobKind = 'shell' | 'prompt';
/**
 * `cancelling`: a cancel was sent and the job's process group has not exited yet.
 * `lost`: the ledger said running but its process group was gone when the registry reloaded.
 */
export type JobState =
  | 'queued'
  | 'running'
  | 'cancelling'
  | 'done'
  | 'failed'
  | 'killed'
  | 'cancelled'
  | 'lost';

/** A job as peers see it: never the command (leanzero-link `JobBrief`). */
export interface JobBrief {
  job_id: string;
  origin_node: string;
  kind: JobKind;
  started_at: string;
  state: JobState;
  heavy?: boolean;
  /** Set while the job waits in its ORIGIN node's queue: no Mac had room for it yet. */
  queue?: JobQueueInfo;
}

/** How a job without a named target is placed (leanzero-link `PlacementStrategy`). */
export type PlacementStrategy = 'local-first' | 'selfish';

/** One Mac's answer at the last placement try (leanzero-link `NodeRefusal`, snake_case). */
export interface JobNodeRefusal {
  node_id: string;
  code: string;
  reason: string;
}

/** A queued job's place in line (1 = next) and why each Mac said no (leanzero-link `QueueInfo`). */
export interface JobQueueInfo {
  position: number;
  strategy: PlacementStrategy;
  refusals: JobNodeRefusal[];
}

/** `NodeState.load`, on the 3 s poll: own use, inference, jobs. */
export interface NodeLoad {
  own: OwnLoad;
  inference: InferenceLoad;
  jobs: JobBrief[];
  sampled_at?: string;
}

/** `GET /v1/swarm/jobs/{id}`: the brief plus how it ended (snake_case, verbatim). */
export interface JobView extends JobBrief {
  ended_at?: string | null;
  exit_code?: number | null;
  reason?: string | null;
  session_id?: string | null;
  log_bytes: number;
  /** A queued job that has since been placed: the Mac it runs on and its id there. */
  placed_on?: { node_id: string; job_id: string } | null;
}

export interface JobLogChunk {
  from: number;
  next: number;
  data: string;
  /** The job has ended and `next` is the end of the log. */
  done: boolean;
}

/** The `/v1/swarm` `NodeState` — snake_case, consumed verbatim. */
export interface NodeState {
  node_id: string;
  hostname: string;
  mesh_ip?: string;
  status: NodeStatus;
  sessions_active: number;
  updated_at: string;
  /** Why this Mac's last poll of the peer failed; absent after a clean poll and on `self`. */
  last_poll_error?: string;
  /** The name its owner gave it (macOS ComputerName); absent from a goose that predates it. */
  computer_name?: string;
  /** What it lets the other Macs do there, as it enforces it now; absent = unknown (older goose). */
  allows?: NodeAllows;
  /** Own use, AI inference and jobs, sampled on every poll; absent from a goose that predates it. */
  load?: NodeLoad | null;
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

/**
 * Every Link state this window reads goes to MAIN too (main owns no ACP client), so the menu-bar
 * tray says whether the mesh is up — and says it loudly when a launch reconnect failed.
 */
/**
 * Window event fired after a Connect or Disconnect returns, from whichever screen made it (the Link
 * tab, the tray). `reportToMain` already carries the Link line; the tray's per-Mac lines are read by
 * `useLinkTrayReporter`, which otherwise would not look again for up to 30 s.
 */
export const LINK_CHANGED_EVENT = 'leanzero-link-changed';

function announceLinkChanged(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(LINK_CHANGED_EVENT));
}

function reportToMain(state: LinkState): LinkState {
  const report = (
    window as unknown as {
      electron?: { linkReport?: (r: unknown) => void };
    }
  ).electron?.linkReport;
  report?.(toLinkTrayReport(state));
  return state;
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

/**
 * Bring up the mesh + control service; returns the state after the attempt. Records the intent
 * `connected`, so every later launch reconnects on its own.
 */
export async function leanzeroLinkConnect(): Promise<LinkState> {
  const state = reportToMain(await call<LinkState>('_goose/unstable/leanzeroLink/connect', {}));
  announceLinkChanged();
  return state;
}

export async function leanzeroLinkStatus(): Promise<LinkState> {
  return reportToMain(await call<LinkState>('_goose/unstable/leanzeroLink/status', {}));
}

/**
 * Take this Mac off the mesh and keep it off across launches; the account stays signed in
 * (`loggedIn`). Connect is the way back.
 */
export async function leanzeroLinkDisconnect(): Promise<LinkState> {
  const state = reportToMain(await call<LinkState>('_goose/unstable/leanzeroLink/disconnect', {}));
  announceLinkChanged();
  return state;
}

/** Tear down + clear identity. `wipe` also removes the mesh state dir (slower re-login). */
export async function leanzeroLinkLogout(wipe: boolean): Promise<LinkState> {
  return reportToMain(await call<LinkState>('_goose/unstable/leanzeroLink/logout', { wipe }));
}

export async function leanzeroLinkNodes(): Promise<NodesResponse> {
  return await call<NodesResponse>('_goose/unstable/leanzeroLink/nodes', {});
}

/** Args for `remoteExecute` — camelCase, matching the Rust `LeanzeroLinkRemoteExecuteRequest`. */
export interface RemoteExecuteArgs {
  /** A node id from `nodes()` (self or an Idle peer). */
  targetNodeId: string;
  prompt: string;
  /** Absolute working dir on the TARGET; omitted → the device's home. */
  workingDir?: string;
}

/** `{ sessionId }` — the session created on the TARGET node, mirrored over the swarm stream. */
export interface RemoteExecuteResult {
  sessionId: string;
}

/**
 * Start a NEW session on an idle linked node (self or a peer) with a fresh prompt.
 * The backend requires the manager be `connected` and the target be idle; a busy /
 * disabled / unwired / unknown / not-connected target surfaces its status text VERBATIM
 * as an `invalid_params` error — render it via `linkBannerText`. `workingDir` is omitted
 * from the wire when blank (the backend defaults to the device's home).
 */
export async function leanzeroLinkRemoteExecute(
  args: RemoteExecuteArgs
): Promise<RemoteExecuteResult> {
  const params: Record<string, unknown> = {
    targetNodeId: args.targetNodeId,
    prompt: args.prompt,
  };
  const dir = args.workingDir?.trim();
  if (dir) params.workingDir = dir;
  return await call<RemoteExecuteResult>('_goose/unstable/leanzeroLink/remoteExecute', params);
}

// ---------------------------------------------------------------------------
// Mesh jobs (`/v1/swarm/jobs` on the target, proxied by goosed).
// ---------------------------------------------------------------------------

export async function leanzeroLinkJobsList(targetNodeId: string): Promise<JobView[]> {
  const r = await call<{ jobs: JobView[] }>('_goose/unstable/leanzeroLink/jobsList', {
    targetNodeId,
  });
  return r.jobs;
}

export async function leanzeroLinkJobsGet(targetNodeId: string, jobId: string): Promise<JobView> {
  const r = await call<{ job: JobView }>('_goose/unstable/leanzeroLink/jobsGet', {
    targetNodeId,
    jobId,
  });
  return r.job;
}

/** Cancel kills the job's whole process group there; a prompt job answers `cancel_unsupported`. */
export async function leanzeroLinkJobsCancel(
  targetNodeId: string,
  jobId: string
): Promise<JobView> {
  const r = await call<{ job: JobView }>('_goose/unstable/leanzeroLink/jobsCancel', {
    targetNodeId,
    jobId,
  });
  return r.job;
}

/** The log from byte `from`; read again from `next` until `done`. */
export async function leanzeroLinkJobsLog(
  targetNodeId: string,
  jobId: string,
  from: number
): Promise<JobLogChunk> {
  const r = await call<{ log: JobLogChunk }>('_goose/unstable/leanzeroLink/jobsLog', {
    targetNodeId,
    jobId,
    from,
  });
  return r.log;
}

/** A job to send (leanzero-link `JobRequest`; snake_case inside `job`, as the node's route takes it). */
export interface JobRequest {
  kind: JobKind;
  command: string;
  cwd?: string;
  need_mem_gb?: number;
  heavy: boolean;
  timeout_s?: number;
}

/**
 * Where the job went (the placement's pick when no target was given). `queued`: no Mac had room,
 * so it waits in THIS node's queue at `queuePosition` (1 = next) and `nodeId`/`jobId` are this node
 * and the queue id, which keeps answering get/log/cancel after the job is placed.
 */
export interface JobSubmitResult {
  nodeId: string;
  jobId: string;
  queued?: boolean;
  queuePosition?: number | null;
}

/**
 * Send one job. `targetNodeId` omitted = goosed places it by `strategy` (omitted = the setting),
 * queueing it when no Mac has room. A refusal from a NAMED target (jobs off there, no memory
 * headroom, heavy slot held, …) throws with `data.reason`.
 */
export async function leanzeroLinkJobsSubmit(
  job: JobRequest,
  targetNodeId?: string,
  strategy?: PlacementStrategy
): Promise<JobSubmitResult> {
  const params: Record<string, unknown> = { job };
  if (targetNodeId) params.targetNodeId = targetNodeId;
  if (strategy) params.strategy = strategy;
  return await call<JobSubmitResult>('_goose/unstable/leanzeroLink/jobsSubmit', params);
}

/** One candidate Mac for a job, best first (leanzero-link placement). */
export interface JobPlacement {
  nodeId: string;
  userActive: boolean;
  score: number;
  /** More jobs of this size that fit there (3.0.131; absent from older goose). */
  canTake?: number | null;
  /** The verdict line with its numbers. */
  verdict?: string;
}

/** One Mac's refusal at placement (camelCase, as `jobsPlace` answers). */
export interface JobPlaceRefusal {
  nodeId: string;
  code: string;
  reason: string;
}

/** `jobsPlace`: the order the strategy gives, the strategy used, and why each other Mac said no. */
export interface JobPlan {
  placements: JobPlacement[];
  strategy: PlacementStrategy;
  refusals: JobPlaceRefusal[];
}

/**
 * Where a job could run now, best first by `strategy` (omitted = the setting); busy Macs and Macs
 * that refuse jobs are absent from `placements` and listed in `refusals` with their reason.
 */
export async function leanzeroLinkJobsPlace(args: {
  needMemGb?: number;
  heavy: boolean;
  strategy?: PlacementStrategy;
}): Promise<JobPlan> {
  const params: Record<string, unknown> = { heavy: args.heavy };
  if (args.needMemGb != null) params.needMemGb = args.needMemGb;
  if (args.strategy) params.strategy = args.strategy;
  const r = await call<Partial<JobPlan>>('_goose/unstable/leanzeroLink/jobsPlace', params);
  return {
    placements: r.placements ?? [],
    strategy: r.strategy === 'selfish' ? 'selfish' : 'local-first',
    refusals: r.refusals ?? [],
  };
}

/**
 * A job refusal arrives as `invalid_params` whose `data` is `{code, reason}` (leanzero-link
 * `Refusal`); its reason is the sentence to show. Anything else falls back to `linkBannerText`.
 */
export function jobErrorText(error: unknown): string {
  const data = (error as { data?: unknown } | null | undefined)?.data;
  if (
    data &&
    typeof data === 'object' &&
    typeof (data as { reason?: unknown }).reason === 'string'
  ) {
    return (data as { reason: string }).reason;
  }
  return linkBannerText(error);
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

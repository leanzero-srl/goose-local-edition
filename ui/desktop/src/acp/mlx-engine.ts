import type {
  MlxDistributedHostedRankDto,
  MlxMountFitDto,
  MlxPlacementBadgeDto,
  MlxPlacementCandidateDto,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for the in-house supervised MLX engine (Rapid-MLX sidecar).
 *
 * These are custom `_goose/unstable/mlxEngine/*` extension methods; they are not part of the
 * generated SDK yet, so the types live here (local types, per the repo rule to never import
 * generated API types) and calls go through the generic `extMethod` dispatcher — the same wire
 * path the generated client uses for every other `_goose/unstable/*` method.
 * Wire fields are camelCase (serde `rename_all = "camelCase"` on the Rust DTOs).
 */

export type MlxEngineState = 'stopped' | 'mounting' | 'running' | 'failed';

export interface MlxEngineStatus {
  state: MlxEngineState;
  modelId?: string;
  /**
   * The id the live engine serves on its API — differs from `modelId` (the HF directory)
   * when a served-model alias is configured. Chat requests MUST use this id.
   */
  servedModelId?: string;
  /**
   * Requests the engine has accepted and not finished (Rapid-MLX `/v1/status` num_running +
   * num_waiting), read by the sidecar on the same probe as `servedModelId`. Absent when the engine
   * did not report it — never a fabricated 0; `activeRequestsError` says why. `> 0` is the node's
   * BUSY fact for the fleet corroboration; an explicit 0 is idle.
   */
  activeRequests?: number;
  activeRequestsError?: string;
  baseUrl?: string;
  pid?: number;
  contextWindow?: number;
  toolCallParser?: string;
  /** A failed `/v1/models` probe reports here — contextWindow/toolCallParser are never fabricated. */
  probeError?: string;
  /** A mount gate refusal (e.g. not enough memory). Render VERBATIM, never paraphrased. */
  gateMessage?: string;
  gateVerdict?: 'allow' | 'warn' | 'block';
  /** Persisted settings would spawn the running engine differently; remount to apply. */
  restartRequired: boolean;
  /**
   * Set while the manager is NOT running yet something already listens on the configured
   * port — an unsupervised engine orphaned by a previous session. Unmount reclaims it.
   * Optional defensively: older agents do not send it.
   */
  strayListenerPort?: number;
  /**
   * Memory a mount can take: free pages plus the file cache the OS reclaims on demand (on macOS,
   * Activity Monitor's physical minus used). 0 exactly when `memoryError` is set.
   */
  availableMemoryGb: number;
  totalMemoryGb: number;
  /** The part of `availableMemoryGb` that is reclaimable file cache; absent where the OS does not split it out (Linux). */
  reclaimableCacheGb?: number;
  /** The OS memory probe failed; the memory figures are 0 and must not be read as a measurement. */
  memoryError?: string;
  /** This Mac's chip, probed once per goose; absent exactly when `chipError` says why. */
  chip?: { hwModel: string; brand: string; gpuCores?: number | null } | null;
  chipError?: string | null;
  lastError?: string;
  /**
   * While mounting: the sidecar's measure of the start — `phase` makingRoom | starting | loading |
   * warming, the engine process's resident bytes against the model's bytes on disk.
   */
  load?: { phase: string; residentBytes?: number | null; weightsBytes: number } | null;
  /**
   * The sidecar's one fit rule for the status request's `fitModelId` on this Mac now — what a
   * Mount would be judged on; absent exactly when `mountFitError` says why (or none was asked).
   */
  mountFit?: MlxMountFitDto | null;
  mountFitError?: string | null;
  /** Set while THIS Mac holds part of another Mac's split model (the single engine is refused). */
  hosting?: MlxDistributedHostedRankDto | null;
}

/**
 * Per-model sampling/context profile. Sampling is PER MODEL: the engine spawns each
 * mounted model with the flags from ITS entry in `MlxEngineSettings.modelProfiles`.
 * An absent key means "engine default"; an explicit 0 is a real value — keep them apart.
 */
export interface MlxModelProfile {
  temperature?: number;
  topP?: number;
  topK?: number;
  minP?: number;
  repetitionPenalty?: number;
  presencePenalty?: number;
  frequencyPenalty?: number;
  contextLimit?: number;
  /**
   * Speculative decoding: 'mtp' demands the MTP head (skipped with a warning when the model
   * dir has no mtp.safetensors), 'off' refuses it; absent = auto (on when the head exists).
   */
  speculative?: 'mtp' | 'off';
  /**
   * Directory of an mlx-lm LoRA/DoRA adapter fused at load (`--adapter-path`); `~` expands.
   * The mount fails, naming the missing file, when it is not one.
   */
  adapterPath?: string;
  /**
   * false lets a vision-bearing checkpoint take the engine's MLLM lane; absent/true pins the
   * text lane (`--text-only`). No effect on a checkpoint that declares no vision.
   */
  textOnly?: boolean;
  /**
   * Sent as `chat_template_kwargs.enable_thinking` on every turn a session routes to this model.
   * Absent = auto: nothing is sent and the engine decides (off whenever the request carries
   * tools). Captured once per session.
   */
  thinking?: 'on' | 'off';
  /**
   * One of the model's `thinking.effortLevels`, sent as `chat_template_kwargs.reasoning_effort`.
   * Absent = the template's own default. Captured once per session.
   */
  reasoningEffort?: string;
  /**
   * Compressed live KV cache (`--kv-cache-dtype`). Absent = off: the engine's bf16 cache, no
   * flag. A change restarts the engine; the mount is refused, with the reason, when the model's
   * KV cannot take it.
   */
  kvCache?: MlxKvCacheMode;
}

export type MlxKvCacheMode = 'int8' | 'int4';

/**
 * KV bytes one token of context costs at each cache setting, from the model's config.json and
 * the engine's packed layout. Only full-attention layers grow with the context.
 */
export interface MlxKvCacheFacts {
  attentionLayers: number;
  stateLayers: number;
  slidingLayers: number;
  kvHeads: number;
  headDim: number;
  /** Absent = no quantization group fits head_dim: the KV cannot be compressed. */
  groupSize?: number | null;
  bf16BytesPerToken: number;
  int8BytesPerToken?: number | null;
  int4BytesPerToken?: number | null;
}

/** One setting's greedy quality against the bf16 cache on the same prompts. */
export interface MlxKvModeMeasurement {
  /** Tokens before the first divergence from bf16, over bf16's tokens (0..1). */
  agreement: number;
  identicalAnswers: number;
  retrievalFound: boolean;
  /** Median decode tok/s with `decodeContextTokens` of context cached (MTP on), over bf16's. */
  decodeTpsRatio?: number | null;
  decodeContextTokens?: number | null;
}

/** The model folder's goose-kv-cache.json (evals/mlx-engine-bench/kv_quant_compare.py). */
export interface MlxKvCacheMeasurement {
  measuredAt: string;
  engine: string;
  prompts: number;
  /** bf16 against itself — the floor every setting is read against. */
  noiseFloor: MlxKvModeMeasurement;
  int8?: MlxKvModeMeasurement | null;
  int4?: MlxKvModeMeasurement | null;
  source: string;
}

export interface MlxEngineSettings {
  modelId?: string;
  modelsDir: string;
  port: number;
  /**
   * LEGACY flat sampling/context fields. READ-ONLY compatibility: the backend still sends
   * them until its one-time migration into `modelProfiles` has run, and it MIGRATES any it
   * receives — so writing them back would clobber profile edits. Never include them in an
   * update payload; `sanitizeSettingsForWrite` in MlxEngineView strips them.
   */
  contextLimit?: number;
  temperature?: number;
  topP?: number;
  topK?: number;
  minP?: number;
  repetitionPenalty?: number;
  presencePenalty?: number;
  frequencyPenalty?: number;
  /** Swarm-facing model id advertised by the engine (`--served-model-name`). */
  servedModelName?: string;
  spawnCommand: string[];
  /** Per-model sampling/context profiles keyed by HF model id — the source of truth. */
  modelProfiles: Record<string, MlxModelProfile>;
}

/**
 * What the model's own chat template lets a request steer about reasoning, proven on the
 * template's Jinja AST with the engine's detection rules.
 */
export interface MlxThinkingCapabilities {
  /** `enable_thinking` or `reasoning`; absent = the template has no on/off switch. */
  thinkingSwitch?: string | null;
  /** The template's own effort vocabulary in template order; empty = none declared. */
  effortLevels: string[];
  /** The level the template renders when a request names none; absent when not provable. */
  defaultEffort?: string | null;
  preserveThinking: boolean;
  budgetForcible: boolean;
}

export interface MlxLocalModel {
  id: string;
  sizeBytes: number;
  complete: boolean;
  /**
   * Files provably missing or unfinished — shards the model's safetensors index names
   * that are absent/empty, plus `.part` leftovers. 0 when complete.
   */
  missingFiles: number;
  /** Absent exactly when `thinkingError` says why. */
  thinking?: MlxThinkingCapabilities | null;
  thinkingError?: string | null;
  /** Absent exactly when `kvCacheError` says why. */
  kvCache?: MlxKvCacheFacts | null;
  kvCacheError?: string | null;
  /** Absent with no error = never measured on this model. */
  kvCacheMeasurement?: MlxKvCacheMeasurement | null;
  kvCacheMeasurementError?: string | null;
}

export interface MlxModelsList {
  models: MlxLocalModel[];
  /** Free bytes an unprivileged writer can use on the models dir's volume. */
  diskAvailableBytes: number;
  diskTotalBytes: number;
}

export interface MlxHfModelHit {
  id: string;
  downloads: number;
  likes: number;
  updatedAt: string;
}

export type MlxBrowseSort = 'downloads' | 'newest';

export interface MlxBrowseParams {
  query?: string;
  author?: string;
  /** Normalized bit-width tag ('4-bit', '8-bit', …) — matches HF tags server-side. */
  quant?: string;
  /** Architecture tag ('qwen3_5', 'llama', …) — matches HF tags server-side. */
  arch?: string;
  sort: MlxBrowseSort;
  /** A previous page's `nextCursor`; every other parameter is already baked into it. */
  cursor?: string;
  /** Page size, default 20, capped at 50 by the backend. */
  limit?: number;
}

/**
 * One MLX browse hit. `quant`/`arch` are DERIVED display fields (tags first, name
 * patterns as fallback) — they describe the hit, they are not proof the server filtered
 * on them unless the request set the corresponding filter.
 */
export interface MlxBrowseHit {
  id: string;
  /** The publisher prefix of `id`. */
  author: string;
  downloads: number;
  likes: number;
  createdAt?: string;
  lastModified?: string;
  tags: string[];
  quant?: string;
  arch?: string;
  /**
   * Exact repository download bytes. Wire name retained for older clients.
   */
  sizeBytesEstimate?: number;
}

/**
 * Filter vocabularies for the browse UI, aggregated live from a bounded HuggingFace crawl
 * and cached backend-side (~1h TTL). Every value is server-side filterable; free text
 * beyond them still passes to the browse filters. Frequency-ordered.
 */
export interface MlxBrowseFilters {
  quants: string[];
  archs: string[];
  authors: string[];
  /** Distinct repos the vocabulary was aggregated from — a top-N sample, not a census. */
  sampledRepos: number;
  /** Unix epoch seconds when the crawl ran. */
  computedAt: number;
  /** Present when the vocabulary is served stale because a TTL refresh failed. */
  refreshError?: string;
}

export interface MlxRepoFile {
  path: string;
  sizeBytes: number;
}

/**
 * Everything the fullscreen model-card modal needs for one repo. A repo without a README
 * yields no `readmeMarkdown` — an absent field, not an error.
 */
export interface MlxModelCard {
  readmeMarkdown?: string;
  /** True when the README exceeded the backend's cap and was cut. */
  readmeTruncated: boolean;
  files: MlxRepoFile[];
  /** Exact sum of every file size the repo tree lists. */
  totalBytes: number;
  tags: string[];
  downloads: number;
  likes: number;
  license?: string;
  createdAt?: string;
  lastModified?: string;
}

export interface MlxBrowsePage {
  hits: MlxBrowseHit[];
  /** Opaque continuation for the next page; absent on the last page. */
  nextCursor?: string;
}

export type MlxDownloadState =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'done'
  | 'failed'
  | 'cancelled';

/**
 * Snapshot download progress. A "cancelled" download has no on-disk claim any more —
 * the backend deleted its partial repo dir — so a cancelled row may simply be dropped.
 */
export interface MlxDownloadProgress {
  state: MlxDownloadState;
  totalBytes: number;
  downloadedBytes: number;
  currentFile?: string;
  /**
   * Files this attempt restarted from zero because their on-disk `.part` or the server's
   * range answer disagreed with the repo tree's size. Absent on the wire when empty.
   */
  restartedFiles?: string[];
  error?: string;
}

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/**
 * Target a device. EVERY mlxEngine method takes an optional `nodeId`: absent (or the local
 * node's id) runs on THIS machine, byte-identical to a call that never carried the field;
 * a peer's node id makes goosed forward the whole op over the mesh and return ITS result.
 * The field is OMITTED from the wire when undefined — an absent field, never `nodeId: null` —
 * so the local path is preserved exactly. Node ids come from `leanzeroLink/nodes`.
 */
function withNode(params: Record<string, unknown>, nodeId?: string): Record<string, unknown> {
  if (nodeId != null) params.nodeId = nodeId;
  return params;
}

export async function mlxEngineStatus(
  nodeId?: string,
  fitModelId?: string | null
): Promise<MlxEngineStatus> {
  const response = await call<{ status: MlxEngineStatus }>(
    '_goose/unstable/mlxEngine/status',
    withNode(fitModelId ? { fitModelId } : {}, nodeId)
  );
  if (nodeId == null) reportToMain(response.status);
  return response.status;
}

/**
 * Every LOCAL status read, wherever it happens (the Providers view, the chat selector, the tray's
 * mount), tells MAIN what goose said, so the menu-bar monitor wakes on the same fact the view saw.
 * A linked peer's status is never reported: main reads only this machine's engine.
 */
function reportToMain(status: MlxEngineStatus): void {
  const report = (
    window as unknown as {
      electron?: { mlxEngineReport?: (r: Record<string, string | undefined>) => void };
    }
  ).electron?.mlxEngineReport;
  report?.({
    state: status.state,
    baseUrl: status.baseUrl,
    modelId: status.modelId,
    servedModelId: status.servedModelId,
    lastError: status.lastError,
  });
}

/**
 * The mount gate's refusal as goose answers it (`MlxMountRefusalDto`): the fit rule's verdict and,
 * when the model fits somewhere else, the placement that would work (a placement candidate whose
 * `action` says how to start it) and the model's badge from the same plan.
 */
export interface MlxMountRefusal {
  fit: { modelId: string; verdict: string; message: string; shortBytes?: number | null };
  alternative?: MlxPlacementCandidateDto | null;
  badge?: MlxPlacementBadgeDto | null;
  alternativeError?: string | null;
}

/**
 * A refused mount. goose answers a gate refusal as a RESULT (`{ refusal }`), not an error; every
 * caller of `mlxEngineMount` awaited a void and would read that answer as "mounting started", so
 * the refusal is thrown here — with the fit rule's own words as the message — and every existing
 * catch arm shows it. The Providers view reads `refusal` for the placement that would work.
 */
export class MlxMountRefusedError extends Error {
  readonly refusal: MlxMountRefusal;
  constructor(refusal: MlxMountRefusal) {
    super(refusal.fit.message);
    this.name = 'MlxMountRefusedError';
    this.refusal = refusal;
  }
}

/**
 * Returns once mounting started; state flips to "mounting" — poll status for running/failed. A gate
 * refusal throws `MlxMountRefusedError`.
 */
export async function mlxEngineMount(modelId: string, nodeId?: string): Promise<void> {
  const response = await call<{ refusal?: MlxMountRefusal | null } | null | undefined>(
    '_goose/unstable/mlxEngine/mount',
    withNode({ modelId }, nodeId)
  );
  if (response?.refusal) throw new MlxMountRefusedError(response.refusal);
}

export async function mlxEngineUnmount(nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/unmount', withNode({}, nodeId));
}

export async function mlxEngineSettingsRead(nodeId?: string): Promise<MlxEngineSettings> {
  const response = await call<{ settings: MlxEngineSettings }>(
    '_goose/unstable/mlxEngine/settingsRead',
    withNode({}, nodeId)
  );
  return response.settings;
}

/**
 * Persist settings. Optional sampling fields left `undefined` are OMITTED on the wire
 * (JSON-RPC serialization drops undefined keys), which is how "engine default" is expressed —
 * an explicit 0 and an unset field are different facts and both survive the round trip.
 */
export async function mlxEngineSettingsUpdate(
  settings: MlxEngineSettings,
  nodeId?: string
): Promise<MlxEngineSettings> {
  const response = await call<{ settings: MlxEngineSettings }>(
    '_goose/unstable/mlxEngine/settingsUpdate',
    withNode({ settings }, nodeId)
  );
  return response.settings;
}

export async function mlxEngineModelsList(nodeId?: string): Promise<MlxModelsList> {
  return await call<MlxModelsList>('_goose/unstable/mlxEngine/modelsList', withNode({}, nodeId));
}

export async function mlxEngineModelDelete(modelId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/modelDelete', withNode({ modelId }, nodeId));
}

export async function mlxEngineHfSearch(
  query: string,
  limit?: number,
  nodeId?: string
): Promise<MlxHfModelHit[]> {
  const params: Record<string, unknown> = { query };
  if (limit != null) params.limit = limit;
  const response = await call<{ hits: MlxHfModelHit[] }>(
    '_goose/unstable/mlxEngine/hfSearch',
    withNode(params, nodeId)
  );
  return response.hits;
}

/**
 * Paginated MLX-only Hugging Face browse. All four filters are SERVER-side; pass a page's
 * `nextCursor` back as `cursor` to append the next page. Undefined optional params are
 * omitted on the wire.
 */
export async function mlxEngineBrowse(
  params: MlxBrowseParams,
  nodeId?: string
): Promise<MlxBrowsePage> {
  const payload: Record<string, unknown> = { sort: params.sort };
  if (params.query != null && params.query !== '') payload.query = params.query;
  if (params.author != null && params.author !== '') payload.author = params.author;
  if (params.quant != null && params.quant !== '') payload.quant = params.quant;
  if (params.arch != null && params.arch !== '') payload.arch = params.arch;
  if (params.cursor != null) payload.cursor = params.cursor;
  if (params.limit != null) payload.limit = params.limit;
  return await call<MlxBrowsePage>('_goose/unstable/mlxEngine/browse', withNode(payload, nodeId));
}

export async function mlxEngineDownload(repoId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/download', withNode({ repoId }, nodeId));
}

/** `null` when no download was ever tracked for this repo. */
export async function mlxEngineDownloadProgress(
  repoId: string,
  nodeId?: string
): Promise<MlxDownloadProgress | null> {
  const response = await call<{ progress?: MlxDownloadProgress }>(
    '_goose/unstable/mlxEngine/downloadProgress',
    withNode({ repoId }, nodeId)
  );
  return response.progress ?? null;
}

/**
 * Cancel a download AND delete its on-disk claim: every `.part` and the whole partial
 * repo directory. For an active task the deletion runs as the task stops (poll progress
 * until "cancelled"); for a paused/failed one it has already run when this returns.
 */
export async function mlxEngineDownloadCancel(repoId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/downloadCancel', withNode({ repoId }, nodeId));
}

/** Pause an active download; every `.part` stays on disk for a later resume. */
export async function mlxEngineDownloadPause(repoId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/downloadPause', withNode({ repoId }, nodeId));
}

/**
 * Resume a paused/failed download — or partial residue on disk from an earlier session
 * that was never tracked by this one. Complete files are skipped, `.part` files continue
 * via HTTP Range; a mismatched partial restarts from zero and lands in `restartedFiles`.
 */
export async function mlxEngineDownloadResume(repoId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/downloadResume', withNode({ repoId }, nodeId));
}

/** Cached backend-side (~1h TTL) — the first call after a cold start pays the crawl. */
export async function mlxEngineBrowseFilters(nodeId?: string): Promise<MlxBrowseFilters> {
  return await call<MlxBrowseFilters>(
    '_goose/unstable/mlxEngine/browseFilters',
    withNode({}, nodeId)
  );
}

export async function mlxEngineModelCard(repoId: string, nodeId?: string): Promise<MlxModelCard> {
  return await call<MlxModelCard>(
    '_goose/unstable/mlxEngine/modelCard',
    withNode({ repoId }, nodeId)
  );
}

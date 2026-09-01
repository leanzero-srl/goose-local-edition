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
  availableMemoryGb: number;
  totalMemoryGb: number;
  lastError?: string;
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

export interface MlxLocalModel {
  id: string;
  sizeBytes: number;
  complete: boolean;
  /**
   * Files provably missing or unfinished — shards the model's safetensors index names
   * that are absent/empty, plus `.part` leftovers. 0 when complete.
   */
  missingFiles: number;
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
   * ESTIMATE of the weight payload in bytes, derived from the server's safetensors dtype
   * counts (excludes tokenizer/config files). Absent = unknown — show nothing, never a dash.
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

export async function mlxEngineStatus(nodeId?: string): Promise<MlxEngineStatus> {
  const response = await call<{ status: MlxEngineStatus }>(
    '_goose/unstable/mlxEngine/status',
    withNode({}, nodeId)
  );
  return response.status;
}

/** Returns immediately; state flips to "mounting" — poll status for running/failed. */
export async function mlxEngineMount(modelId: string, nodeId?: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/mount', withNode({ modelId }, nodeId));
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
  return await call<MlxModelCard>('_goose/unstable/mlxEngine/modelCard', withNode({ repoId }, nodeId));
}

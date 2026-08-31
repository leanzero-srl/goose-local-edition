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
}

export interface MlxBrowsePage {
  hits: MlxBrowseHit[];
  /** Opaque continuation for the next page; absent on the last page. */
  nextCursor?: string;
}

export type MlxDownloadState = 'queued' | 'downloading' | 'done' | 'failed' | 'cancelled';

export interface MlxDownloadProgress {
  state: MlxDownloadState;
  totalBytes: number;
  downloadedBytes: number;
  currentFile?: string;
  error?: string;
}

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

export async function mlxEngineStatus(): Promise<MlxEngineStatus> {
  const response = await call<{ status: MlxEngineStatus }>('_goose/unstable/mlxEngine/status', {});
  return response.status;
}

/** Returns immediately; state flips to "mounting" — poll status for running/failed. */
export async function mlxEngineMount(modelId: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/mount', { modelId });
}

export async function mlxEngineUnmount(): Promise<void> {
  await call('_goose/unstable/mlxEngine/unmount', {});
}

export async function mlxEngineSettingsRead(): Promise<MlxEngineSettings> {
  const response = await call<{ settings: MlxEngineSettings }>(
    '_goose/unstable/mlxEngine/settingsRead',
    {}
  );
  return response.settings;
}

/**
 * Persist settings. Optional sampling fields left `undefined` are OMITTED on the wire
 * (JSON-RPC serialization drops undefined keys), which is how "engine default" is expressed —
 * an explicit 0 and an unset field are different facts and both survive the round trip.
 */
export async function mlxEngineSettingsUpdate(
  settings: MlxEngineSettings
): Promise<MlxEngineSettings> {
  const response = await call<{ settings: MlxEngineSettings }>(
    '_goose/unstable/mlxEngine/settingsUpdate',
    { settings }
  );
  return response.settings;
}

export async function mlxEngineModelsList(): Promise<MlxLocalModel[]> {
  const response = await call<{ models: MlxLocalModel[] }>(
    '_goose/unstable/mlxEngine/modelsList',
    {}
  );
  return response.models;
}

export async function mlxEngineModelDelete(modelId: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/modelDelete', { modelId });
}

export async function mlxEngineHfSearch(query: string, limit?: number): Promise<MlxHfModelHit[]> {
  const params: Record<string, unknown> = { query };
  if (limit != null) params.limit = limit;
  const response = await call<{ hits: MlxHfModelHit[] }>('_goose/unstable/mlxEngine/hfSearch', params);
  return response.hits;
}

/**
 * Paginated MLX-only Hugging Face browse. All four filters are SERVER-side; pass a page's
 * `nextCursor` back as `cursor` to append the next page. Undefined optional params are
 * omitted on the wire.
 */
export async function mlxEngineBrowse(params: MlxBrowseParams): Promise<MlxBrowsePage> {
  const payload: Record<string, unknown> = { sort: params.sort };
  if (params.query != null && params.query !== '') payload.query = params.query;
  if (params.author != null && params.author !== '') payload.author = params.author;
  if (params.quant != null && params.quant !== '') payload.quant = params.quant;
  if (params.arch != null && params.arch !== '') payload.arch = params.arch;
  if (params.cursor != null) payload.cursor = params.cursor;
  if (params.limit != null) payload.limit = params.limit;
  return await call<MlxBrowsePage>('_goose/unstable/mlxEngine/browse', payload);
}

export async function mlxEngineDownload(repoId: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/download', { repoId });
}

/** `null` when no download was ever tracked for this repo. */
export async function mlxEngineDownloadProgress(
  repoId: string
): Promise<MlxDownloadProgress | null> {
  const response = await call<{ progress?: MlxDownloadProgress }>(
    '_goose/unstable/mlxEngine/downloadProgress',
    { repoId }
  );
  return response.progress ?? null;
}

export async function mlxEngineDownloadCancel(repoId: string): Promise<void> {
  await call('_goose/unstable/mlxEngine/downloadCancel', { repoId });
}

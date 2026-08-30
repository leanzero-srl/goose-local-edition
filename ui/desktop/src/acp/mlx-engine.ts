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
  availableMemoryGb: number;
  totalMemoryGb: number;
  lastError?: string;
}

export interface MlxEngineSettings {
  modelId?: string;
  modelsDir: string;
  port: number;
  contextLimit?: number;
  temperature?: number;
  topP?: number;
  topK?: number;
  minP?: number;
  repetitionPenalty?: number;
  presencePenalty?: number;
  frequencyPenalty?: number;
  spawnCommand: string[];
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

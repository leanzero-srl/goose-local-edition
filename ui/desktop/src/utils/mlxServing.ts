import type { FetchLike } from './fleetProbe';

/**
 * WHO the local LeanZero MLX engine is serving. Rapid-MLX cannot tell (its /v1/status lists requests
 * by an internal id); goose can, at the two doors its own work leaves through — the swarm router's
 * lease on the mlx-sidecar node and the OpenAI-compatible route — and `goose serve` lists that work
 * at `GET /mlx-engine/serving` (crates/goose/src/api/mlx_serving.rs). MAIN reads it beside the
 * engine's own request list; everything the engine runs that neither door explains is COUNTED as
 * unattributed, never named: a `goose swarm run` child, another app on the port, a linked peer.
 */

/** The provider id goose dispatches the MLX engine through (leanzeroSelectorPolicy.MLX_PROVIDER_ID). */
export const MLX_ENGINE_PROVIDER = 'omlx';

/** One row of `GET /mlx-engine/serving` (serde camelCase, entry flattened into the row). */
export interface MlxServingRow {
  id: number;
  via: 'swarmRouter' | 'openaiApi';
  sessionId: string | null;
  provider: string;
  model: string;
  nodeId: string | null;
  startedAt: string;
  sessionName: string | null;
  /** goose's SessionType, snake_case: `user`, `scheduled`, `sub_agent`, `hidden`, `terminal`, … */
  sessionType: string | null;
  sessionError: string | null;
  /**
   * The linked Mac whose engine a router lease went to (a remote single), by its one name; absent or
   * null = this Mac's engine. A backend older than the field never leased a linked Mac's engine at
   * all (its router listed only `mlx-sidecar` leases), so absent really does mean this Mac.
   */
  peer?: string | null;
}

export type MlxClient =
  /** A chat in this app, routed to the engine by the swarm router. */
  | { key: string; kind: 'chat'; sessionId: string; sessionName: string | null; count: number }
  /** An external client's `POST /v1/chat/completions` whose turn runs on the engine. */
  | { key: string; kind: 'external'; model: string; count: number }
  /** Any other goose session on the engine (a sub-agent, a scheduled job) — named by its own type. */
  | {
      key: string;
      kind: 'session';
      sessionId: string | null;
      sessionName: string | null;
      sessionType: string | null;
      count: number;
    };

export interface MlxServing {
  clients: MlxClient[];
  /** Requests the engine reports that no listed client explains. */
  unattributed: number;
  /** Swarm runs this app knows are live — stated beside the count, never as its explanation. */
  swarmRuns: string[];
  /** Why goose's own list could not be read (the rows are then empty, not assumed empty). */
  error: string | null;
}

/**
 * The rows that belong to the engine being read: this Mac's (`onPeer` false — the single engine or a
 * split's rank 0) or the linked Mac's that serves this Mac's chat (`onPeer` true). A router lease
 * says which by its `peer`; an external /v1 row follows the router lease of its session, and one
 * with no lease went straight to a provider on this Mac. Without this, this app's own turn on the
 * Studio was counted "not from this app's chats" (3.0.29, live).
 */
export function servingRowsForEngine(
  rows: readonly MlxServingRow[],
  onPeer: boolean
): MlxServingRow[] {
  const peerSessions = new Set<string>();
  for (const row of rows) {
    if (row.via === 'swarmRouter' && row.peer && row.sessionId) peerSessions.add(row.sessionId);
  }
  return rows.filter((row) => {
    if (row.via === 'swarmRouter') return Boolean(row.peer) === onPeer;
    const leasedOnPeer = row.sessionId !== null && peerSessions.has(row.sessionId);
    return leasedOnPeer === onPeer;
  });
}

/**
 * Join goose's in-flight rows to the engine's own request count. A router row IS on the engine; an
 * API row is on it when its provider is the engine's, or when a router row carries its session (the
 * external request was routed through `swarm`). API rows on any other provider are not this engine's.
 */
export function attributeServing(
  rows: readonly MlxServingRow[],
  engineRequests: number,
  swarmRuns: readonly string[],
  error: string | null
): MlxServing {
  const apiBySession = new Map<string, MlxServingRow>();
  for (const row of rows) {
    if (row.via === 'openaiApi' && row.sessionId) apiBySession.set(row.sessionId, row);
  }
  const clients = new Map<string, MlxClient>();
  const add = (client: MlxClient) => {
    const existing = clients.get(client.key);
    if (existing) existing.count += 1;
    else clients.set(client.key, client);
  };
  const routedApiSessions = new Set<string>();
  for (const row of rows) {
    if (row.via !== 'swarmRouter') continue;
    const api = row.sessionId ? apiBySession.get(row.sessionId) : undefined;
    if (api && row.sessionId) {
      routedApiSessions.add(row.sessionId);
      add({ key: `external:${row.sessionId}`, kind: 'external', model: modelRef(api), count: 1 });
    } else if (row.sessionId && row.sessionType === 'user') {
      add({
        key: `chat:${row.sessionId}`,
        kind: 'chat',
        sessionId: row.sessionId,
        sessionName: row.sessionName,
        count: 1,
      });
    } else {
      add({
        key: `session:${row.sessionId ?? `row-${row.id}`}`,
        kind: 'session',
        sessionId: row.sessionId,
        sessionName: row.sessionName,
        sessionType: row.sessionType,
        count: 1,
      });
    }
  }
  for (const row of rows) {
    if (row.via !== 'openaiApi' || row.provider !== MLX_ENGINE_PROVIDER) continue;
    if (row.sessionId && routedApiSessions.has(row.sessionId)) continue;
    add({
      key: `external:${row.sessionId ?? `row-${row.id}`}`,
      kind: 'external',
      model: modelRef(row),
      count: 1,
    });
  }
  const list = [...clients.values()];
  const attributed = list.reduce((n, c) => n + c.count, 0);
  return {
    clients: list,
    unattributed: Math.max(0, engineRequests - attributed),
    swarmRuns: [...swarmRuns],
    error,
  };
}

function modelRef(row: MlxServingRow): string {
  return `${row.provider}/${row.model}`;
}

/** `http://host:port` of a goose serve backend from its ACP websocket URL (`ws://host:port/acp?token=`). */
export function serveHttpBase(acpUrl: string): string | null {
  let url: URL;
  try {
    url = new URL(acpUrl);
  } catch {
    return null;
  }
  const protocol =
    url.protocol === 'wss:' ? 'https:' : url.protocol === 'ws:' ? 'http:' : url.protocol;
  if (protocol !== 'http:' && protocol !== 'https:') return null;
  return `${protocol}//${url.host}`;
}

export type MlxServingRead = { ok: true; rows: MlxServingRow[] } | { ok: false; detail: string };

function isRow(v: unknown): v is MlxServingRow {
  if (v == null || typeof v !== 'object') return false;
  const r = v as Record<string, unknown>;
  return (
    typeof r.id === 'number' &&
    (r.via === 'swarmRouter' || r.via === 'openaiApi') &&
    typeof r.provider === 'string' &&
    typeof r.model === 'string'
  );
}

/** One backend's `GET /mlx-engine/serving`, under its own secret; every failure is named. */
export async function fetchMlxServing(
  httpBase: string,
  secretKey: string,
  fetchImpl: FetchLike,
  timeoutMs: number
): Promise<MlxServingRead> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetchImpl(`${httpBase}/mlx-engine/serving`, {
      method: 'GET',
      headers: { 'X-Secret-Key': secretKey },
      signal: controller.signal,
    });
    if (res.status === 404) {
      return { ok: false, detail: 'this goose backend predates /mlx-engine/serving' };
    }
    if (!res.ok) return { ok: false, detail: `goose backend returned ${res.status}` };
    const body = (await res.json()) as { serving?: unknown };
    if (!Array.isArray(body?.serving)) {
      return { ok: false, detail: 'goose backend answered without a serving list' };
    }
    const bad = body.serving.findIndex((row) => !isRow(row));
    if (bad >= 0) return { ok: false, detail: `serving row ${bad} is not a serving row` };
    return { ok: true, rows: body.serving as MlxServingRow[] };
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      return { ok: false, detail: `goose backend did not answer within ${timeoutMs} ms` };
    }
    return { ok: false, detail: err instanceof Error ? err.message : String(err) };
  } finally {
    clearTimeout(timer);
  }
}

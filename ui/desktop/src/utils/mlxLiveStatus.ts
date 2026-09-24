import type { FetchLike } from './fleetProbe';
import { MLX_STATUS_POLL_MS } from '../components/leanzero-swarm/mlxLiveStats';

/**
 * The LeanZero MLX engine's live instrument read, done by MAIN (IPC `mlx-live-status`): a GET of
 * Rapid-MLX's own `<baseUrl>/v1/status` — the in-flight requests with their phase and token counts,
 * the decode rate, Metal memory and the prefix-cache hit rate. Main has no CSP in the path and the
 * ACP status DTO stays untouched: this is a read of the engine's own public status route, nothing
 * the sidecar computes.
 *
 * LOCAL ONLY, by construction: the engine binds loopback, and a base URL that is not a loopback host
 * is refused (`bad-base-url`) — a renderer string never turns main into a fetcher of arbitrary hosts,
 * and a linked peer's `127.0.0.1` is never read on THIS machine as if it were the peer's engine (the
 * renderer only asks for a local target). A remote single's engine is read through goosed's own
 * loopback relay to it (`http://127.0.0.1:<port>/relay/<capability>`): the base's PATH is kept, so
 * `<base>/v1/status` reaches the peer engine's status through the relay — never a peer address.
 */

export type MlxLiveStatusError =
  | 'bad-base-url' // not an http loopback base; nothing was fetched
  | 'timeout' // the engine did not answer inside one poll interval
  | 'unreachable' // connection refused — no listener on the port
  | 'http' // non-2xx
  | 'bad-json'; // 2xx with a body that is not JSON

export type MlxLiveStatusResult =
  | { ok: true; url: string; body: unknown }
  | { ok: false; url: string; error: MlxLiveStatusError; detail: string; status?: number };

/**
 * A transport bound on ONE status GET (it decides no model work): three quarters of the status
 * poll, so a slow read never stacks behind the next one. Over LeanZero Link the read crosses the
 * relay and the peer's control service; the mesh round trip to the Studio measured 1 ms, so a read
 * that runs out this bound is a stalled path or engine — the tile says so, it never shows an old
 * number.
 */
export const MLX_LIVE_STATUS_TIMEOUT_MS = Math.round((MLX_STATUS_POLL_MS * 3) / 4); // ratio: ¾ of the status poll; measured: `tailscale ping` Mac→Work's Mac Studio over Link 2026-09-24, direct 192.168.0.2, 10/10 at 1 ms

const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]', '::1']);

/** `<base>/v1/status` for a loopback http base; throws with the offending text otherwise. */
export function mlxLiveStatusUrl(baseUrl: string): string {
  let url: URL;
  try {
    url = new URL(baseUrl);
  } catch {
    throw new Error(`engine base URL is not a URL: ${baseUrl || '(empty)'}`);
  }
  if (url.protocol !== 'http:') {
    throw new Error(`engine base URL is not http: ${baseUrl}`);
  }
  if (!LOOPBACK_HOSTS.has(url.hostname)) {
    throw new Error(`engine base URL is not a loopback host: ${baseUrl}`);
  }
  const host = url.hostname === 'localhost' ? '127.0.0.1' : url.hostname;
  const path = url.pathname.replace(/\/+$/, '');
  return `http://${host}${url.port ? `:${url.port}` : ''}${path}/v1/status`;
}

export async function fetchMlxLiveStatus(
  baseUrl: string,
  fetchImpl: FetchLike,
  timeoutMs = MLX_LIVE_STATUS_TIMEOUT_MS
): Promise<MlxLiveStatusResult> {
  let url: string;
  try {
    url = mlxLiveStatusUrl(baseUrl);
  } catch (err) {
    return {
      ok: false,
      url: baseUrl,
      error: 'bad-base-url',
      detail: err instanceof Error ? err.message : String(err),
    };
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetchImpl(url, { method: 'GET', signal: controller.signal });
    if (!res.ok) {
      return {
        ok: false,
        url,
        error: 'http',
        status: res.status,
        detail: `engine returned ${res.status}`,
      };
    }
    try {
      return { ok: true, url, body: await res.json() };
    } catch (err) {
      return {
        ok: false,
        url,
        error: 'bad-json',
        detail: err instanceof Error ? err.message : String(err),
      };
    }
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      return { ok: false, url, error: 'timeout', detail: `no answer within ${timeoutMs} ms` };
    }
    const cause = (err as { cause?: unknown })?.cause;
    const detail =
      cause instanceof Error ? cause.message : err instanceof Error ? err.message : String(err);
    return { ok: false, url, error: 'unreachable', detail };
  } finally {
    clearTimeout(timer);
  }
}

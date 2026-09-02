import { cspSafe } from './csp';

/**
 * The fleet probes, done by the MAIN process (IPC `fleet-probe` / `fleet-chat`), so the renderer's CSP
 * is not in the path at all.
 *
 * WHY MAIN (gate 8 refutation of 949d3fa6e, 2026-09-02): the renderer's effective CSP is the INTERSECTION
 * of index.html's static meta (`connect-src 'self' http://127.0.0.1:* https: ws: wss:`) and the header
 * main installs in onHeadersReceived. The meta is copied verbatim into the packaged renderer, which is
 * loaded over file:// (main.ts getAppUrl → pathToFileURL), so no origin the header adds can widen it:
 * a LAN LM Studio (`swarm.endpoint: http://192.168.8.220:1234`) was blocked from the renderer no matter
 * what, and every fleet probe read "offline" with the right host name. Main has no document and no CSP;
 * `net.fetch` there reaches whatever host the engine is configured for, and the renderer keeps the
 * exact hook/prop shapes it had (useFleet, fetchSwarmContextLimit, the wizard's `complete`).
 *
 * Both functions take the fetch implementation so the branches are testable without a network.
 */

/** The endpoint's http(s) origin. Throws on anything else so a probe against a bad value fails loudly
 *  (offline, naming the configured text) rather than probing some other host — `new URL('localhost:1234')`
 *  parses as scheme `localhost:` with origin `null`, which would otherwise become `null/api/v0/models`. */
function swarmOriginOf(endpoint: string): string {
  const url = new URL(endpoint);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error(`swarm endpoint is not an http(s) host base: ${endpoint}`);
  }
  return url.origin;
}

/** `<origin>/api/v0/models` from a host base — the FETCH url. A `localhost` base fetches 127.0.0.1
 *  (csp.ts `cspSafe`); the DISPLAY text a card shows stays the configured base verbatim. */
export function modelsUrl(endpoint: string): string {
  return cspSafe(`${swarmOriginOf(endpoint)}/api/v0/models`);
}

/** `<origin>/v1/chat/completions` from a host base — LM Studio's OpenAI-compatible chat route. */
export function chatCompletionsUrl(endpoint: string): string {
  return cspSafe(`${swarmOriginOf(endpoint)}/v1/chat/completions`);
}

/** Why a probe produced no JSON — every arm is NAMED so the renderer's offline state is honest. */
export type FleetProbeError =
  | 'bad-endpoint' // the configured text is not an http(s) host base; nothing was fetched
  | 'timeout' // the host did not answer within the probe's window
  | 'unreachable' // connection refused / no route / DNS — LM Studio is not listening there
  | 'http' // the host answered with a non-2xx status
  | 'bad-json'; // the host answered 2xx with a body that is not JSON

export type FleetProbeResult =
  | { ok: true; url: string; data: Array<Record<string, unknown>> }
  | { ok: false; url: string; error: FleetProbeError; detail: string; status?: number };

export type FleetChatResult =
  | { ok: true; url: string; body: unknown }
  | { ok: false; url: string; error: FleetProbeError; detail: string; status?: number };

export type FetchLike = (url: string, init?: RequestInit) => Promise<Response>;

/** The discovery probe's window — what the renderer used before the probe moved to main. */
export const FLEET_PROBE_TIMEOUT_MS = 3000;
/** The wizard's chat window — a weak local model drafting a recipe; what the renderer used before. */
export const FLEET_CHAT_TIMEOUT_MS = 120_000;

function classify(err: unknown): { error: FleetProbeError; detail: string } {
  if (err instanceof Error && err.name === 'AbortError') {
    return { error: 'timeout', detail: 'no answer within the probe window' };
  }
  const cause = (err as { cause?: unknown })?.cause;
  const detail =
    cause instanceof Error ? cause.message : err instanceof Error ? err.message : String(err);
  return { error: 'unreachable', detail };
}

async function fetchJson(
  url: string,
  init: RequestInit,
  timeoutMs: number,
  fetchImpl: FetchLike
): Promise<{ ok: true; body: unknown } | { ok: false; error: FleetProbeError; detail: string; status?: number }> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetchImpl(url, { ...init, signal: controller.signal });
    if (!res.ok) {
      return { ok: false, error: 'http', status: res.status, detail: `fleet returned ${res.status}` };
    }
    try {
      return { ok: true, body: await res.json() };
    } catch (err) {
      return { ok: false, error: 'bad-json', detail: err instanceof Error ? err.message : String(err) };
    }
  } catch (err) {
    return { ok: false, ...classify(err) };
  } finally {
    clearTimeout(timer);
  }
}

/** GET `<endpoint>/api/v0/models`; the `data` array as LM Studio sent it, or a named error. */
export async function probeFleetModels(
  endpoint: string,
  fetchImpl: FetchLike,
  timeoutMs = FLEET_PROBE_TIMEOUT_MS
): Promise<FleetProbeResult> {
  let url: string;
  try {
    url = modelsUrl(endpoint);
  } catch (err) {
    return {
      ok: false,
      url: endpoint,
      error: 'bad-endpoint',
      detail: err instanceof Error ? err.message : String(err),
    };
  }
  const r = await fetchJson(url, { method: 'GET' }, timeoutMs, fetchImpl);
  if (!r.ok) return { url, ...r };
  const data = (r.body as { data?: unknown } | null)?.data;
  return { ok: true, url, data: Array.isArray(data) ? (data as Array<Record<string, unknown>>) : [] };
}

/** POST `<endpoint>/v1/chat/completions` with `body` (non-streaming); the JSON reply or a named error. */
export async function postFleetChat(
  endpoint: string,
  body: unknown,
  fetchImpl: FetchLike,
  timeoutMs = FLEET_CHAT_TIMEOUT_MS
): Promise<FleetChatResult> {
  let url: string;
  try {
    url = chatCompletionsUrl(endpoint);
  } catch (err) {
    return {
      ok: false,
      url: endpoint,
      error: 'bad-endpoint',
      detail: err instanceof Error ? err.message : String(err),
    };
  }
  const r = await fetchJson(
    url,
    { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) },
    timeoutMs,
    fetchImpl
  );
  if (!r.ok) return { url, ...r };
  return { ok: true, url, body: r.body };
}

import { useEffect, useState } from 'react';
import { acpReadConfig } from '../../acp/config';
import { cspSafe } from '../../utils/csp';
import { DEFAULTS, type SwarmConfig } from '../settings/swarm/golden';
import { NodeLane, NodeStatus } from './FanInCard';

/**
 * Live fleet discovery for Goose Local Edition — reads the LM Studio / LM Link endpoint so the swarm
 * fan-in reflects the REAL resident nodes, never hardcoded sample data. This mirrors what the CLI swarm
 * does with `lms ps` (auto-pool from resident models), but from the desktop over LM Studio's native
 * `/api/v0/models` endpoint (returns each model's id, arch, state).
 *
 * WHICH HOST (U-M3, branch review 2026-09-01): the engine talks to `swarm.endpoint` from config.yaml
 * (swarm.rs sets LMSTUDIO_HOST from it), while every desktop probe was pinned to 127.0.0.1:1234 — so a
 * fleet configured on another machine read "offline" here while the engine was building on it, and the
 * settings card named one host in its message and probed another. Every URL below is DERIVED from the
 * configured endpoint, which is a HOST BASE (`http://localhost:1234`, golden.ts DEFAULTS — the engine's
 * baked default_endpoint).
 *
 * LOOPBACK (restored 2026-09-02 after gate 8 refuted 949d3fa6e): the renderer's CSP is the INTERSECTION of
 * index.html's static meta (`connect-src 'self' http://127.0.0.1:* https: ws: wss:`) and main's header, so
 * a `localhost` probe is blocked no matter what the header allows — the default install read "offline"
 * with the right host name. The fetch URL is loopback-normalised to 127.0.0.1 (`cspSafe`), the display
 * text is not.
 */

/** The engine's own default host base — what an absent `swarm.endpoint` means, not a UI fallback. */
export const DEFAULT_SWARM_ENDPOINT: string = DEFAULTS.endpoint ?? 'http://localhost:1234';

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
 *  (csp.ts `cspSafe`: the static meta CSP in index.html allows `http://127.0.0.1:*` and blocks the
 *  `localhost` origin — measured d28443d90, regressed 949d3fa6e). The DISPLAY text (`FleetState.endpoint`)
 *  stays the configured host base verbatim; loopback is loopback either way. */
export function modelsUrl(endpoint: string): string {
  return cspSafe(`${swarmOriginOf(endpoint)}/api/v0/models`);
}

/** `<origin>/v1/chat/completions` from a host base — LM Studio's OpenAI-compatible chat route; the same
 *  loopback rewrite as `modelsUrl`. */
export function chatCompletionsUrl(endpoint: string): string {
  return cspSafe(`${swarmOriginOf(endpoint)}/v1/chat/completions`);
}

/** The configured `swarm.endpoint`, or the engine's default when the key is absent. An unreadable
 *  config answers the default too, because that is exactly what the engine runs with in that case. */
export async function resolveSwarmEndpoint(): Promise<string> {
  try {
    const raw = (await acpReadConfig('swarm', false)) as SwarmConfig | null;
    const ep = typeof raw?.endpoint === 'string' ? raw.endpoint.trim() : '';
    return ep || DEFAULT_SWARM_ENDPOINT;
  } catch {
    return DEFAULT_SWARM_ENDPOINT;
  }
}

export interface FleetState {
  lanes: NodeLane[];
  /** Raw LM Studio model identifiers of the loaded, non-embedding models (e.g. 'mihai-qwopus3.6-27b-coder-mlx').
   *  The swarm's planner_model is matched by EXACT equality against these, so a picker must offer them verbatim. */
  models: string[];
  online: boolean;
  loading: boolean;
  /** The HOST BASE being probed — the same text the settings card shows, so "offline at X" is X. Empty
   *  until the configured endpoint has been resolved (or while discovery is disabled). */
  endpoint: string;
}

/**
 * One-shot read of the fleet's real context window. LM Studio's /api/v0/models returns per-model
 * `loaded_context_length` (what the model was actually loaded with) and `max_context_length` (its ceiling).
 * Returns the MIN across loaded non-embedding models — the honest ceiling the whole fleet can rely on — or
 * null if the endpoint is unreachable or reports no usable length (caller then keeps its own default).
 * Without an explicit host base it probes the CONFIGURED one — the host the engine actually runs against.
 */
export async function fetchSwarmContextLimit(endpoint?: string): Promise<number | null> {
  try {
    const base = endpoint ?? (await resolveSwarmEndpoint());
    const res = await fetchWithTimeout(modelsUrl(base), 3000);
    const data = (await res.json()) as { data?: Array<Record<string, unknown>> };
    const loaded = (data.data ?? []).filter(
      (m) => m['state'] === 'loaded' && m['type'] !== 'embeddings'
    );
    const limits = loaded
      .map((m) => {
        const ctx = m['loaded_context_length'] ?? m['max_context_length'];
        return typeof ctx === 'number' && ctx > 0 ? ctx : null;
      })
      .filter((n): n is number => n != null);
    return limits.length ? Math.min(...limits) : null;
  } catch {
    return null;
  }
}

/** Derive a node/device name from an LM Link model id: the prefix before the first '-' (mihai-, workhorse-, gabee-). */
export function deviceFromModelId(id: string): string {
  const bare = id.split('/').pop() || id; // strip any publisher/ prefix
  const dash = bare.indexOf('-');
  return dash > 0 ? bare.slice(0, dash) : bare;
}

/**
 * LM Studio's OWN live per-node status via `lms ps --json` (through the main process) — the ground truth for
 * "is this node generating RIGHT NOW", which the REST /api/v0/models cannot report. Keyed by node short name
 * (gabee/mihai/workhorse via deviceFromModelId) so the panel can cross-check the goose digest against it.
 * Empty when LM Studio / lms is unavailable, so the caller degrades to the digest-only view.
 */
export function useFleetStatus(pollMs = 1500, enabled = true): Record<string, string> {
  const [status, setStatus] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!enabled) {
      // LM Studio surfaces disabled (showLmStudioFleet, default off): no lms polling, no statuses —
      // callers degrade to their digest-only view exactly as when lms is unavailable.
      setStatus({});
      return undefined;
    }
    let alive = true;
    const poll = async () => {
      try {
        const raw = (await window.electron.fleetStatus()) || {};
        if (!alive) return;
        const byNode: Record<string, string> = {};
        for (const [id, st] of Object.entries(raw)) {
          byNode[deviceFromModelId(id)] = st;
        }
        setStatus(byNode);
      } catch {
        if (alive) setStatus({});
      }
    };
    poll();
    const t = setInterval(poll, pollMs);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [pollMs, enabled]);
  return status;
}

async function fetchWithTimeout(url: string, ms: number): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ms);
  try {
    return await fetch(url, { signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Poll the LM Link endpoint for resident models and map them to fan-in lanes.
 * `endpoint` is the HOST BASE (`swarm.endpoint`); callers holding the swarm config pass `cfg.endpoint`,
 * the rest pass nothing and the configured value is resolved here — no discovery fetch goes out before
 * it is known, so the first probe is never against a host the engine does not use.
 * `online: false` means the endpoint is unreachable (LM Studio not running) — the caller shows an
 * explicit offline state rather than fabricating data.
 */
export function useFleet(pollMs = 5000, endpoint?: string, enabled = true): FleetState {
  const [resolved, setResolved] = useState<string | null>(endpoint ?? null);
  useEffect(() => {
    if (endpoint) {
      setResolved(endpoint);
      return undefined;
    }
    if (!enabled) return undefined;
    let alive = true;
    void resolveSwarmEndpoint().then((ep) => {
      if (alive) setResolved(ep);
    });
    return () => {
      alive = false;
    };
  }, [endpoint, enabled]);

  const [state, setState] = useState<FleetState>({
    lanes: [],
    models: [],
    online: false,
    loading: true,
    endpoint: resolved ?? '',
  });

  useEffect(() => {
    if (!enabled) {
      // LM Studio surfaces disabled (showLmStudioFleet, default off): no discovery fetches, and the
      // state reads exactly like an unreachable endpoint — no fabricated rows.
      setState({ lanes: [], models: [], online: false, loading: false, endpoint: '' });
      return undefined;
    }
    if (!resolved) return undefined;
    const base = resolved;
    let alive = true;

    const tick = async () => {
      try {
        const res = await fetchWithTimeout(modelsUrl(base), 3000);
        const data = (await res.json()) as { data?: Array<Record<string, unknown>> };
        const loaded = (data.data ?? []).filter(
          (m) => m['state'] === 'loaded' && m['type'] !== 'embeddings'
        );
        const models: string[] = loaded.map((m) => String(m['id'] ?? '')).filter(Boolean);
        const lanes: NodeLane[] = loaded.map((m) => {
          const id = String(m['id'] ?? '');
          const arch = m['arch'] ? ` · ${String(m['arch'])}` : '';
          return {
            device: deviceFromModelId(id),
            action: `${id}${arch}`,
            status: 'done' as NodeStatus, // resident + loaded = ready to serve
          };
        });
        if (alive) setState({ lanes, models, online: true, loading: false, endpoint: base });
      } catch {
        if (alive) setState({ lanes: [], models: [], online: false, loading: false, endpoint: base });
      }
    };

    void tick();
    const iv = setInterval(() => void tick(), pollMs);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, [pollMs, resolved, enabled]);

  return state;
}

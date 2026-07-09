import { useEffect, useState } from 'react';
import { NodeLane, NodeStatus } from './FanInCard';

/**
 * Live fleet discovery for Goose Local Edition — reads the LM Studio / LM Link endpoint so the swarm
 * fan-in reflects the REAL resident nodes, never hardcoded sample data. This mirrors what the CLI swarm
 * does with `lms ps` (auto-pool from resident models), but from the desktop over LM Studio's native
 * `/api/v0/models` endpoint (returns each model's id, arch, state).
 */

// Use 127.0.0.1, not localhost: the app CSP allows `connect-src http://127.0.0.1:*` but treats
// `localhost` as a different (blocked) origin. Same LM Studio server, either way.
const DEFAULT_ENDPOINT = 'http://127.0.0.1:1234/api/v0/models';

/** Rewrite localhost -> 127.0.0.1 so the fetch is not blocked by the app CSP. */
function cspSafe(url: string): string {
  return url.replace('localhost', '127.0.0.1');
}

export interface FleetState {
  lanes: NodeLane[];
  online: boolean;
  loading: boolean;
  endpoint: string;
}

/** Derive a node/device name from an LM Link model id: the prefix before the first '-' (mihai-, workhorse-, gabee-). */
export function deviceFromModelId(id: string): string {
  const bare = id.split('/').pop() || id; // strip any publisher/ prefix
  const dash = bare.indexOf('-');
  return dash > 0 ? bare.slice(0, dash) : bare;
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
 * `online: false` means the endpoint is unreachable (LM Studio not running) — the caller shows an
 * explicit offline state rather than fabricating data.
 */
export function useFleet(pollMs = 5000, endpoint = DEFAULT_ENDPOINT): FleetState {
  const [state, setState] = useState<FleetState>({
    lanes: [],
    online: false,
    loading: true,
    endpoint,
  });

  useEffect(() => {
    let alive = true;

    const tick = async () => {
      try {
        const res = await fetchWithTimeout(cspSafe(endpoint), 3000);
        const data = (await res.json()) as { data?: Array<Record<string, unknown>> };
        const loaded = (data.data ?? []).filter(
          (m) => m['state'] === 'loaded' && m['type'] !== 'embeddings'
        );
        const lanes: NodeLane[] = loaded.map((m) => {
          const id = String(m['id'] ?? '');
          const arch = m['arch'] ? ` · ${String(m['arch'])}` : '';
          return {
            device: deviceFromModelId(id),
            action: `${id}${arch}`,
            status: 'done' as NodeStatus, // resident + loaded = ready to serve
          };
        });
        if (alive) setState({ lanes, online: true, loading: false, endpoint });
      } catch {
        if (alive) setState({ lanes: [], online: false, loading: false, endpoint });
      }
    };

    void tick();
    const iv = setInterval(() => void tick(), pollMs);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, [pollMs, endpoint]);

  return state;
}

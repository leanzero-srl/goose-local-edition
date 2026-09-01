import type { ExternalGoosedConfig } from './settings';

const DEFAULT_CONNECT_SOURCES = [
  "'self'",
  'http://127.0.0.1:*',
  'https://127.0.0.1:*',
  'ws://127.0.0.1:*',
  'wss://127.0.0.1:*',
  'http://localhost:*',
  'https://localhost:*',
  'ws://localhost:*',
  'wss://localhost:*',
  'https://api.github.com',
  'https://github.com',
  'https://objects.githubusercontent.com',
];

const LOOPBACK_HOSTS: ReadonlySet<string> = new Set(['127.0.0.1', 'localhost', '[::1]', '::1']);

/**
 * Rewrite a `localhost` URL to `127.0.0.1` so a renderer fetch is not blocked by the app's CSP.
 *
 * MEASURED (d28443d90, 2026-07-09; refuted-and-restored 2026-09-02): index.html ships a STATIC meta
 * `Content-Security-Policy … connect-src 'self' http://127.0.0.1:* https: ws: wss:`. Vite copies that
 * meta verbatim into the packaged renderer, which main loads over file:// (`getAppUrl()` →
 * pathToFileURL), and CSP policies INTERSECT — a request must satisfy the meta AND the header this
 * module builds, so nothing the header adds can widen the meta. `http://localhost:1234` is a different
 * origin from `http://127.0.0.1:1234` and the meta blocks it; the same LM Studio server answers on both.
 * Only the hostname is rewritten (a path or query that happens to contain "localhost" is untouched);
 * anything unparseable is returned as-is so the caller's own URL validation is what fails loudly.
 */
export function cspSafe(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.hostname !== 'localhost') return url;
    parsed.hostname = '127.0.0.1';
    return parsed.toString();
  } catch {
    return url;
  }
}

/** The configured swarm host base as an origin, or null when absent/unparseable (nothing is added —
 *  the defaults already cover loopback, which is what an absent endpoint means engine-side). */
function swarmOrigin(swarmEndpoint?: string | null): URL | null {
  if (!swarmEndpoint || !swarmEndpoint.trim()) return null;
  try {
    const url = new URL(swarmEndpoint.trim());
    return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

/**
 * `swarmEndpoint` is `swarm.endpoint` from goose's config.yaml — the LM Studio / LM Link host the ENGINE
 * builds against (U-M3). The renderer probes the same host (useFleet derives every URL from it), and
 * without its origin here a fleet configured on another machine is blocked by connect-src and reads
 * "offline" while the engine is using it.
 */
export function buildConnectSrc(
  externalGoosed?: ExternalGoosedConfig,
  swarmEndpoint?: string | null
): string {
  const sources = [...DEFAULT_CONNECT_SOURCES];

  if (externalGoosed?.enabled && externalGoosed.url) {
    try {
      const externalUrl = new URL(externalGoosed.url);
      sources.push(externalUrl.origin);
      externalUrl.protocol = externalUrl.protocol === 'https:' ? 'wss:' : 'ws:';
      sources.push(externalUrl.origin);
    } catch {
      console.warn('Invalid external goosed URL in settings, skipping CSP entry');
    }
  }

  // Loopback is already covered by the wildcard-port defaults above; only another host needs an entry.
  const swarm = swarmOrigin(swarmEndpoint);
  if (swarm && !LOOPBACK_HOSTS.has(swarm.hostname) && !sources.includes(swarm.origin)) {
    sources.push(swarm.origin);
  }

  return sources.join(' ');
}

/**
 * Returns true when upgrade-insecure-requests should be included in the CSP.
 *
 * The directive is omitted when the user has configured an external backend
 * that uses plain HTTP, because Chromium would silently rewrite those
 * requests to HTTPS. The remote server typically does not speak TLS, so the
 * upgraded requests fail with "Failed to fetch".
 *
 * Loopback addresses (127.0.0.1 / localhost) are exempt from the upgrade
 * per the CSP spec, which is why the built-in local backend is unaffected.
 *
 * The same rule applies to a plain-HTTP swarm endpoint on a non-loopback host
 * (an LM Studio server on the LAN speaks no TLS either).
 */
export function shouldUpgradeInsecureRequests(
  externalGoosed?: ExternalGoosedConfig,
  swarmEndpoint?: string | null
): boolean {
  const swarm = swarmOrigin(swarmEndpoint);
  if (swarm && swarm.protocol === 'http:' && !LOOPBACK_HOSTS.has(swarm.hostname)) {
    return false;
  }

  if (!externalGoosed?.enabled || !externalGoosed.url) {
    return true;
  }

  try {
    const parsed = new URL(externalGoosed.url);
    return parsed.protocol !== 'http:';
  } catch {
    return true;
  }
}

export function buildCSP(externalGoosed?: ExternalGoosedConfig, swarmEndpoint?: string | null): string {
  const connectSrc = buildConnectSrc(externalGoosed, swarmEndpoint);
  const upgradeDirective = shouldUpgradeInsecureRequests(externalGoosed, swarmEndpoint)
    ? 'upgrade-insecure-requests;'
    : '';

  return (
    "default-src 'self';" +
    "style-src 'self' 'unsafe-inline';" +
    "script-src 'self' 'unsafe-inline';" +
    "img-src 'self' data: https:;" +
    `connect-src ${connectSrc};` +
    "object-src 'none';" +
    "frame-src 'self' https: http:;" +
    "font-src 'self' data: https:;" +
    "media-src 'self' mediastream:;" +
    "form-action 'none';" +
    "base-uri 'self';" +
    "manifest-src 'self';" +
    "worker-src 'self';" +
    upgradeDirective
  );
}

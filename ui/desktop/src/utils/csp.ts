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

/**
 * The HEADER policy. It is enforced TOGETHER with index.html's static meta policy (CSP policies
 * intersect), so an origin listed here is reachable only if the meta allows it too: the meta's
 * connect-src is `'self' http://127.0.0.1:* https: ws: wss:`, which is why the `localhost` entries
 * below are inert and why the swarm host is NOT added here — the fleet probes run in main
 * (utils/fleetProbe.ts) instead. 949d3fa6e added the swarm origin here believing the header could
 * widen the meta; gate 8's tracer refuted it (2026-09-02).
 */
export function buildConnectSrc(externalGoosed?: ExternalGoosedConfig): string {
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
 */
export function shouldUpgradeInsecureRequests(externalGoosed?: ExternalGoosedConfig): boolean {
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

export function buildCSP(externalGoosed?: ExternalGoosedConfig): string {
  const connectSrc = buildConnectSrc(externalGoosed);
  const upgradeDirective = shouldUpgradeInsecureRequests(externalGoosed)
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

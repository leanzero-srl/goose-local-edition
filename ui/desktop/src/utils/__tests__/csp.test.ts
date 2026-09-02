import { describe, it, expect } from 'vitest';
import { buildConnectSrc, shouldUpgradeInsecureRequests, buildCSP, cspSafe } from '../csp';
import type { ExternalGoosedConfig } from '../settings';

describe('buildConnectSrc', () => {
  it('includes default sources when no external backend is configured', () => {
    const result = buildConnectSrc(undefined);
    expect(result).toContain("'self'");
    expect(result).toContain('http://127.0.0.1:*');
    expect(result).toContain('wss://127.0.0.1:*');
  });

  it('includes external backend origin when enabled', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    const result = buildConnectSrc(config);
    expect(result).toContain('http://dev.company.net:12604');
    expect(result).toContain('ws://dev.company.net:12604');
  });

  it('includes external secure WebSocket origin for HTTPS backends', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'https://secure.company.net:12604',
      secret: 'test',
    };
    const result = buildConnectSrc(config);
    expect(result).toContain('https://secure.company.net:12604');
    expect(result).toContain('wss://secure.company.net:12604');
  });

  it('does not include external origin when disabled', () => {
    const config: ExternalGoosedConfig = {
      enabled: false,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    const result = buildConnectSrc(config);
    expect(result).not.toContain('dev.company.net');
  });

  it('handles invalid URLs gracefully', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'not-a-valid-url',
      secret: 'test',
    };
    const result = buildConnectSrc(config);
    expect(result).toContain("'self'");
    expect(result).not.toContain('not-a-valid-url');
  });
});

describe('shouldUpgradeInsecureRequests', () => {
  it('returns true when no external backend is configured', () => {
    expect(shouldUpgradeInsecureRequests(undefined)).toBe(true);
  });

  it('returns true when external backend is disabled', () => {
    const config: ExternalGoosedConfig = {
      enabled: false,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    expect(shouldUpgradeInsecureRequests(config)).toBe(true);
  });

  it('returns false when external backend uses HTTP', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    expect(shouldUpgradeInsecureRequests(config)).toBe(false);
  });

  it('returns true when external backend uses HTTPS', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'https://dev.company.net:12604',
      secret: 'test',
    };
    expect(shouldUpgradeInsecureRequests(config)).toBe(true);
  });

  it('returns true for invalid URLs', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'not-a-url',
      secret: 'test',
    };
    expect(shouldUpgradeInsecureRequests(config)).toBe(true);
  });

  it('returns true when URL is empty', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: '',
      secret: 'test',
    };
    expect(shouldUpgradeInsecureRequests(config)).toBe(true);
  });
});

describe('buildCSP', () => {
  it('includes upgrade-insecure-requests with no external backend', () => {
    const csp = buildCSP(undefined);
    expect(csp).toContain('upgrade-insecure-requests');
  });

  it('includes upgrade-insecure-requests with HTTPS external backend', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'https://secure.company.net:12604',
      secret: 'test',
    };
    const csp = buildCSP(config);
    expect(csp).toContain('upgrade-insecure-requests');
    expect(csp).toContain('https://secure.company.net:12604');
  });

  it('excludes upgrade-insecure-requests with HTTP external backend', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    const csp = buildCSP(config);
    expect(csp).not.toContain('upgrade-insecure-requests');
    expect(csp).toContain('http://dev.company.net:12604');
  });

  it('always includes core directives', () => {
    const config: ExternalGoosedConfig = {
      enabled: true,
      url: 'http://dev.company.net:12604',
      secret: 'test',
    };
    const csp = buildCSP(config);
    expect(csp).toContain("default-src 'self'");
    expect(csp).toContain("script-src 'self' 'unsafe-inline'");
    expect(csp).toContain('connect-src');
    expect(csp).toContain("object-src 'none'");
  });
});

/**
 * Loopback regression (gate 8 refutation of 949d3fa6e, 2026-09-02): index.html's STATIC meta allows
 * `http://127.0.0.1:*` and blocks the `localhost` origin, and CSP policies intersect — so a renderer fetch
 * to the live default `http://localhost:1234` reads "offline" whatever the header says. d28443d90 had
 * measured this and rewritten the host; 949d3fa6e deleted the rewrite. Restored here, by URL parsing.
 */
describe('cspSafe — the loopback rewrite the static meta CSP requires', () => {
  it('rewrites a localhost HOST to 127.0.0.1, keeping scheme, port, path and query', () => {
    expect(cspSafe('http://localhost:1234/api/v0/models')).toBe('http://127.0.0.1:1234/api/v0/models');
    expect(cspSafe('http://localhost:1234/v1/chat/completions?x=1')).toBe(
      'http://127.0.0.1:1234/v1/chat/completions?x=1'
    );
    expect(cspSafe('https://localhost:8443/')).toBe('https://127.0.0.1:8443/');
  });

  it('leaves 127.0.0.1 and a LAN host verbatim — the meta blocks a LAN origin regardless', () => {
    expect(cspSafe('http://127.0.0.1:1234/api/v0/models')).toBe('http://127.0.0.1:1234/api/v0/models');
    expect(cspSafe('http://192.168.8.220:1234/api/v0/models')).toBe(
      'http://192.168.8.220:1234/api/v0/models'
    );
  });

  it('touches only the hostname — "localhost" in a path or a subdomain is not the loopback origin', () => {
    expect(cspSafe('http://192.168.8.220:1234/localhost/x')).toBe('http://192.168.8.220:1234/localhost/x');
    expect(cspSafe('http://localhost.example.com:1234/')).toBe('http://localhost.example.com:1234/');
  });

  it('returns an unparseable value unchanged so the caller\'s own validation is what fails', () => {
    expect(cspSafe('localhost:1234')).toBe('localhost:1234');
    expect(cspSafe('')).toBe('');
  });
});

import { describe, it, expect } from 'vitest';
import { buildConnectSrc, shouldUpgradeInsecureRequests, buildCSP } from '../csp';
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
 * U-M3 (branch review, 2026-09-01): the engine builds against `swarm.endpoint`, but connect-src named
 * only loopback + github, so a fleet on another machine was unreachable from the renderer no matter
 * what useFleet probed — and upgrade-insecure-requests would have rewritten an http LAN probe to https.
 */
describe('the swarm endpoint origin', () => {
  it('is added to connect-src, once, as an origin', () => {
    const result = buildConnectSrc(undefined, 'http://192.168.8.220:1234');
    expect(result.split(' ').filter((s) => s === 'http://192.168.8.220:1234')).toHaveLength(1);
  });

  it('adds nothing for loopback, an absent key, or junk — the defaults already cover loopback', () => {
    const base = buildConnectSrc(undefined);
    expect(buildConnectSrc(undefined, 'http://localhost:1234')).toBe(base);
    expect(buildConnectSrc(undefined, undefined)).toBe(base);
    expect(buildConnectSrc(undefined, '')).toBe(base);
    expect(buildConnectSrc(undefined, 'not a url')).toBe(base);
    expect(buildConnectSrc(undefined, 'ftp://x:1')).toBe(base);
  });

  it('skips upgrade-insecure-requests for a plain-http swarm host off loopback, keeps it on loopback', () => {
    expect(shouldUpgradeInsecureRequests(undefined, 'http://192.168.8.220:1234')).toBe(false);
    expect(shouldUpgradeInsecureRequests(undefined, 'http://localhost:1234')).toBe(true);
    expect(shouldUpgradeInsecureRequests(undefined, 'http://127.0.0.1:1234')).toBe(true);
    expect(shouldUpgradeInsecureRequests(undefined, 'https://lms.company.net')).toBe(true);
  });

  it('lands in the full header the same way the external backend does', () => {
    const csp = buildCSP(undefined, 'http://192.168.8.220:1234');
    expect(csp).toContain('http://192.168.8.220:1234');
    expect(csp).not.toContain('upgrade-insecure-requests');
    const loop = buildCSP(undefined, 'http://localhost:1234');
    expect(loop).toContain('upgrade-insecure-requests');
  });
});

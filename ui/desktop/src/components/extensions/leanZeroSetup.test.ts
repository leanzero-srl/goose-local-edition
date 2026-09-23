import { describe, expect, it } from 'vitest';
import { hostedSettings, mergeMcpSettings, validateLocalIntegrations } from './leanZeroSetup';
const entry = {
  name: 'LeanZero Web Search',
  description: '',
  type: 'stdio' as const,
  cmd: '/node',
  args: ['/web.js'],
  envs: {},
  timeout: 300,
};
const saved = {
  name: entry.name,
  type: 'streamable_http' as const,
  uri: 'https://example.org/mcp',
  enabled: true,
  headers: { Authorization: 'Bearer ${OLD}', 'X-Serper-Key': '${SEARCH}', 'X-Output-Dir': 'old' },
  env_keys: ['OLD', 'SEARCH'],
};
describe('LeanZero transport settings', () => {
  it('never forwards credentials to a changed endpoint', () => {
    expect(() => hostedSettings(entry, saved, { ENDPOINT: 'https://another.org/mcp' })).toThrow(
      'token again'
    );
    const result = hostedSettings(entry, saved, {
      ENDPOINT: 'https://another.org/mcp',
      ACCESS_TOKEN: 'new',
    });
    expect(result.config).toMatchObject({
      headers: { Authorization: 'Bearer ${LEANZERO_HOSTED_WEB_ACCESS_TOKEN}' },
    });
    expect(JSON.stringify(result.config)).not.toContain('SEARCH');
    expect(JSON.stringify(result.config)).not.toContain('old');
  });
  it.each([
    'http://example.org/mcp',
    'https://user:pass@example.org/mcp',
    'https://example.org/mcp?token=secret',
  ])('rejects unsafe endpoint %s', (ENDPOINT) => {
    expect(() => hostedSettings(entry, undefined, { ENDPOINT, ACCESS_TOKEN: 'key' })).toThrow();
  });
  it('removes explicitly cleared optional header references', () => {
    const result = hostedSettings(entry, saved, {
      ENDPOINT: saved.uri,
      SERPER_API_KEY: '',
      HOSTED_OUTPUT_DIR: '',
    });
    expect(result.config).toMatchObject({
      headers: { Authorization: 'Bearer ${OLD}' },
      env_keys: ['OLD'],
    });
  });
  it('keeps local and hosted credentials separate and resets transport-only fields', () => {
    const result = mergeMcpSettings(entry, saved, { OUTPUT_DIR: '/research' });
    expect(result.type).toBe('stdio');
    expect(result).not.toHaveProperty('headers');
    expect(result.env_keys).toEqual([]);
  });
  it('does not allow half-configured fact checking or an ambiguous vision endpoint', () => {
    expect(() => validateLocalIntegrations(false, { WEB_SEARCH_BEARER: 'token' })).toThrow(
      'all three'
    );
    expect(() => validateLocalIntegrations(false, { Z_AI_API_KEY: 'key' })).toThrow(
      'vision endpoint'
    );
    expect(() => validateLocalIntegrations(false, {})).not.toThrow();
  });
});

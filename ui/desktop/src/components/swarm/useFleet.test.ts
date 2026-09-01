import { describe, it, expect } from 'vitest';
import { chatCompletionsUrl, deviceFromModelId, modelsUrl } from './useFleet';

/** U-M3: every probe URL is derived from the configured host base — never a pinned 127.0.0.1 — and
 *  (gate 8 refutation of 949d3fa6e) a LOOPBACK base fetches 127.0.0.1, the one loopback origin the static
 *  meta CSP in index.html allows; the display text (`FleetState.endpoint`) stays the configured base. */
describe('probe URLs derive from the configured swarm endpoint (a host base)', () => {
  it('appends the LM Studio routes to the endpoint ORIGIN, whatever path or slash it carries', () => {
    expect(modelsUrl('http://192.168.8.220:1234/')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(modelsUrl('http://192.168.8.220:1234/v1')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(chatCompletionsUrl('http://192.168.8.220:1234')).toBe(
      'http://192.168.8.220:1234/v1/chat/completions'
    );
  });

  it('probes the live default `http://localhost:1234` at 127.0.0.1 — the URL the meta CSP allows', () => {
    expect(modelsUrl('http://localhost:1234')).toBe('http://127.0.0.1:1234/api/v0/models');
    expect(chatCompletionsUrl('http://localhost:1234')).toBe('http://127.0.0.1:1234/v1/chat/completions');
    expect(modelsUrl('http://127.0.0.1:1234')).toBe('http://127.0.0.1:1234/api/v0/models');
  });

  it('refuses to guess a host for an unparseable endpoint', () => {
    expect(() => modelsUrl('localhost:1234')).toThrow();
    expect(() => modelsUrl('')).toThrow();
  });
});

describe('deviceFromModelId', () => {
  it('derives the node name from an LM Link prefixed model id', () => {
    expect(deviceFromModelId('mihai-qwopus3.6-27b-coder-mlx')).toBe('mihai');
    expect(deviceFromModelId('workhorse-qwopus3.6-27b-coder-mlx')).toBe('workhorse');
    expect(deviceFromModelId('gabee-qwopus3.6-27b-coder-mlx')).toBe('gabee');
  });

  it('strips a publisher/ prefix before deriving the node', () => {
    expect(deviceFromModelId('qwen/qwen3.6-27b')).toBe('qwen3.6');
  });

  it('returns the id unchanged when there is no dash', () => {
    expect(deviceFromModelId('llama3')).toBe('llama3');
  });
});

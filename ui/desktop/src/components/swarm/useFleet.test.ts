import { describe, it, expect } from 'vitest';
import { chatCompletionsUrl, deviceFromModelId, modelsUrl } from './useFleet';

/** U-M3: every probe URL is derived from the configured host base — never a pinned 127.0.0.1. */
describe('probe URLs derive from the configured swarm endpoint (a host base)', () => {
  it('appends the LM Studio routes to the endpoint ORIGIN, whatever path or slash it carries', () => {
    expect(modelsUrl('http://localhost:1234')).toBe('http://localhost:1234/api/v0/models');
    expect(modelsUrl('http://192.168.8.220:1234/')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(modelsUrl('http://192.168.8.220:1234/v1')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(chatCompletionsUrl('http://localhost:1234')).toBe('http://localhost:1234/v1/chat/completions');
  });

  it('keeps the configured host verbatim — the settings message and the probe name the SAME host', () => {
    expect(modelsUrl('http://localhost:1234')).not.toContain('127.0.0.1');
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

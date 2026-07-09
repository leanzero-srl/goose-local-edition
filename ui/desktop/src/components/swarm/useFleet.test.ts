import { describe, it, expect } from 'vitest';
import { deviceFromModelId } from './useFleet';

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

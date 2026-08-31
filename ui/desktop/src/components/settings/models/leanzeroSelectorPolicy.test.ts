import { describe, it, expect } from 'vitest';
import {
  isLocalProviderName,
  keepProviderInLeanzeroSelector,
  MLX_PROVIDER_ID,
  MLX_ENTRY_LABEL,
} from './leanzeroSelectorPolicy';

describe('leanzeroSelectorPolicy', () => {
  it('classes every local provider family as local, including the built-in "local"', () => {
    for (const name of [
      'ollama',
      'ollama_cloud',
      'lmstudio',
      'LMStudio',
      'localai',
      'llama-cpp',
      'my-llama-host',
      'swarm',
      'omlx',
      'mlx-sidecar',
      'local',
    ]) {
      expect(isLocalProviderName(name), `${name} should class as local`).toBe(true);
    }
  });

  it('classes cloud providers as not local', () => {
    for (const name of ['anthropic', 'openai', 'google', 'groq', 'openrouter', 'azure_openai']) {
      expect(isLocalProviderName(name), `${name} should class as cloud`).toBe(false);
    }
  });

  it('keeps cloud rows and hides local rows — including omlx, which gets the dedicated entry', () => {
    expect(keepProviderInLeanzeroSelector('anthropic')).toBe(true);
    expect(keepProviderInLeanzeroSelector('openai')).toBe(true);
    expect(keepProviderInLeanzeroSelector('ollama')).toBe(false);
    expect(keepProviderInLeanzeroSelector('lmstudio')).toBe(false);
    expect(keepProviderInLeanzeroSelector('local')).toBe(false);
    expect(keepProviderInLeanzeroSelector(MLX_PROVIDER_ID)).toBe(false);
  });

  it('the engine entry names are the load-bearing constants', () => {
    expect(MLX_PROVIDER_ID).toBe('omlx');
    expect(MLX_ENTRY_LABEL).toBe('Leanzero MLX');
  });
});

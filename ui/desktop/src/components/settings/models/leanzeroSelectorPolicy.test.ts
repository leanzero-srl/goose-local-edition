import { describe, it, expect } from 'vitest';
import {
  isLocalProviderName,
  keepProviderInLocalEdition,
  isLocalEditionCloudProvider,
  legacyProviderMigration,
  MLX_PROVIDER_ID,
  MLX_ENTRY_LABEL,
  SWARM_BUILD_MODEL_ID,
  SWARM_CHAT_MODEL_ID,
} from './leanzeroSelectorPolicy';
describe('leanzeroSelectorPolicy', () => {
  it('admits registry cloud providers without a second fixed catalog', () => {
    for (const id of [
      'aws_bedrock',
      'zai',
      'google',
      'custom_deepseek',
      'anthropic',
      'openai',
      'openrouter',
      'ollama_cloud',
      'custom_new_vendor',
    ]) {
      expect(keepProviderInLocalEdition(id)).toBe(true);
      expect(isLocalEditionCloudProvider(id)).toBe(true);
    }
    expect(keepProviderInLocalEdition('swarm')).toBe(true);
    for (const id of ['swarm', 'omlx', 'lmstudio', 'ollama', 'llama_swap', 'local']) {
      expect(isLocalEditionCloudProvider(id)).toBe(false);
    }
  });

  it('isLocalProviderName stays the edition-derivation fragment test (mainBrand parity)', () => {
    for (const name of [
      'ollama',
      'lmstudio',
      'LMStudio',
      'swarm',
      'omlx',
      'mlx-sidecar',
      'local',
    ]) {
      expect(isLocalProviderName(name), `${name} should class as local`).toBe(true);
    }
    for (const name of ['anthropic', 'openai', 'google', 'zai', 'aws_bedrock', 'custom_deepseek']) {
      expect(isLocalProviderName(name), `${name} should class as cloud`).toBe(false);
    }
  });

  it('legacyProviderMigration: omlx/lmstudio in the local edition -> swarm/swarm; nothing else moves', () => {
    expect(legacyProviderMigration('local', 'omlx')).toEqual({ provider: 'swarm', model: 'swarm' });
    expect(legacyProviderMigration('local', 'lmstudio')).toEqual({
      provider: 'swarm',
      model: 'swarm',
    });
    expect(legacyProviderMigration('standard', 'omlx')).toBeNull();
    expect(legacyProviderMigration('standard', 'lmstudio')).toBeNull();
    expect(legacyProviderMigration('local', 'swarm')).toBeNull();
    expect(legacyProviderMigration('local', 'google')).toBeNull();
    expect(legacyProviderMigration('local', 'ollama')).toBeNull();
    expect(legacyProviderMigration('local', null)).toBeNull();
    expect(legacyProviderMigration('local', undefined)).toBeNull();
  });

  it('the load-bearing constants', () => {
    expect(MLX_PROVIDER_ID).toBe('omlx');
    expect(MLX_ENTRY_LABEL).toBe('Leanzero MLX');
    expect(SWARM_CHAT_MODEL_ID).toBe('swarm');
    expect(SWARM_BUILD_MODEL_ID).toBe('swarm-build');
  });
});

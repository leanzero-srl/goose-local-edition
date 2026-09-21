import { describe, it, expect } from 'vitest';
import {
  CLOUD_PROVIDER_LABELS,
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
  it('offers exactly the fourteen key providers requested', () => {
    expect(Object.keys(CLOUD_PROVIDER_LABELS)).toEqual([
      'aws_bedrock',
      'azure_openai',
      'openai',
      'anthropic',
      'google',
      'alibaba',
      'openrouter',
      'ollama_cloud',
      'minimax',
      'mistral',
      'zai',
      'xai',
      'moonshot',
      'custom_deepseek',
    ]);
    for (const id of Object.keys(CLOUD_PROVIDER_LABELS))
      expect(keepProviderInLocalEdition(id)).toBe(true);
    for (const id of [
      'gemini_oauth',
      'claude_code',
      'chatgpt_codex',
      'cursor_agent',
      'custom_new_vendor',
      'lmstudio',
      'omlx',
    ]) {
      expect(isLocalEditionCloudProvider(id)).toBe(false);
    }
    expect(keepProviderInLocalEdition('swarm')).toBe(true);
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

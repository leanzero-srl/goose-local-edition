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
import {
  CLOUD_PROVIDERS,
  LOCAL_EDITION_PROVIDER_IDS,
} from '../../leanzero-swarm/cloudProviders';

describe('leanzeroSelectorPolicy', () => {
  it('the allow-list is EXACTLY the four swarm cloud registry ids plus swarm, derived from CLOUD_PROVIDERS', () => {
    expect([...LOCAL_EDITION_PROVIDER_IDS].sort()).toEqual(
      ['swarm', 'aws_bedrock', 'zai', 'google', 'custom_deepseek'].sort()
    );
    for (const c of CLOUD_PROVIDERS) {
      expect(LOCAL_EDITION_PROVIDER_IDS).toContain(c.registry);
    }
    expect(LOCAL_EDITION_PROVIDER_IDS).toHaveLength(CLOUD_PROVIDERS.length + 1);
  });

  it('keepProviderInLocalEdition passes exactly the allowed ids', () => {
    for (const id of ['aws_bedrock', 'zai', 'google', 'custom_deepseek', 'swarm']) {
      expect(keepProviderInLocalEdition(id), `${id} must pass`).toBe(true);
    }
    for (const id of [
      'openai',
      'anthropic',
      'omlx',
      'lmstudio',
      'ollama',
      'ollama_cloud',
      'llama_swap',
      'local',
      'openrouter',
      // the CLI family names are NOT registry ids — the join is on registry id only
      'bedrock',
      'deepseek',
    ]) {
      expect(keepProviderInLocalEdition(id), `${id} must fail`).toBe(false);
    }
  });

  it('isLocalEditionCloudProvider is the allow-list minus swarm', () => {
    for (const id of ['aws_bedrock', 'zai', 'google', 'custom_deepseek']) {
      expect(isLocalEditionCloudProvider(id)).toBe(true);
    }
    expect(isLocalEditionCloudProvider('swarm')).toBe(false);
    expect(isLocalEditionCloudProvider('anthropic')).toBe(false);
  });

  it('isLocalProviderName stays the edition-derivation fragment test (mainBrand parity)', () => {
    for (const name of ['ollama', 'lmstudio', 'LMStudio', 'swarm', 'omlx', 'mlx-sidecar', 'local']) {
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

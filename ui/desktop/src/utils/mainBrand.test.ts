import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  getBrandName,
  parseProviderFromConfigYaml,
  resolveBrandName,
  STANDARD_BRAND,
  SWARM_BRAND,
} from './mainBrand';

/**
 * Queued fix #10: the main process (tray tooltip, dialogs, notifications) brands from ONE
 * resolver that mirrors the renderer's EditionContext / the Rust edition.rs precedence:
 * explicit persisted edition first, else derivation from the active provider.
 */
describe('resolveBrandName', () => {
  it('an explicit persisted edition wins over any provider', () => {
    expect(resolveBrandName('local', null)).toBe(SWARM_BRAND);
    expect(resolveBrandName('standard', 'omlx')).toBe(STANDARD_BRAND);
  });

  it('with nothing persisted, a local/swarm provider derives Goose Swarm', () => {
    // this machine's live case: provider omlx, no stored edition
    expect(resolveBrandName(undefined, 'omlx')).toBe(SWARM_BRAND);
    expect(resolveBrandName(undefined, 'lmstudio')).toBe(SWARM_BRAND);
    expect(resolveBrandName(undefined, 'swarm')).toBe(SWARM_BRAND);
    // this fork IS Goose Swarm: a cloud or absent provider with nothing persisted is still Goose Swarm
    expect(resolveBrandName(undefined, 'anthropic')).toBe(SWARM_BRAND);
    expect(resolveBrandName(undefined, null)).toBe(SWARM_BRAND);
  });

  it('an unrecognized persisted value falls through to derivation (never crashes)', () => {
    expect(resolveBrandName('LOCAL-ish', 'ollama')).toBe(SWARM_BRAND);
    expect(resolveBrandName(42, null)).toBe(SWARM_BRAND);
  });
});

describe('parseProviderFromConfigYaml', () => {
  it('reads the flat GOOSE_PROVIDER key, with or without quotes', () => {
    expect(parseProviderFromConfigYaml('GOOSE_PROVIDER: omlx\nGOOSE_MODEL: x\n')).toBe('omlx');
    expect(parseProviderFromConfigYaml('GOOSE_PROVIDER: "lmstudio"\n')).toBe('lmstudio');
    expect(parseProviderFromConfigYaml("GOOSE_PROVIDER: 'ollama' # note\n")).toBe('ollama');
  });

  /** THE LIVE CASE on this machine: the agentic config writes active_provider and no
   *  GOOSE_PROVIDER at all — a GOOSE_PROVIDER-only parse branded the tray plain "Goose". */
  it('falls back to active_provider (the agentic config shape), GOOSE_PROVIDER winning when both exist', () => {
    expect(
      parseProviderFromConfigYaml('providers:\n  omlx:\n    enabled: true\nactive_provider: omlx\n')
    ).toBe('omlx');
    expect(
      parseProviderFromConfigYaml('active_provider: omlx\nGOOSE_PROVIDER: anthropic\n')
    ).toBe('anthropic');
  });

  it('ignores nested/absent keys instead of inventing a provider', () => {
    expect(parseProviderFromConfigYaml('swarm:\n  GOOSE_PROVIDER: nope\n')).toBeNull();
    expect(parseProviderFromConfigYaml('swarm:\n  active_provider: nope\n')).toBeNull();
    expect(parseProviderFromConfigYaml('GOOSE_MODEL: x\n')).toBeNull();
    expect(parseProviderFromConfigYaml('')).toBeNull();
  });
});

describe('getBrandName (file-backed)', () => {
  it('reads settings.json + config.yaml; missing files resolve to the Goose Swarm brand', () => {
    const dir = mkdtempSync(join(tmpdir(), 'mainbrand-'));
    const settingsFile = join(dir, 'settings.json');
    const configFile = join(dir, 'config.yaml');

    // nothing on disk at all -> Goose Swarm (this fork IS Goose Swarm; "Goose" only by explicit choice)
    expect(getBrandName({ settingsFile, configYamlPath: configFile })).toBe(SWARM_BRAND);

    // provider-derived local edition, no explicit setting -> Goose Swarm
    writeFileSync(configFile, 'GOOSE_PROVIDER: omlx\n');
    expect(getBrandName({ settingsFile, configYamlPath: configFile })).toBe(SWARM_BRAND);

    // an explicit standard setting overrides the derivation
    writeFileSync(settingsFile, JSON.stringify({ edition: 'standard' }));
    expect(getBrandName({ settingsFile, configYamlPath: configFile })).toBe(STANDARD_BRAND);

    // and an explicit local setting brands Goose Swarm regardless of provider
    writeFileSync(settingsFile, JSON.stringify({ edition: 'local' }));
    writeFileSync(configFile, 'GOOSE_PROVIDER: anthropic\n');
    expect(getBrandName({ settingsFile, configYamlPath: configFile })).toBe(SWARM_BRAND);
  });
});

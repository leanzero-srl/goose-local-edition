import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { benchmarkProfileDirectory } from '../benchProfile';
import {
  agentWorkRegistryPath,
  gooseConfigYamlPath,
  gooseDirs,
  gooseGlobalMemoryDir,
  gooseMemoryProposalsDir,
  resolveGoosePathRoot,
} from './goosePaths';
import { defaultGooseConfigPath } from './mainBrand';

afterEach(() => {
  vi.unstubAllEnvs();
});

describe('goose paths under GOOSE_PATH_ROOT (Q-183, mirrors crates/goose/src/config/paths.rs)', () => {
  it('hangs every goose dir and every main-process goose file from the root', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'goose-path-root-'));
    vi.stubEnv('GOOSE_PATH_ROOT', root);

    expect(resolveGoosePathRoot()).toBe(root);
    expect(gooseDirs()).toEqual({
      config: path.join(root, 'config'),
      data: path.join(root, 'data'),
      state: path.join(root, 'state'),
    });
    expect(gooseGlobalMemoryDir()).toBe(path.join(root, 'config', 'memory'));
    expect(gooseMemoryProposalsDir()).toBe(path.join(root, 'config', 'proposals'));
    expect(agentWorkRegistryPath()).toBe(path.join(root, 'config', 'agent-work.json'));
    expect(gooseConfigYamlPath()).toBe(path.join(root, 'config', 'config.yaml'));
    expect(defaultGooseConfigPath()).toBe(path.join(root, 'config', 'config.yaml'));
    expect(benchmarkProfileDirectory(os.homedir(), resolveGoosePathRoot())).toBe(
      path.join(root, 'config', 'benchmark')
    );
    for (const p of [gooseGlobalMemoryDir(), agentWorkRegistryPath(), gooseConfigYamlPath()]) {
      expect(p.startsWith(os.homedir() + path.sep + '.config')).toBe(false);
    }
    fs.rmSync(root, { recursive: true, force: true });
  });

  it('trims the root and expands a leading tilde, as main always did', () => {
    vi.stubEnv('GOOSE_PATH_ROOT', '  ~/isolated-profile  ');
    expect(resolveGoosePathRoot()).toBe(path.join(os.homedir(), 'isolated-profile'));
    expect(agentWorkRegistryPath()).toBe(
      path.join(os.homedir(), 'isolated-profile', 'config', 'agent-work.json')
    );
  });

  it('with the root unset (or blank) every path is byte-identical to the pre-Q-183 home paths', () => {
    for (const unset of [undefined, '', '   ']) {
      vi.stubEnv('GOOSE_PATH_ROOT', unset);
      const home = os.homedir();
      expect(resolveGoosePathRoot()).toBeUndefined();
      expect(gooseDirs()).toEqual({
        config: path.join(home, '.config', 'goose'),
        data: path.join(home, '.local', 'share', 'goose'),
        state: path.join(home, '.local', 'state', 'goose'),
      });
      expect(gooseGlobalMemoryDir()).toBe(path.join(home, '.config', 'goose', 'memory'));
      expect(gooseMemoryProposalsDir()).toBe(path.join(home, '.config', 'goose', 'proposals'));
      expect(agentWorkRegistryPath()).toBe(path.join(home, '.config', 'goose', 'agent-work.json'));
      expect(defaultGooseConfigPath()).toBe(path.join(home, '.config', 'goose', 'config.yaml'));
      expect(benchmarkProfileDirectory(home, resolveGoosePathRoot())).toBe(
        path.join(home, '.config', 'goose', 'benchmark')
      );
    }
  });
});

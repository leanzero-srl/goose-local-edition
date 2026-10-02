import { expect, it, vi } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { BENCH_SPEC_FILE, defaultBenchmarkTier } from './benchTierPayload';
import { TIERS, familyOfScorer, TIER_SCORER } from './components/benchmark/baselines';

/** Runs forge.config.ts's mirror against a fake fs; `exists` decides which sources are present. */
function runMirror(exists: (file: string) => boolean = () => true) {
  const source = fs.readFileSync(path.resolve('forge.config.ts'), 'utf8');
  const mirror = source.slice(
    source.indexOf('function mirrorSwarmBenchPayload()'),
    source.indexOf('\nlet cfg')
  );
  const filters: Array<(source: string) => boolean> = [];
  const trees: string[] = [];
  const files: string[] = [];
  const fakeFs = {
    existsSync: exists,
    rmSync: () => {},
    mkdirSync: () => {},
    copyFileSync: (from: string) => files.push(from),
    readdirSync: () => [],
    cpSync: (from: string, _b: string, options: { filter: (source: string) => boolean }) => {
      trees.push(from);
      filters.push(options.filter);
    },
  };
  const copyManifest = Object.assign(vi.fn(), { FORGE_RELEASE: { dir: 'forge' } });
  const requireHelper = vi.fn((id: string) => {
    expect(id).toBe('./scripts/copy-bench-release-manifest.cjs');
    return copyManifest;
  });
  vm.runInNewContext(mirror + ';mirrorSwarmBenchPayload();', {
    fs: fakeFs,
    require: requireHelper,
    join: path.join,
    resolve: path.resolve,
    __dirname: '/fixture/ui/desktop',
  });
  return { filters, trees, files, copyManifest, requireHelper };
}

const SRC = '/fixture/evals/swarm-bench';
const FORGE_TREES = ['public', 'starter', 'kit', 'site'].map((tree) => `${SRC}/forge/${tree}`);

it('every recursive payload copy excludes bytecode and OS caches', () => {
  const { filters, copyManifest, requireHelper } = runMirror();
  expect(requireHelper).toHaveBeenCalledOnce();
  // SB's stable manifest, then the Forge family's — both verified against the shipped bytes.
  expect(copyManifest).toHaveBeenCalledTimes(2);
  expect(copyManifest.mock.calls[1][2]).toEqual({ dir: 'forge' });
  expect(filters).toHaveLength(7);
  for (const filter of filters) {
    for (const file of [
      '/starter/app/__pycache__/x.pyc',
      '/sb8/__pycache__',
      '/sb8/.DS_Store',
      'C:\\starter\\.DS_Store',
      '/starter/module.pyc',
    ])
      expect(filter(file)).toBe(false);
    expect(filter('/starter/app/module.py')).toBe(true);
  }
});

it('ships every tier spec, and both payments tiers starters and visual contracts', () => {
  const { trees, files } = runMirror();
  for (const tier of TIERS)
    if (familyOfScorer(TIER_SCORER[tier]) === 'sb')
      expect(files).toContain(`${SRC}/${BENCH_SPEC_FILE[tier]}`);
    // Forge's public task files ride in the forge/public tree.
    else expect(BENCH_SPEC_FILE[tier].startsWith('forge/public/')).toBe(true);
  expect(files).toContain(`${SRC}/${BENCH_SPEC_FILE[defaultBenchmarkTier()]}`);
  expect(trees).toEqual([
    `${SRC}/sb7.1/starter`,
    `${SRC}/sb7.2/starter`,
    `${SRC}/sb8`,
    ...FORGE_TREES,
  ]);
  expect(files).toContain(`${SRC}/sb7.1/VISUAL-CONTRACT.md`);
  expect(files).toContain(`${SRC}/sb7.2/VISUAL-CONTRACT.md`);
  expect(files).toContain(`${SRC}/sb7.2/SB7-CONTRACT.md`);
  expect(files).toContain(`${SRC}/sb7.2/STARTER.md`);
});

it('leaves SB7.2 starter inputs to the release manifest when the bench payload shares SB7.1s', () => {
  const { trees, files, copyManifest } = runMirror((file) => !file.includes('/sb7.2/'));
  expect(trees).toEqual([`${SRC}/sb7.1/starter`, `${SRC}/sb8`, ...FORGE_TREES]);
  expect(files).not.toContain(`${SRC}/sb7.2/VISUAL-CONTRACT.md`);
  expect(files).not.toContain(`${SRC}/sb7.2/SB7-CONTRACT.md`);
  // The manifest check still runs: a file it lists that was not shipped refuses the package.
  expect(copyManifest).toHaveBeenCalledTimes(2);
});

it('never ships a Forge module tree: the kit is materialised on the user machine, not bundled', () => {
  const { filters, trees } = runMirror();
  const forge = filters.filter((_f, i) => trees[i].includes('/forge/'));
  expect(forge).toHaveLength(4);
  for (const filter of forge) {
    for (const file of [
      '/forge/kit/node_modules/@forge/api/index.js',
      '/forge/kit/app-modules/node_modules',
      '/forge/kit/lint-modules',
      '/forge/kit/test/__pycache__/x.pyc',
    ])
      expect(filter(file)).toBe(false);
    expect(filter('/forge/kit/lib/emulator.cjs')).toBe(true);
    expect(filter('/forge/starter/skills/.gitkeep')).toBe(true);
  }
});

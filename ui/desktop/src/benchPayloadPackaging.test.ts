import { expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
it('both recursive payload copies exclude bytecode and OS caches', () => {
  const source = fs.readFileSync(path.resolve('forge.config.ts'), 'utf8');
  const mirror = source.slice(
    source.indexOf('function mirrorSwarmBenchPayload()'),
    source.indexOf('\nlet cfg')
  );
  const filters: Array<(source: string) => boolean> = [];
  const fakeFs = {
    existsSync: () => true,
    rmSync: () => {},
    mkdirSync: () => {},
    copyFileSync: () => {},
    readdirSync: () => [],
    cpSync: (_a: string, _b: string, options: { filter: (source: string) => boolean }) =>
      filters.push(options.filter),
  };
  vm.runInNewContext(mirror + ';mirrorSwarmBenchPayload();', {
    fs: fakeFs,
    join: path.join,
    resolve: path.resolve,
    __dirname: '/fixture/ui/desktop',
  });
  expect(filters).toHaveLength(2);
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

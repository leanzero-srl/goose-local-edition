import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { inspectBenchmarkRuntime, installBenchmarkRuntime } from './benchRuntimeInstaller';
import { resolveBenchmarkRuntime } from './benchRuntime';

const directories: string[] = [];
afterEach(async () => {
  vi.unstubAllGlobals();
  await Promise.all(
    directories.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true }))
  );
});
async function directory() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'benchmark-install-test-'));
  directories.push(root);
  return root;
}

it('reports missing tools without downloading anything', async () => {
  const fetcher = vi.fn();
  vi.stubGlobal('fetch', fetcher);
  const result = await inspectBenchmarkRuntime(await directory());
  expect(result.state).toBe('missing');
  expect(result.downloadBytes).toBeGreaterThan(0);
  expect(fetcher).not.toHaveBeenCalled();
});

it('rejects corrupted downloads, preserves the previous install and supports retry', async () => {
  const root = await directory();
  await fs.mkdir(path.join(root, 'runtime'));
  await fs.writeFile(path.join(root, 'runtime/previous.txt'), 'working old tools');
  const fetcher = vi
    .fn()
    .mockImplementation(async () => new Response('corrupt archive', { status: 200 }));
  vi.stubGlobal('fetch', fetcher);
  const progress = vi.fn();
  const first = installBenchmarkRuntime(root, progress);
  expect(installBenchmarkRuntime(root, progress)).toBe(first);
  await expect(first).rejects.toThrow('integrity check');
  expect(await fs.readFile(path.join(root, 'runtime/previous.txt'), 'utf8')).toBe(
    'working old tools'
  );
  expect(await fs.readdir(root)).toEqual(['runtime']);
  await expect(installBenchmarkRuntime(root, progress)).rejects.toThrow('integrity check');
  expect(fetcher).toHaveBeenCalledTimes(2);
  expect(progress.mock.calls.some(([value]) => value.phase === 'extracting')).toBe(false);
});

it('cleans an interrupted transfer and never marks it ready', async () => {
  const root = await directory();
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('connection lost')));
  await expect(installBenchmarkRuntime(root, () => {})).rejects.toThrow('connection lost');
  expect(await fs.readdir(root)).toEqual([]);
  expect((await inspectBenchmarkRuntime(root)).state).toBe('missing');
});

it.runIf(process.env.BENCH_RUNTIME_INTEGRATION === '1')(
  'downloads and verifies the real tools in a clean profile without developer PATH',
  async () => {
    const root = await directory();
    const originalPath = process.env.PATH;
    process.env.PATH = '/usr/bin:/bin';
    try {
      const progress: string[] = [];
      await installBenchmarkRuntime(root, (event) => progress.push(event.phase));
      expect(new Set(progress)).toEqual(new Set(['downloading', 'extracting', 'verifying']));
      expect((await inspectBenchmarkRuntime(root)).state).toBe('ready');
      const runtime = await resolveBenchmarkRuntime(root);
      expect(runtime.python.startsWith(root)).toBe(true);
      expect(runtime.env.PATH.endsWith(':/usr/bin:/bin')).toBe(true);
    } finally {
      process.env.PATH = originalPath;
    }
  },
  120000
);

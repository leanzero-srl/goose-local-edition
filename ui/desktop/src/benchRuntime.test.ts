import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { resolveBenchmarkRuntime } from './benchRuntime';
const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => fs.rm(root, { recursive: true, force: true })));
});
async function fixture() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-runtime-'));
  roots.push(root);
  await fs.mkdir(path.join(root, 'runtime'));
  return root;
}
it('refuses a missing installed runtime instead of borrowing a developer Python', async () => {
  await expect(resolveBenchmarkRuntime(await fixture())).rejects.toThrow('manifest is missing');
});
it('validates manifest identity and executable confinement', async () => {
  const root = await fixture();
  const file = path.join(root, 'runtime/manifest.json');
  await fs.writeFile(file, JSON.stringify({ platform: 'linux', arch: 'arm64' }));
  await expect(resolveBenchmarkRuntime(root)).rejects.toThrow('does not match');
  await fs.writeFile(
    file,
    JSON.stringify({ platform: 'darwin', arch: 'arm64', python: '../outside' })
  );
  await expect(resolveBenchmarkRuntime(root)).rejects.toThrow('escapes');
  await fs.writeFile(
    file,
    JSON.stringify({ platform: 'darwin', arch: 'arm64', python: 'missing' })
  );
  await expect(resolveBenchmarkRuntime(root)).rejects.toThrow('not executable');
  await fs.symlink('/bin/sh', path.join(root, 'runtime/missing'));
  await expect(resolveBenchmarkRuntime(root)).rejects.toThrow('not executable');
});
it('runs every installed tool preflight and returns absolute child-process paths', async () => {
  const root = await fixture();
  const dir = path.join(root, 'runtime');
  const manifest = {
    platform: 'darwin',
    arch: 'arm64',
    python: 'python',
    ffmpeg: 'ffmpeg',
    ffprobe: 'ffprobe',
  };
  await fs.writeFile(path.join(dir, 'manifest.json'), JSON.stringify(manifest));
  for (const name of ['python', 'ffmpeg', 'ffprobe']) {
    await fs.writeFile(path.join(dir, name), '#!/bin/sh\nprintf called > "$0.receipt"\n', {
      mode: 0o755,
    });
  }
  const runtime = await resolveBenchmarkRuntime(root);
  expect(runtime.python).toBe(path.join(dir, 'python'));
  expect(runtime.env.BENCH_FFMPEG).toBe(path.join(dir, 'ffmpeg'));
  expect(runtime.env.BENCH_FFPROBE).toBe(path.join(dir, 'ffprobe'));
  expect(runtime.env.PATH.split(path.delimiter)[0]).toBe(dir);
  for (const name of ['python', 'ffmpeg', 'ffprobe'])
    expect(await fs.readFile(path.join(dir, `${name}.receipt`), 'utf8')).toBe('called');
  await fs.writeFile(path.join(dir, 'ffprobe'), '#!/bin/sh\nexit 1\n', { mode: 0o755 });
  await expect(resolveBenchmarkRuntime(root)).rejects.toThrow('preflight failed');
});

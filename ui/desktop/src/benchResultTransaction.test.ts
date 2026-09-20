import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { benchmarkResultTransaction } from './benchResultTransaction';
const roots: string[] = [];
afterEach(async () => {
  for (const root of roots.splice(0)) await fs.rm(root, { recursive: true, force: true });
});
it('restores prior result, session, shots and absent canonical verdict when a later commit step fails', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'retry-transaction-test-'));
  roots.push(root);
  const result = path.join(root, 'result.json'),
    sessions = path.join(root, 'sessions.json'),
    shots = path.join(root, 'shots'),
    canonical = path.join(root, 'verdict.json');
  await fs.writeFile(result, 'original result');
  await fs.writeFile(sessions, 'original session');
  await fs.mkdir(shots);
  await fs.writeFile(path.join(shots, 'original.png'), 'original capture');
  await expect(
    benchmarkResultTransaction([result, sessions, shots, canonical], async () => {
      await fs.writeFile(result, 'new result');
      await fs.writeFile(sessions, 'finished');
      await fs.rm(shots, { recursive: true });
      await fs.mkdir(shots);
      await fs.writeFile(path.join(shots, 'new.png'), 'new capture');
      await fs.writeFile(canonical, 'new verdict');
      throw new Error('canonical copy failed');
    })
  ).rejects.toThrow('canonical copy failed');
  expect(await fs.readFile(result, 'utf8')).toBe('original result');
  expect(await fs.readFile(sessions, 'utf8')).toBe('original session');
  expect(await fs.readdir(shots)).toEqual(['original.png']);
  await expect(fs.stat(canonical)).rejects.toMatchObject({ code: 'ENOENT' });
});
it('retains the new canonical result only when all commit steps succeed', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'retry-transaction-test-'));
  roots.push(root);
  const result = path.join(root, 'result.json');
  await fs.writeFile(result, 'old');
  expect(
    await benchmarkResultTransaction([result], async () => {
      await fs.writeFile(result, 'new');
      return 'saved';
    })
  ).toBe('saved');
  expect(await fs.readFile(result, 'utf8')).toBe('new');
});

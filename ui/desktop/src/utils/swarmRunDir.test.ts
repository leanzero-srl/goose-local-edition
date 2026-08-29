import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { resolveSwarmDir } from './swarmRunDir';

let root: string;

beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'swarm-rundir-'));
});
afterEach(async () => {
  await fs.rm(root, { recursive: true, force: true });
});

const spawnDir = () => path.join(root, 'spawn');

async function breadcrumb(body: string): Promise<void> {
  await fs.mkdir(path.join(spawnDir(), '.swarm'), { recursive: true });
  await fs.writeFile(path.join(spawnDir(), '.swarm', 'current-run.json'), body);
}

describe('resolveSwarmDir — one breadcrumb rule for every main-process consumer', () => {
  it('falls back to the spawn dir when there is no breadcrumb', async () => {
    await fs.mkdir(path.join(spawnDir(), '.swarm'), { recursive: true });
    const r = await resolveSwarmDir(spawnDir());
    expect(r).toEqual({
      swarmDir: path.join(spawnDir(), '.swarm'),
      pinnedRunId: null,
      pinnedRunFile: null,
      hadBreadcrumb: false,
    });
  });

  it('follows the breadcrumb to a redirected build tree and pins its run', async () => {
    const redirected = path.join(root, 'build');
    await fs.mkdir(path.join(redirected, '.swarm'), { recursive: true });
    await breadcrumb(JSON.stringify({ run_id: 'abc123', dir: redirected }));

    const r = await resolveSwarmDir(spawnDir());
    expect(r.swarmDir).toBe(path.join(redirected, '.swarm'));
    expect(r.pinnedRunId).toBe('abc123');
    expect(r.pinnedRunFile).toBe('run-abc123.jsonl');
    expect(r.hadBreadcrumb).toBe(true);
  });

  it('keeps the spawn dir when the redirected dir does not exist, but still pins the run', async () => {
    await breadcrumb(JSON.stringify({ run_id: 'abc123', dir: path.join(root, 'gone') }));
    const r = await resolveSwarmDir(spawnDir());
    expect(r.swarmDir).toBe(path.join(spawnDir(), '.swarm'));
    expect(r.pinnedRunFile).toBe('run-abc123.jsonl');
    expect(r.hadBreadcrumb).toBe(true);
  });

  it('treats a torn mid-write breadcrumb as absent rather than throwing', async () => {
    await breadcrumb('{"run_id": "abc');
    const r = await resolveSwarmDir(spawnDir());
    expect(r.hadBreadcrumb).toBe(false);
    expect(r.swarmDir).toBe(path.join(spawnDir(), '.swarm'));
  });
});

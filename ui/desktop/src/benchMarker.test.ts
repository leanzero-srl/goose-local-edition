import { spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, it } from 'vitest';
import { attachBenchMarker, benchMarkerLabel, benchMarkerPath } from './benchMarker';

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'bench-marker-'));

it('writes {pid, label, started_at} while the runner lives and removes it on close', async () => {
  const dir = tmp();
  const child = spawn(process.execPath, ['-e', 'process.stdin.once("data",()=>process.exit(0))']);
  const file = attachBenchMarker(
    child,
    benchMarkerLabel('Gauntlet', 'build run'),
    dir,
    () => new Date('2026-10-25T15:40:12.000Z')
  );
  expect(file).toBe(benchMarkerPath(dir, child.pid!));
  expect(JSON.parse(fs.readFileSync(file!, 'utf8'))).toEqual({
    pid: child.pid,
    label: 'Gauntlet build run',
    started_at: '2026-10-25T15:40:12.000Z',
  });
  child.stdin!.write('x');
  await once(child, 'close');
  expect(fs.existsSync(file!)).toBe(false);
});

it('writes nothing for a runner that never started', async () => {
  const dir = tmp();
  const child = spawn('/nonexistent/benchmark-runner');
  expect(attachBenchMarker(child, 'Forge rescore', dir)).toBeNull();
  await once(child, 'error');
  expect(fs.readdirSync(dir)).toEqual([]);
});

import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { pickBenchShots, limitBenchShotsForPublish } from './benchShots';

const dirs: string[] = [];
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==',
  'base64'
);
async function fixture(files: string[]) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-shots-test-'));
  dirs.push(dir);
  await fs.mkdir(path.join(dir, 'bench-shots'));
  for (const file of files) await fs.writeFile(path.join(dir, 'bench-shots', file), PNG);
  return dir;
}
afterEach(async () => {
  await Promise.all(dirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

describe('benchmark screenshot evidence', () => {
  it('reads all eight timestamped SB8 full-page stages, with no upload limit or invented repair claim', async () => {
    const stages = ['initial', 'front', 'top', 'iso', 'kinematics', 'lift', 'rotation', 'final'];
    const dir = await fixture(stages.map((stage, i) => `${100 + i}-sb8-${stage}.png`));
    const shots = await pickBenchShots(dir);
    expect(shots.map((shot) => shot.name)).toEqual(stages.map((stage) => `sb8-${stage}`));
    expect(shots.every((shot) => shot.b64 === PNG.toString('base64'))).toBe(true);
    expect(shots.map((shot) => shot.caption).join(' ')).not.toMatch(/repairs/);
    expect(limitBenchShotsForPublish(shots)).toHaveLength(5);
  });

  it('recovers historical canvas camera/gate evidence and prefers timestamped captures', async () => {
    const dir = await fixture([
      'sb8-front.png',
      'sb8-top.png',
      'sb8-iso.png',
      'sb8-gate-100.png',
      '200-sb8-front.png',
    ]);
    const shots = await pickBenchShots(dir);
    expect(shots.find((s) => s.name === 'sb8-front')?.caption).toBe('Front camera');
    expect(shots.find((s) => s.name === 'sb8-top')?.caption).toBe('Top camera · legacy capture');
    expect(shots.find((s) => s.name === 'sb8-gate')?.caption).toContain('gate capture');
  });

  it('retains SB7 first/latest renders plus sync, error, empty and mobile evidence', async () => {
    const dir = await fixture([
      '100-loaded.png',
      '200-loaded.png',
      '201-synced.png',
      '202-error.png',
      '203-empty.png',
      '204-mobile.png',
      'unrelated.png',
    ]);
    expect((await pickBenchShots(dir)).map((s) => s.name)).toEqual([
      'loaded-before',
      'loaded',
      'synced',
      'error',
      'empty',
      'mobile',
    ]);
  });

  it('returns honest absence and keeps publication byte/count limits separate', async () => {
    const dir = await fixture([]);
    expect(await pickBenchShots(dir)).toEqual([]);
    const oversized = {
      name: 'huge',
      caption: 'Huge',
      b64: Buffer.alloc(1.5 * 1024 * 1024).toString('base64'),
    };
    const medium = {
      name: 'medium',
      caption: 'Medium',
      b64: Buffer.alloc(1.3 * 1024 * 1024).toString('base64'),
    };
    expect(limitBenchShotsForPublish([oversized, medium, medium, medium])).toHaveLength(2);
  });
});

it('keeps SB71 field, currency inspection and live update evidence', async () => {
  const names = ['sb71-field', 'sb71-inspect-usd', 'sb71-inspect-xyz', 'sb71-live-update', 'sb71-final-inspector'];
  const dir = await fixture(names.map((name, i) => `${100 + i}-${name}.png`));
  const shots = await pickBenchShots(dir);
  expect(new Set(shots.map((shot) => shot.name))).toEqual(new Set(names));
  expect(shots.find((shot) => shot.name === 'sb71-inspect-xyz')?.caption).toBe('XYZ payment inspection');
  expect(shots.find((shot) => shot.name === 'sb71-live-update')?.caption).toBe('Committed payment update');
});

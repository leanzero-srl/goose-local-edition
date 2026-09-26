import { createRequire } from 'node:module';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, it } from 'vitest';

const { rewriteMachoUuid, uniquifyBundle, nameUuid, formatUuid } = createRequire(import.meta.url)(
  '../scripts/unique-macho-uuid.cjs'
);

const roots: string[] = [];
afterEach(() => roots.splice(0).forEach((root) => rmSync(root, { recursive: true, force: true })));
const scratch = () => {
  const root = mkdtempSync(join(tmpdir(), 'macho-uuid-'));
  roots.push(root);
  return root;
};

const MH_MAGIC_64 = 0xfeedfacf;
const LC_SEGMENT_64 = 0x19;
const LC_UUID = 0x1b;
const CPU_TYPE_ARM64 = 0x0100000c;
const STOCK = Buffer.from('4C4C445055553144A175A5A5EB513DF3', 'hex');

/** A thin arm64 Mach-O header: one 72-byte LC_SEGMENT_64, then LC_UUID. */
function thinMacho(uuid: Buffer): Buffer {
  const buf = Buffer.alloc(32 + 72 + 24 + 16);
  buf.writeUInt32LE(MH_MAGIC_64, 0);
  buf.writeUInt32LE(CPU_TYPE_ARM64, 4);
  buf.writeUInt32LE(2, 16);
  buf.writeUInt32LE(72 + 24, 20);
  buf.writeUInt32LE(LC_SEGMENT_64, 32);
  buf.writeUInt32LE(72, 36);
  buf.writeUInt32LE(LC_UUID, 104);
  buf.writeUInt32LE(24, 108);
  uuid.copy(buf, 112);
  return buf;
}

function fat(slices: Buffer[]): Buffer {
  const headerSize = 8 + slices.length * 20;
  const out: Buffer[] = [Buffer.alloc(headerSize)];
  out[0].writeUInt32BE(0xcafebabe, 0);
  out[0].writeUInt32BE(slices.length, 4);
  let offset = headerSize;
  slices.forEach((slice, i) => {
    out[0].writeUInt32BE(CPU_TYPE_ARM64, 8 + i * 20);
    out[0].writeUInt32BE(offset, 8 + i * 20 + 8);
    out[0].writeUInt32BE(slice.length, 8 + i * 20 + 12);
    out.push(slice);
    offset += slice.length;
  });
  return Buffer.concat(out);
}

it('rewrites the stock Electron LC_UUID to a stable name-based one and nothing else', () => {
  const file = join(scratch(), 'Electron');
  const original = thinMacho(STOCK);
  writeFileSync(file, original);
  const [change] = rewriteMachoUuid(file, 'com.electron.goose:MacOS/Electron');
  expect(change.before).toBe('4C4C4450-5555-3144-A175-A5A5EB513DF3');
  expect(change.after).toBe(formatUuid(nameUuid('com.electron.goose:MacOS/Electron:arm64')));
  // RFC 4122 version 5 and variant bits.
  expect(change.after[14]).toBe('5');
  expect(['8', '9', 'A', 'B']).toContain(change.after[19]);
  const rewritten = readFileSync(file);
  expect(rewritten.subarray(0, 112)).toEqual(original.subarray(0, 112));
  expect(formatUuid(rewritten.subarray(112, 128))).toBe(change.after);
  // Deterministic: the same seed on the next build gives the same UUID.
  writeFileSync(file, original);
  expect(rewriteMachoUuid(file, 'com.electron.goose:MacOS/Electron')[0].after).toBe(change.after);
});

it('rewrites every slice of a universal binary with a per-arch UUID', () => {
  const file = join(scratch(), 'Universal');
  writeFileSync(file, fat([thinMacho(STOCK), thinMacho(STOCK)]));
  const changes = rewriteMachoUuid(file, 'seed');
  expect(changes).toHaveLength(2);
  changes.forEach((c: { before: string }) =>
    expect(c.before).toBe('4C4C4450-5555-3144-A175-A5A5EB513DF3')
  );
});

it('refuses a file that is not a 64-bit Mach-O instead of writing into it', () => {
  const file = join(scratch(), 'script');
  writeFileSync(file, Buffer.from('#!/bin/sh\necho hi\n'.padEnd(64, ' ')));
  expect(() => rewriteMachoUuid(file, 'seed')).toThrow('not a 64-bit Mach-O');
});

it('gives the main executable and each helper of a bundle its own UUID', () => {
  const contents = join(scratch(), 'Goose.app', 'Contents');
  mkdirSync(join(contents, 'MacOS'), { recursive: true });
  mkdirSync(join(contents, 'Resources', 'app'), { recursive: true });
  writeFileSync(join(contents, 'MacOS', 'Electron'), thinMacho(STOCK));
  for (const helper of ['Electron Helper', 'Electron Helper (GPU)']) {
    const dir = join(contents, 'Frameworks', `${helper}.app`, 'Contents', 'MacOS');
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, helper), thinMacho(STOCK));
  }
  const rewritten = uniquifyBundle(join(contents, 'Resources', 'app'), 'com.electron.goose');
  expect(rewritten.map((r: { file: string }) => r.file).sort()).toEqual([
    'Frameworks/Electron Helper (GPU).app/Contents/MacOS/Electron Helper (GPU)',
    'Frameworks/Electron Helper.app/Contents/MacOS/Electron Helper',
    'MacOS/Electron',
  ]);
  const afters = rewritten.map((r: { changes: { after: string }[] }) => r.changes[0].after);
  expect(new Set(afters).size).toBe(3);
});

// Apple's dwarfdump reads the rewritten UUID back. Its xcrun shim builds a cache on first use: on
// GitHub's macOS runners this file took 2.0-3.0 s on green runs and passed the 5 s default on run
// 36239643591, so the test carries its own timeout. Anywhere the tool cannot run, the skip names why.
const DWARFDUMP_TEST_TIMEOUT_MS = 120_000;

it(
  'agrees with dwarfdump on a real Mach-O',
  (ctx) => {
    ctx.skip(
      process.platform !== 'darwin',
      `dwarfdump and /usr/bin/true are macOS tools; this is ${process.platform}`
    );
    let dwarfdump = '';
    let missing = '';
    try {
      dwarfdump = execFileSync('/usr/bin/xcrun', ['--find', 'dwarfdump'], {
        encoding: 'utf8',
      }).trim();
    } catch (err) {
      missing = String(err);
    }
    ctx.skip(missing !== '', `no dwarfdump on this Mac: xcrun --find dwarfdump failed: ${missing}`);
    const file = join(scratch(), 'true');
    copyFileSync('/usr/bin/true', file);
    const [{ after }] = rewriteMachoUuid(file, 'seed').filter(
      (c: { arch: string }) => c.arch === 'arm64'
    );
    const report = execFileSync(dwarfdump, ['--uuid', file], { encoding: 'utf8' });
    expect(report).toContain(`UUID: ${after} (arm64`);
  },
  DWARFDUMP_TEST_TIMEOUT_MS
);

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {
  eventsGeneration,
  readTail,
  readEvents,
  resetSwarmReadCache,
  swarmReadCacheSize,
} from './swarmIncrementalRead';

let dir: string;

beforeEach(async () => {
  resetSwarmReadCache();
  dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'swarm-inc-'));
});
afterEach(async () => {
  await fsp.rm(dir, { recursive: true, force: true });
});

/** What the old code did: read the last `max` bytes, every time, from byte 0 of the range. */
async function fullReadTail(p: string, max: number): Promise<string> {
  const st = await fsp.stat(p);
  const start = Math.max(0, st.size - max);
  const fh = await fsp.open(p, 'r');
  try {
    const buf = Buffer.alloc(Math.min(st.size, max));
    await fh.read(buf, 0, buf.length, start);
    return buf.toString('utf8');
  } finally {
    await fh.close();
  }
}

describe('readTail', () => {
  it('matches a full re-read after every append', async () => {
    const p = path.join(dir, 'task.log');
    await fsp.writeFile(p, '');
    for (let i = 0; i < 40; i++) {
      await fsp.appendFile(p, `chunk ${i} of narration text\n`);
      expect((await readTail(p, 1_000_000))!.text).toBe(await fullReadTail(p, 1_000_000));
    }
  });

  it('matches a full re-read once the tail bound is exceeded', async () => {
    const p = path.join(dir, 'big.log');
    await fsp.writeFile(p, '');
    const MAX = 512;
    for (let i = 0; i < 60; i++) {
      await fsp.appendFile(p, `line ${String(i).padStart(4, '0')} xxxxxxxxxxxxxxxxxxxx\n`);
      const got = await readTail(p, MAX)!;
      expect(got!.text).toBe(await fullReadTail(p, MAX));
      expect(Buffer.byteLength(got!.text, 'utf8')).toBeLessThanOrEqual(MAX);
    }
  });

  it('reports the true file size, not the tail length', async () => {
    const p = path.join(dir, 'sz.log');
    await fsp.writeFile(p, 'x'.repeat(5000));
    const got = await readTail(p, 100);
    expect(got!.size).toBe(5000);
    expect(got!.text.length).toBe(100);
  });

  it('resets instead of splicing when the file shrinks', async () => {
    const p = path.join(dir, 'rot.log');
    await fsp.writeFile(p, 'AAAAAAAAAA');
    expect((await readTail(p, 1000))!.text).toBe('AAAAAAAAAA');
    await fsp.writeFile(p, 'B');
    expect((await readTail(p, 1000))!.text).toBe('B');
  });

  it('returns null for a file that does not exist', async () => {
    expect(await readTail(path.join(dir, 'nope.log'), 100)).toBeNull();
  });

  it('does not corrupt multibyte text split across appends', async () => {
    const p = path.join(dir, 'utf8.log');
    await fsp.writeFile(p, '');
    const text = 'ходят слухи — 日本語のテキスト 🚀🚀🚀';
    const buf = Buffer.from(text, 'utf8');
    for (let i = 0; i < buf.length; i += 3) {
      await fsp.appendFile(p, buf.subarray(i, i + 3));
      await readTail(p, 1_000_000);
    }
    expect((await readTail(p, 1_000_000))!.text).toBe(text);
  });
});

describe('readEvents', () => {
  it('accumulates appended events and matches a full parse', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '');
    const expected: Record<string, unknown>[] = [];
    for (let i = 0; i < 50; i++) {
      const e = { event: 'task_completed', i, ts: `2026-08-29T06:${String(i).padStart(2, '0')}:00Z` };
      expected.push(e);
      await fsp.appendFile(p, JSON.stringify(e) + '\n');
      expect(await readEvents(p)).toEqual(expected);
    }
  });

  it('carries a partial trailing line instead of dropping it', async () => {
    const p = path.join(dir, 'partial.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n{"eve');
    expect(await readEvents(p)).toEqual([{ event: 'a' }]);
    await fsp.appendFile(p, 'nt":"b"}\n');
    expect(await readEvents(p)).toEqual([{ event: 'a' }, { event: 'b' }]);
  });

  it('re-parses from scratch when the log is replaced by a new run', async () => {
    const p = path.join(dir, 'reset.jsonl');
    await fsp.writeFile(p, '{"event":"old1"}\n{"event":"old2"}\n');
    expect(await readEvents(p)).toHaveLength(2);
    await fsp.writeFile(p, '{"event":"new"}\n');
    expect(await readEvents(p)).toEqual([{ event: 'new' }]);
  });

  it('tolerates a line that is never valid JSON', async () => {
    const p = path.join(dir, 'junk.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\nnot json at all\n{"event":"b"}\n');
    expect(await readEvents(p)).toEqual([{ event: 'a' }, { event: 'b' }]);
  });

  it('returns [] for a missing file without throwing', async () => {
    expect(await readEvents(path.join(dir, 'gone.jsonl'))).toEqual([]);
  });

  it('reads no bytes when the file has not changed', async () => {
    const p = path.join(dir, 'stable.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n');
    const first = await readEvents(p);
    const second = await readEvents(p);
    expect(second).toBe(first); // identical array reference proves nothing was re-parsed
  });
});

describe('cache bounds', () => {
  it('evicts least-recently-used paths instead of growing forever', async () => {
    for (let i = 0; i < 80; i++) {
      const p = path.join(dir, `lane-${i}.log`);
      await fsp.writeFile(p, `narration ${i}`);
      await readTail(p, 1000);
    }
    expect(swarmReadCacheSize().tails).toBe(64);
  });

  it('still returns correct text for a path that was evicted', async () => {
    const first = path.join(dir, 'first.log');
    await fsp.writeFile(first, 'ORIGINAL');
    await readTail(first, 1000);
    for (let i = 0; i < 80; i++) {
      const p = path.join(dir, `filler-${i}.log`);
      await fsp.writeFile(p, 'x');
      await readTail(p, 1000);
    }
    await fsp.appendFile(first, '+MORE');
    expect((await readTail(first, 1000))!.text).toBe('ORIGINAL+MORE');
  });

  it('keeps a hot path alive across many other reads', async () => {
    const hot = path.join(dir, 'hot.log');
    await fsp.writeFile(hot, 'H');
    for (let i = 0; i < 80; i++) {
      const p = path.join(dir, `cold-${i}.log`);
      await fsp.writeFile(p, 'c');
      await readTail(p, 1000);
      await readTail(hot, 1000); // touched every round, must never be the LRU victim
    }
    expect(swarmReadCacheSize().tails).toBe(64);
    expect((await readTail(hot, 1000))!.text).toBe('H');
  });
});

describe('same-path replacement — the defect size-only caching could not see', () => {
  it('readTail does not splice a NEW longer file onto the old tail', async () => {
    const p = path.join(dir, 'run.log');
    await fsp.writeFile(p, 'OLDRUN-AAAA');
    expect((await readTail(p, 1_000_000))!.text).toBe('OLDRUN-AAAA');
    await fsp.rm(p);
    await fsp.writeFile(p, 'NEWRUN-BBBBBBBBBBBBBBBB');
    expect((await readTail(p, 1_000_000))!.text).toBe('NEWRUN-BBBBBBBBBBBBBBBB');
  });

  it('readEvents drops the previous run entirely when the log is replaced', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '{"event":"old"}\n');
    expect(await readEvents(p)).toEqual([{ event: 'old' }]);
    await fsp.rm(p);
    await fsp.writeFile(p, '{"event":"n1"}\n{"event":"n2"}\n{"event":"n3"}\n');
    expect(await readEvents(p)).toEqual([
      { event: 'n1' },
      { event: 'n2' },
      { event: 'n3' },
    ]);
  });

  it('a plain append is still treated as an append, not a replacement', async () => {
    const p = path.join(dir, 'append.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n');
    await readEvents(p);
    await fsp.appendFile(p, '{"event":"b"}\n');
    expect(await readEvents(p)).toEqual([{ event: 'a' }, { event: 'b' }]);
  });
});

/**
 * THE GENERATION, which is the renderer's whole cache key.
 *
 * The panel folds this array incrementally and must know whether what it just received is the previous
 * array extended. It cannot answer that from the array itself: IPC structured-clones it, so reference
 * identity is gone, and a content fingerprint reports two same-length arrays that differ in the middle as
 * the same array. The generation is the answer from the only place that has it — it must therefore hold
 * still across every append and move on every rebuild, which is what these prove.
 */
describe('eventsGeneration — the same number for the same accumulation, a new one for a new log', () => {
  it('does not move while the log is only ever appended to', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n');
    await readEvents(p);
    const gen = eventsGeneration(p);
    expect(gen).toBeGreaterThan(0);

    await fsp.appendFile(p, '{"event":"b"}\n');
    await readEvents(p);
    expect(eventsGeneration(p)).toBe(gen);

    // An unchanged file (the common poll) must not move it either.
    await readEvents(p);
    expect(eventsGeneration(p)).toBe(gen);
  });

  it('moves when the log is REPLACED at the same path — the bench harness reuses run.jsonl', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '{"event":"old"}\n');
    await readEvents(p);
    const gen = eventsGeneration(p);

    await fsp.rm(p);
    await fsp.writeFile(p, '{"event":"n1"}\n{"event":"n2"}\n');
    await readEvents(p);
    expect(eventsGeneration(p)).toBeGreaterThan(gen);
  });

  it('moves when the log SHRANK', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n{"event":"b"}\n');
    await readEvents(p);
    const gen = eventsGeneration(p);

    await fsp.truncate(p, 0);
    await fsp.appendFile(p, '{"event":"c"}\n');
    await readEvents(p);
    expect(eventsGeneration(p)).toBeGreaterThan(gen);
  });

  it('never re-serves a number after the cache is cleared', async () => {
    const p = path.join(dir, 'run.jsonl');
    await fsp.writeFile(p, '{"event":"a"}\n');
    await readEvents(p);
    const gen = eventsGeneration(p);
    resetSwarmReadCache();
    expect(eventsGeneration(p)).toBe(0);
    await readEvents(p);
    expect(eventsGeneration(p)).toBeGreaterThan(gen);
  });

  it('gives two different logs two different numbers', async () => {
    const a = path.join(dir, 'run-a.jsonl');
    const b = path.join(dir, 'run-b.jsonl');
    await fsp.writeFile(a, '{"event":"a"}\n');
    await fsp.writeFile(b, '{"event":"b"}\n');
    await readEvents(a);
    await readEvents(b);
    expect(eventsGeneration(a)).not.toBe(eventsGeneration(b));
  });

  it('is 0 for a log that is not there', async () => {
    expect(eventsGeneration(path.join(dir, 'nothing.jsonl'))).toBe(0);
  });
});

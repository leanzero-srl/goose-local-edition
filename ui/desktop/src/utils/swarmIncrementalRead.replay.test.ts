import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { readTail, readEvents, resetSwarmReadCache } from './swarmIncrementalRead';

/**
 * ARCHIVED-TREE REPLAY, applied to the panel's reader.
 *
 * The unit tests next door use synthetic files, which only prove the reader is self-consistent. This
 * replays it over a REAL run the engine actually wrote — its run.jsonl, its per-lane .think.log and
 * .log transcripts — by copying the real bytes into a temp file in growing prefixes, exactly the way
 * the engine appends them, and polling between each append. The result must equal a plain full read of
 * the whole file.
 *
 * This is the harness the campaign skill prescribes: prove a fix in seconds against ~30 stored runs
 * rather than discovering it three hours into a live one.
 */
const ARCHIVE = path.join(
  os.homedir(),
  'Library/Application Support/Goose/benchmark/runs/build'
);

/**
 * A run is only usable for replay if it has BOTH the event log and real transcripts. Picking the first
 * directory with a run.jsonl chose one with an empty activity dir and the transcript half asserted
 * against nothing — a replay harness that silently covers less than it claims is worse than none.
 */
async function pickRun(): Promise<string | null> {
  const dirs = await fsp.readdir(ARCHIVE).catch(() => [] as string[]);
  for (const d of dirs) {
    const p = path.join(ARCHIVE, d);
    const hasLog = await fsp.stat(path.join(p, 'run.jsonl')).then(() => true, () => false);
    if (!hasLog) continue;
    const logs = (await fsp.readdir(path.join(p, '.swarm', 'activity')).catch(() => [] as string[]))
      .filter((f) => f.endsWith('.log'));
    if (logs.length > 0) return p;
  }
  return null;
}

/** Append `src` into `dest` in `steps` growing chunks, polling `read` after every one. */
async function growAndPoll(
  src: string,
  dest: string,
  steps: number,
  poll: (p: string) => Promise<unknown>
): Promise<void> {
  const bytes = await fsp.readFile(src);
  await fsp.writeFile(dest, '');
  const step = Math.max(1, Math.ceil(bytes.length / steps));
  for (let off = 0; off < bytes.length; off += step) {
    await fsp.appendFile(dest, bytes.subarray(off, Math.min(off + step, bytes.length)));
    await poll(dest);
  }
}

describe('replay against a real archived run', () => {
  let tmp: string;
  let run: string | null;

  beforeEach(async () => {
    resetSwarmReadCache();
    tmp = await fsp.mkdtemp(path.join(os.tmpdir(), 'swarm-replay-'));
    run = await pickRun();
  });
  afterEach(async () => {
    await fsp.rm(tmp, { recursive: true, force: true });
  });

  it('readEvents over a real run.jsonl, grown in 25 appends, equals a full parse', async () => {
    if (!run) return; // no archive on this machine — the synthetic suite still covers the logic
    const src = path.join(run, 'run.jsonl');
    const dest = path.join(tmp, 'run.jsonl');
    await growAndPoll(src, dest, 25, (p) => readEvents(p));

    const whole = (await fsp.readFile(src, 'utf8'))
      .split('\n')
      .filter((l) => l.trim())
      .map((l) => {
        try {
          return JSON.parse(l);
        } catch {
          return null;
        }
      })
      .filter((e) => e !== null);

    const got = await readEvents(dest);
    expect(got.length).toBe(whole.length);
    expect(got).toEqual(whole);
    expect(got.length).toBeGreaterThan(0);
  });

  it('readTail over every real transcript, grown in 15 appends, equals the last N bytes', async () => {
    if (!run) return;
    const actDir = path.join(run, '.swarm', 'activity');
    const files = (await fsp.readdir(actDir).catch(() => [] as string[])).filter((f) =>
      f.endsWith('.log')
    );
    expect(files.length).toBeGreaterThan(0);

    for (const f of files) {
      resetSwarmReadCache();
      const src = path.join(actDir, f);
      const dest = path.join(tmp, f);
      const MAX = 4096; // small on purpose, so the tail bound is genuinely exercised
      await growAndPoll(src, dest, 15, (p) => readTail(p, MAX));

      const st = await fsp.stat(src);
      const fh = await fsp.open(src, 'r');
      const buf = Buffer.alloc(Math.min(st.size, MAX));
      await fh.read(buf, 0, buf.length, Math.max(0, st.size - MAX));
      await fh.close();

      const got = await readTail(dest, MAX);
      expect(got!.size, `${f} size`).toBe(st.size);
      expect(got!.text, `${f} tail`).toBe(buf.toString('utf8'));
    }
  });
});

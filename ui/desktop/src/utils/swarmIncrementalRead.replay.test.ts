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
 *
 * HERMETIC AGAINST A LIVE RUN (measured flake, r5): the archive dir is also where the CURRENT
 * benchmark run writes, so a picked source file can GROW between two reads of it. Two rules make the
 * test deterministic anyway:
 *   1. every source file is read into memory ONCE, and both the replayed copy and the expected value
 *      derive from that one snapshot — nothing ever re-stats or re-reads the live file;
 *   2. pickRun orders candidates by heartbeat mtime, STALEST FIRST. The live run is the one whose
 *      heartbeat is being rewritten right now, so it sorts last without any wall-clock threshold —
 *      a dead run's heartbeat froze at its last beat and stays frozen.
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
  const candidates: Array<{ dir: string; beat: number }> = [];
  for (const d of dirs) {
    const p = path.join(ARCHIVE, d);
    const hasLog = await fsp.stat(path.join(p, 'run.jsonl')).then(() => true, () => false);
    if (!hasLog) continue;
    const logs = (await fsp.readdir(path.join(p, '.swarm', 'activity')).catch(() => [] as string[]))
      .filter((f) => f.endsWith('.log'));
    if (logs.length === 0) continue;
    const beat = await fsp
      .stat(path.join(p, 'heartbeat'))
      .then((st) => st.mtimeMs)
      .catch(() => 0);
    candidates.push({ dir: p, beat });
  }
  candidates.sort((a, b) => a.beat - b.beat);
  return candidates[0]?.dir ?? null;
}

/** Append the snapshot into `dest` in `steps` growing chunks, polling `read` after every one. */
async function growAndPoll(
  bytes: Buffer,
  dest: string,
  steps: number,
  poll: (p: string) => Promise<unknown>
): Promise<void> {
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
    // ONE snapshot, cut at the last newline: the reader holds a non-terminated final line back in
    // `rest` (it may still be mid-append), so a snapshot caught mid-line must not count that line
    // on the expectation side either. On a dead run the file ends in '\n' and the cut is a no-op.
    const raw = await fsp.readFile(path.join(run, 'run.jsonl'));
    const nl = raw.lastIndexOf(0x0a);
    const bytes = raw.subarray(0, nl + 1);
    const dest = path.join(tmp, 'run.jsonl');
    await growAndPoll(bytes, dest, 25, (p) => readEvents(p));

    const whole = bytes
      .toString('utf8')
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
      // The snapshot is the whole truth for this file: size and expected tail both come from it, so
      // a source growing under a live run cannot fail the copy it was snapshotted into.
      const bytes = await fsp.readFile(path.join(actDir, f));
      const dest = path.join(tmp, f);
      const MAX = 4096; // small on purpose, so the tail bound is genuinely exercised
      await growAndPoll(bytes, dest, 15, (p) => readTail(p, MAX));

      const expected = bytes.subarray(Math.max(0, bytes.length - MAX));
      const got = await readTail(dest, MAX);
      expect(got!.size, `${f} size`).toBe(bytes.length);
      expect(got!.text, `${f} tail`).toBe(expected.toString('utf8'));
    }
  });
});

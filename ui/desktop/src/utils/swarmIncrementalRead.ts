import { StringDecoder } from 'string_decoder';
import fsp from 'node:fs/promises';

/**
 * Append-only readers for the swarm's run log and per-lane transcripts.
 *
 * `run.jsonl`, `<task>.log` and `<task>.think.log` are append-only, yet the panel re-read and re-parsed
 * all of them from byte 0 twice a second — for a 9-lane run that is 68KB of JSON re-parsed plus up to
 * 600KB of transcript tails re-read per poll, forever. That cost is why the panel feels heavy rather
 * than live, and it is pure waste: the only thing that changed is the bytes past the last offset.
 *
 * Both readers return exactly what a full re-read would have returned. A file that SHRANK (rotated, or a
 * new run reusing the path) resets its cache instead of splicing unrelated bytes onto a stale tail.
 */

/**
 * A cache entry is only valid for the SAME FILE, and size alone cannot establish that.
 *
 * Caching on path+size means a file REPLACED at the same path by a LONGER one reads as an append: the
 * new file's tail gets spliced onto the old file's head. Proven, not theorised -- 'OLDRUN-AAAA' followed
 * by a rewrite to 'NEWRUN-BBBBBBBBBBBBBBBB' returned 'OLDRUN-AAAABBBBBBBBBBBB', and readEvents kept the
 * previous run's events wholesale. That is the panel showing the PREVIOUS run's transcript, and
 * `.swarm/run.jsonl` under the bench harness is exactly a path that gets replaced run over run.
 *
 * Identity is the inode plus the creation time. The inode alone is not enough: filesystems reuse them,
 * and a fresh file landing on a recycled inode would look identical to the one it replaced.
 */
type FileId = { ino: number; birth: number };
type TailEntry = { id: FileId; size: number; buf: Buffer };
type EventsEntry = {
  id: FileId;
  size: number;
  generation: number;
  events: Record<string, unknown>[];
  decoder: StringDecoder;
  rest: string;
};

const sameFile = (a: FileId, b: FileId): boolean => a.ino === b.ino && a.birth === b.birth;

/**
 * THE RENDERER'S CACHE KEY, and the reason it is issued here.
 *
 * `readEvents` hands back a GROWING array: the same log, extended, until the moment it is rebuilt (a new
 * file at the path, or one that shrank). The renderer folds that array incrementally and so must know,
 * with certainty, whether the array it just received is the previous one extended -- but the array is
 * structured-cloned across IPC, so reference identity is gone by the time it arrives, and a content
 * fingerprint answers the question WRONG for two same-length arrays that differ in the middle.
 *
 * Here the answer is free and exact. Every rebuild of an entry takes the next number, so an unchanged
 * generation means an unchanged accumulation. It is global rather than per-path so that two different
 * paths can never collide on a value, and it is never reset: a number that could be re-served after a
 * cache clear would be exactly the lie this exists to prevent.
 */
let generationCounter = 0;

const tails = new Map<string, TailEntry>();
const logs = new Map<string, EventsEntry>();

/**
 * A desktop session outlives many runs, and each run contributes a run log plus two transcripts per
 * lane -- roughly 19 paths, each holding up to 400KB of tail or a whole parsed event array. Caching
 * without eviction would turn a pure I/O win into an unbounded leak after a few runs, so the caches are
 * bounded and evict least-recently-used. A Map preserves insertion order, so re-inserting on every hit
 * makes the oldest key the least recently used one.
 */
const MAX_CACHED_PATHS = 64;

function touch<V>(m: Map<string, V>, k: string, v: V): void {
  m.delete(k);
  m.set(k, v);
  while (m.size > MAX_CACHED_PATHS) {
    const oldest = m.keys().next().value as string | undefined;
    if (oldest === undefined) break;
    m.delete(oldest);
  }
}

/** The generation of the events currently accumulated for `p` -- read straight after `readEvents(p)`. */
export function eventsGeneration(p: string): number {
  return logs.get(p)?.generation ?? 0;
}

export function resetSwarmReadCache(): void {
  tails.clear();
  logs.clear();
}

export function swarmReadCacheSize(): { tails: number; logs: number } {
  return { tails: tails.size, logs: logs.size };
}

async function readRange(p: string, start: number, len: number): Promise<Buffer> {
  const fh = await fsp.open(p, 'r');
  try {
    const buf = Buffer.alloc(len);
    const { bytesRead } = await fh.read(buf, 0, len, start);
    return bytesRead === len ? buf : buf.subarray(0, bytesRead);
  } finally {
    await fh.close();
  }
}

/**
 * The last `max` BYTES of an append-only file, reading only what is new.
 *
 * Byte semantics, not character semantics, deliberately: the previous code read from `size - max`, which
 * can land mid-codepoint, and matching that exactly keeps this a pure I/O change with no behavioural
 * difference for a caller to discover later.
 */
export async function readTail(
  p: string,
  max: number
): Promise<{ text: string; size: number } | null> {
  let size: number;
  let id: FileId;
  try {
    const st = await fsp.stat(p);
    size = st.size;
    id = { ino: st.ino, birth: st.birthtimeMs };
  } catch {
    tails.delete(p);
    return null;
  }
  const cached = tails.get(p);
  const prev = cached && sameFile(cached.id, id) ? cached : undefined;
  if (prev && prev.size === size) {
    touch(tails, p, prev);
    return { text: prev.buf.toString('utf8'), size };
  }

  let buf: Buffer;
  if (!prev || size < prev.size) {
    const start = Math.max(0, size - max);
    buf = await readRange(p, start, Math.min(size, max));
  } else {
    const delta = await readRange(p, prev.size, size - prev.size);
    buf = Buffer.concat([prev.buf, delta]);
    if (buf.length > max) buf = buf.subarray(buf.length - max);
  }
  touch(tails, p, { id, size, buf });
  return { text: buf.toString('utf8'), size };
}

type Events = Record<string, unknown>[];

/**
 * READS OF ONE LOG ARE SERIALISED PER PATH, and this is not belt-and-braces.
 *
 * `readOnce` advances `entry.size` only AFTER its `await readRange(...)`, and carries the engine's
 * half-written trailing line in `entry.rest`. Two calls in flight on the same path therefore both start
 * from the SAME stale offset: every line in the overlap is parsed and pushed TWICE (the panel's feed
 * renders each entry twice), and an overlapped PARTIAL line is concatenated with itself into
 * `{"event":"task_dis{"event":"task_dispatched",...}` — unparseable, swallowed by the catch below, and
 * unrecoverable, because the offset has already moved past those bytes. A dispatched task then never
 * appears in the panel at all, for the life of that generation.
 *
 * The overlap is ordinary, not exotic. `useSwarmRun` polls with `setInterval(() => void tick(), 500)`
 * and never awaits the previous tick, `ipcMain.handle('read-swarm-run')` does not serialise, and the
 * hook is mounted at four sites — the chat and the navigation panel can both be live on one workingDir,
 * which makes the concurrency unconditional rather than a matter of a slow tick.
 *
 * Advancing `entry.size` before the await would not be enough: `rest` and the `StringDecoder` are
 * stateful too, and splitting the delta across two readers desynchronises both. One writer at a time per
 * path is the only thing that keeps the accumulation faithful to the file. Different paths stay
 * concurrent, so the handler's per-lane reads are unaffected.
 *
 * `readTail` needs no such chain: it snapshots `prev` before its await and publishes a WHOLE new entry,
 * so concurrent calls each compute a self-consistent tail and the last write is correct either way.
 */
const inFlight = new Map<string, Promise<Events>>();

/**
 * Every event in an append-only JSONL log, parsing only the lines that are new.
 *
 * A trailing partial line (the engine mid-write) is CARRIED to the next call rather than discarded, so a
 * line split across two polls is parsed once, whole, instead of being dropped as unparseable.
 */
export function readEvents(p: string): Promise<Events> {
  const prev = inFlight.get(p);
  const run = () => readOnce(p);
  // Both handlers, so one caller's failure cannot strand the queue behind a rejected promise.
  const next = prev ? prev.then(run, run) : run();
  inFlight.set(p, next);
  const settle = () => {
    if (inFlight.get(p) === next) inFlight.delete(p);
  };
  void next.then(settle, settle);
  return next;
}

async function readOnce(p: string): Promise<Events> {
  let size: number;
  let id: FileId;
  try {
    const st = await fsp.stat(p);
    size = st.size;
    id = { ino: st.ino, birth: st.birthtimeMs };
  } catch {
    logs.delete(p);
    return [];
  }
  const cached = logs.get(p);
  let entry = cached && sameFile(cached.id, id) ? cached : undefined;
  if (entry && entry.size === size) {
    touch(logs, p, entry);
    return entry.events;
  }
  if (!entry || size < entry.size) {
    entry = {
      id,
      size: 0,
      generation: ++generationCounter,
      events: [],
      decoder: new StringDecoder('utf8'),
      rest: '',
    };
  }
  entry.id = id;
  touch(logs, p, entry);

  const delta = await readRange(p, entry.size, size - entry.size);
  const chunk = entry.rest + entry.decoder.write(delta);
  const lines = chunk.split('\n');
  entry.rest = lines.pop() ?? '';
  for (const line of lines) {
    if (!line.trim()) continue;
    try {
      entry.events.push(JSON.parse(line) as Record<string, unknown>);
    } catch {
      /* a line the engine never finished writing correctly — same tolerance as the full re-read */
    }
  }
  entry.size = size;
  return entry.events;
}

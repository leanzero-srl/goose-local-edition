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

type TailEntry = { size: number; buf: Buffer };
type EventsEntry = { size: number; events: Record<string, unknown>[]; decoder: StringDecoder; rest: string };

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
  try {
    size = (await fsp.stat(p)).size;
  } catch {
    tails.delete(p);
    return null;
  }
  const prev = tails.get(p);
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
  touch(tails, p, { size, buf });
  return { text: buf.toString('utf8'), size };
}

/**
 * Every event in an append-only JSONL log, parsing only the lines that are new.
 *
 * A trailing partial line (the engine mid-write) is CARRIED to the next call rather than discarded, so a
 * line split across two polls is parsed once, whole, instead of being dropped as unparseable.
 */
export async function readEvents(p: string): Promise<Record<string, unknown>[]> {
  let size: number;
  try {
    size = (await fsp.stat(p)).size;
  } catch {
    logs.delete(p);
    return [];
  }
  let entry = logs.get(p);
  if (entry && entry.size === size) {
    touch(logs, p, entry);
    return entry.events;
  }
  if (!entry || size < entry.size) {
    entry = { size: 0, events: [], decoder: new StringDecoder('utf8'), rest: '' };
  }
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

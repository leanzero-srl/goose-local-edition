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

export function resetSwarmReadCache(): void {
  tails.clear();
  logs.clear();
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
  if (prev && prev.size === size) return { text: prev.buf.toString('utf8'), size };

  let buf: Buffer;
  if (!prev || size < prev.size) {
    const start = Math.max(0, size - max);
    buf = await readRange(p, start, Math.min(size, max));
  } else {
    const delta = await readRange(p, prev.size, size - prev.size);
    buf = Buffer.concat([prev.buf, delta]);
    if (buf.length > max) buf = buf.subarray(buf.length - max);
  }
  tails.set(p, { size, buf });
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
  if (entry && entry.size === size) return entry.events;
  if (!entry || size < entry.size) {
    entry = { size: 0, events: [], decoder: new StringDecoder('utf8'), rest: '' };
    logs.set(p, entry);
  }

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

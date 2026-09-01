import { createHash, randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { readdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { KVStore, KvMutation } from "./deps";

// Filesystem-backed KVStore for the self-hosted Node deployment. It mirrors the
// subset of Cloudflare Workers KV that the handlers use: get/put/delete/update plus the
// `expirationTtl` option that gives OTP records and rate-limit windows their expiry.
// One JSON file per key, named sha256(key) as 64 hex chars + ".json": a fixed-length
// name from the hex alphabet, so path traversal is unrepresentable AND the name never
// grows with the key — the previous base64url(key) name blew NAME_MAX (ENAMETOOLONG →
// 500 on request-code) at a 141-char email once the ".<uuid>.tmp" suffix was added.
// There is no email/key length cap here at all; none is derived and none is needed.
// The plaintext key lives INSIDE the record so an operator can still `jq .key`. Every
// operation on a key runs on that key's promise chain, so an `update` (read → mutate →
// write) is atomic with respect to every other operation on the same key within this
// process — the only process that owns the directory.
//
// Workers KV deletes an `expirationTtl` record by itself; here `get` deletes a stale file
// only when that key is READ again, so a file for a never-repeated `rl:ip:*`, `rl:email:*`
// or `otp:*` key would sit in the directory forever (W-L9). `sweepExpired` is the
// housekeeping that Workers KV does implicitly: the Node server runs it at boot and once
// per rate window.

interface StoredRecord {
  key: string;
  value: string;
  expiresAtMs?: number;
}

export interface FsKvOptions {
  now?: () => number;
  log?: (event: string, fields?: Record<string, unknown>) => void;
}

export interface FsKvStore extends KVStore {
  /// Removes every record whose `expiresAtMs` is at or before `now()` — the same
  /// predicate `get` uses to report a record absent — and returns how many files were
  /// removed. A record without a TTL (`nodesecret:*`) is never a candidate. Each removal
  /// runs on the key's own promise chain and re-reads the record there, so an `update`
  /// in flight on the same key is never deleted underneath: if it refreshed the record,
  /// the refreshed record is kept and not counted. `now` is consulted once per record
  /// that can expire, exactly as `get` consults the store clock.
  sweepExpired(now: () => number): Promise<number>;
}

const RECORD_FILE_NAME = /^[0-9a-f]{64}\.json$/;

function isErrno(error: unknown, code: string): boolean {
  return typeof error === "object" && error !== null && (error as { code?: string }).code === code;
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function fileNameFor(key: string): string {
  return `${createHash("sha256").update(key, "utf8").digest("hex")}.json`;
}

function parseRecord(raw: string): StoredRecord {
  const parsed: unknown = JSON.parse(raw);
  if (typeof parsed !== "object" || parsed === null) {
    throw new Error("record is not an object");
  }
  const { key, value, expiresAtMs } = parsed as Record<string, unknown>;
  if (typeof key !== "string" || typeof value !== "string") {
    throw new Error("record is not a { key: string, value: string } object");
  }
  if (expiresAtMs === undefined) {
    return { key, value };
  }
  if (typeof expiresAtMs !== "number" || !Number.isFinite(expiresAtMs)) {
    throw new Error("record expiresAtMs is not a finite number");
  }
  return { key, value, expiresAtMs };
}

function isExpired(record: StoredRecord, now: () => number): boolean {
  return record.expiresAtMs !== undefined && now() >= record.expiresAtMs;
}

export function createFsKvStore(dir: string, options: FsKvOptions = {}): FsKvStore {
  const now = options.now ?? ((): number => Date.now());
  const log = options.log ?? ((event: string, fields?: Record<string, unknown>): void => console.log(JSON.stringify({ event, ...fields })));
  mkdirSync(dir, { recursive: true, mode: 0o700 });

  const pathFor = (key: string): string => join(dir, fileNameFor(key));

  const unlinkQuiet = async (file: string): Promise<void> => {
    try {
      await unlink(file);
    } catch {
      /* already gone or unreadable — nothing to recover */
    }
  };

  // Per-key promise chains: an operation on a key starts only after every earlier
  // operation on that key has settled. Keys never wait on each other.
  const chains = new Map<string, Promise<unknown>>();
  const onKey = <T>(key: string, op: () => Promise<T>): Promise<T> => {
    const previous = chains.get(key) ?? Promise.resolve();
    const run = previous.then(op, op);
    const settled = run.then(
      () => undefined,
      () => undefined,
    );
    chains.set(key, settled);
    void settled.then(() => {
      if (chains.get(key) === settled) {
        chains.delete(key);
      }
    });
    return run;
  };

  const readRecord = async (key: string): Promise<string | null> => {
    const file = pathFor(key);
    let raw: string;
    try {
      raw = await readFile(file, "utf8");
    } catch (error) {
      if (isErrno(error, "ENOENT")) {
        return null;
      }
      log("fs_kv_read_error", { key, error: describe(error) });
      return null;
    }
    let record: StoredRecord;
    try {
      record = parseRecord(raw);
      if (record.key !== key) {
        throw new Error("record key is not the key looked up");
      }
    } catch (error) {
      // A corrupt or partially written file is treated as absent, loudly, and
      // cleaned up best-effort. It must never throw into the handler path.
      log("fs_kv_corrupt", { key, error: describe(error) });
      await unlinkQuiet(file);
      return null;
    }
    if (isExpired(record, now)) {
      await unlinkQuiet(file);
      return null;
    }
    return record.value;
  };

  const writeRecord = async (key: string, value: string, expirationTtl: number | undefined): Promise<void> => {
    const record: StoredRecord = { key, value };
    if (expirationTtl !== undefined) {
      record.expiresAtMs = now() + expirationTtl * 1000;
    }
    const file = pathFor(key);
    const tmp = `${file}.${randomUUID()}.tmp`;
    try {
      await writeFile(tmp, JSON.stringify(record), { mode: 0o600 });
      await rename(tmp, file);
    } catch (error) {
      await unlinkQuiet(tmp);
      throw error;
    }
  };

  const deleteRecord = async (key: string): Promise<void> => {
    try {
      await unlink(pathFor(key));
    } catch (error) {
      if (!isErrno(error, "ENOENT")) {
        log("fs_kv_delete_error", { key, error: describe(error) });
      }
    }
  };

  // The key of the expired record in `name`, or null when the file is gone, unreadable,
  // not a record whose key hashes to this name, has no TTL, or is still live. Corrupt
  // and misplaced files are reported and LEFT — the sweep removes expired records only;
  // `get` on their key is the path that cleans them up.
  const expiredKeyIn = async (name: string, clock: () => number): Promise<string | null> => {
    const file = join(dir, name);
    let raw: string;
    try {
      raw = await readFile(file, "utf8");
    } catch (error) {
      if (!isErrno(error, "ENOENT")) {
        log("fs_kv_read_error", { file: name, error: describe(error) });
      }
      return null;
    }
    let record: StoredRecord;
    try {
      record = parseRecord(raw);
      if (fileNameFor(record.key) !== name) {
        throw new Error("record key does not hash to this file name");
      }
    } catch (error) {
      log("fs_kv_corrupt", { file: name, error: describe(error) });
      return null;
    }
    return isExpired(record, clock) ? record.key : null;
  };

  return {
    get(key: string): Promise<string | null> {
      return onKey(key, () => readRecord(key));
    },

    put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void> {
      return onKey(key, () => writeRecord(key, value, options?.expirationTtl));
    },

    delete(key: string): Promise<void> {
      return onKey(key, () => deleteRecord(key));
    },

    update(key: string, mutate: (current: string | null) => KvMutation): Promise<void> {
      return onKey(key, async () => {
        const decision = mutate(await readRecord(key));
        if (decision === "keep") {
          return;
        }
        if (decision === "delete") {
          await deleteRecord(key);
          return;
        }
        await writeRecord(key, decision.value, decision.expirationTtl);
      });
    },

    async sweepExpired(clock: () => number): Promise<number> {
      const names = (await readdir(dir)).filter((name) => RECORD_FILE_NAME.test(name)).sort();
      let removed = 0;
      for (const name of names) {
        // Read OUTSIDE the chain only to learn the key and whether it is a candidate:
        // a write lands by rename, so this sees a whole record or ENOENT, never a torn
        // one. The decision to unlink is taken again ON the chain, after any update
        // in flight on that key has settled, against what the update left behind.
        const key = await expiredKeyIn(name, clock);
        if (key === null) {
          continue;
        }
        removed += await onKey(key, async (): Promise<number> => {
          if ((await expiredKeyIn(name, clock)) === null) {
            return 0;
          }
          try {
            await unlink(join(dir, name));
            return 1;
          } catch (error) {
            if (!isErrno(error, "ENOENT")) {
              log("fs_kv_delete_error", { key, error: describe(error) });
            }
            return 0;
          }
        });
      }
      return removed;
    },
  };
}

import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { readFile, rename, unlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { KVStore, KvMutation } from "./deps";

// Filesystem-backed KVStore for the self-hosted Node deployment. It mirrors the
// subset of Cloudflare Workers KV that the handlers use: get/put/delete plus the
// `expirationTtl` option that gives OTP records and rate-limit windows their expiry.
// One JSON file per key; the key is base64url-encoded into the filename so it can
// never escape `dir` (path traversal via `../` is unrepresentable) and never needs
// raw interpolation. Every operation on a key runs on that key's promise chain, so an
// `update` (read → mutate → write) is atomic with respect to every other operation on
// the same key within this process — the only process that owns the directory.

interface StoredRecord {
  value: string;
  expiresAtMs?: number;
}

export interface FsKvOptions {
  now?: () => number;
  log?: (event: string, fields?: Record<string, unknown>) => void;
}

function isErrno(error: unknown, code: string): boolean {
  return typeof error === "object" && error !== null && (error as { code?: string }).code === code;
}

function encodeKey(key: string): string {
  return `${Buffer.from(key, "utf8").toString("base64url")}.json`;
}

export function createFsKvStore(dir: string, options: FsKvOptions = {}): KVStore {
  const now = options.now ?? ((): number => Date.now());
  const log = options.log ?? ((event: string, fields?: Record<string, unknown>): void => console.log(JSON.stringify({ event, ...fields })));
  mkdirSync(dir, { recursive: true, mode: 0o700 });

  const pathFor = (key: string): string => join(dir, encodeKey(key));

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
      log("fs_kv_read_error", { key, error: error instanceof Error ? error.message : String(error) });
      return null;
    }
    let record: StoredRecord;
    try {
      const parsed: unknown = JSON.parse(raw);
      if (
        typeof parsed !== "object" ||
        parsed === null ||
        typeof (parsed as Record<string, unknown>).value !== "string"
      ) {
        throw new Error("record is not a { value: string } object");
      }
      record = parsed as StoredRecord;
    } catch (error) {
      // A corrupt or partially written file is treated as absent, loudly, and
      // cleaned up best-effort. It must never throw into the handler path.
      log("fs_kv_corrupt", { key, error: error instanceof Error ? error.message : String(error) });
      await unlinkQuiet(file);
      return null;
    }
    if (record.expiresAtMs !== undefined && now() >= record.expiresAtMs) {
      await unlinkQuiet(file);
      return null;
    }
    return record.value;
  };

  const writeRecord = async (key: string, value: string, expirationTtl: number | undefined): Promise<void> => {
    const record: StoredRecord = { value };
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
        log("fs_kv_delete_error", { key, error: error instanceof Error ? error.message : String(error) });
      }
    }
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
  };
}

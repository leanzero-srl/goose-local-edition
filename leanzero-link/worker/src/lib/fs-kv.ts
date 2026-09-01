import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { readFile, rename, unlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { KVStore } from "./deps";

// Filesystem-backed KVStore for the self-hosted Node deployment. It mirrors the
// subset of Cloudflare Workers KV that the handlers use: get/put/delete plus the
// `expirationTtl` option that gives OTP records and rate-limit windows their expiry.
// One JSON file per key; the key is base64url-encoded into the filename so it can
// never escape `dir` (path traversal via `../` is unrepresentable) and never needs
// raw interpolation.

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

  return {
    async get(key: string): Promise<string | null> {
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
    },

    async put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void> {
      const record: StoredRecord = { value };
      if (options?.expirationTtl !== undefined) {
        record.expiresAtMs = now() + options.expirationTtl * 1000;
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
    },

    async delete(key: string): Promise<void> {
      try {
        await unlink(pathFor(key));
      } catch (error) {
        if (!isErrno(error, "ENOENT")) {
          log("fs_kv_delete_error", { key, error: error instanceof Error ? error.message : String(error) });
        }
      }
    },
  };
}

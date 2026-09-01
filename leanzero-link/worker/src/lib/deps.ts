import type { Config } from "./config";

/// What an atomic read-modify-write decides for the key: write a new value (with the
/// usual TTL semantics), delete it, or leave it untouched.
export type KvMutation = { value: string; expirationTtl?: number } | "delete" | "keep";

export interface KVStore {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
  delete(key: string): Promise<void>;
  /// Read-modify-write of ONE key as a single step: no other get/put/delete/update on the
  /// same key may interleave between the read and the write. `mutate` is synchronous so
  /// the store can hold its per-key lock across it. The OTP attempt counter and the
  /// fixed-window rate limits are built on this — a get→put pair in the handler recorded
  /// 500 concurrent wrong guesses as ONE attempt (measured).
  update(key: string, mutate: (current: string | null) => KvMutation): Promise<void>;
}

export type FetchFn = (url: string, init?: RequestInit) => Promise<Response>;

export interface Deps {
  kv: KVStore;
  fetchFn: FetchFn;
  now: () => number;
  randomOtp: () => string;
  log: (event: string, fields?: Record<string, unknown>) => void;
  config: Config;
}

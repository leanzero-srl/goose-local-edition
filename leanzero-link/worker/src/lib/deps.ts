import type { Config } from "./config";

export interface KVStore {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
  delete(key: string): Promise<void>;
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

import { ACCOUNT_KEY, UUID } from "./crypto";
import { CONTROLLER_SCOPES, type AccountState } from "./types";
import type { DeviceRegistryStore } from "./registry-store";

export class ControllerError extends Error {
  constructor(readonly status: number, readonly code: string, message: string) { super(message); }
}
export function fail(status: number, code: string, message: string): never { throw new ControllerError(status, code, message); }
export function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(400, "invalid_request", "Expected an object");
  return value as Record<string, unknown>;
}
export function string(value: unknown, name: string): string {
  if (typeof value !== "string" || value.length === 0) fail(400, "invalid_request", `${name} is required`);
  return value;
}
export function uuid(value: unknown, name: string): string {
  const result = string(value, name);
  if (!UUID.test(result)) fail(400, "invalid_request", `${name} must be a UUID`);
  return result.toLowerCase();
}
export function integer(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) fail(400, "invalid_request", `${name} must be a nonnegative integer`);
  return value as number;
}
export function splitCredential(value: string, prefix: string): { key: string; id: string } {
  const parts = value.split(".");
  if (parts.length !== 4 || parts[0] !== prefix || !ACCOUNT_KEY.test(parts[1] ?? "") || !UUID.test(parts[2] ?? "") || !/^[A-Za-z0-9_-]{43}$/.test(parts[3] ?? "")) {
    fail(401, "invalid_credential", "Invalid credential");
  }
  return { key: parts[1]!, id: parts[2]! };
}

export function parseAccount(value: string): AccountState {
  // A corrupt account must never be reinitialized, nor grant access through missing flags.
  try {
    const a = object(JSON.parse(value));
    if (a.schema !== 1 || !UUID.test(string(a.account_id, "account_id")) || typeof a.email !== "string") throw new Error();
    const devices = object(a.devices); object(a.challenges); object(a.hosts); object(a.revocations); object(a.rate_windows);
    for (const [id, raw] of Object.entries(devices)) {
      const d = object(raw); const r = object(d.record);
      if (r.device_id !== id || !UUID.test(id) || r.role !== "controller" || !Number.isSafeInteger(r.revision) || (r.revision as number) < 1 || typeof r.name !== "string" || !["ios", "ipados", "desktop"].includes(r.platform as string)) throw new Error();
      if (!(r.revoked_at === null || typeof r.revoked_at === "string") || !Array.isArray(r.scopes) || !r.scopes.every(s => CONTROLLER_SCOPES.includes(s)) || typeof r.created_at !== "string") throw new Error();
      if (typeof d.refresh_hash !== "string" || !ACCOUNT_KEY.test(d.refresh_hash) || !Number.isSafeInteger(d.refresh_expires_at)) throw new Error();
      object(d.used_refreshes);
    }
    return a as unknown as AccountState;
  } catch { throw new Error("corrupt controller account registry"); }
}

export class Registry {
  constructor(readonly store: DeviceRegistryStore) {}
  async read(key: string): Promise<AccountState | null> {
    const row = await this.store.read(key);
    return row === null ? null : parseAccount(row.value);
  }
  async update<T>(key: string, operation: (state: AccountState | null) => Promise<{ state: AccountState; result: T }> | { state: AccountState; result: T }): Promise<T> {
    for (;;) {
      const row = await this.store.read(key);
      const { state, result } = await operation(row === null ? null : parseAccount(row.value));
      if (await this.store.compareAndSwap(key, row?.revision ?? null, JSON.stringify(state))) return result;
    }
  }
}

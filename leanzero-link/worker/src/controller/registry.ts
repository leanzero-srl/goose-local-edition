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
    for (const raw of Object.values(object(a.hosts))) {
      const h = object(raw);
      if (!(h.tls_spki_sha256 === null || (typeof h.tls_spki_sha256 === "string" && ACCOUNT_KEY.test(h.tls_spki_sha256)))) throw new Error();
      if (h.controller_port !== undefined && h.controller_port !== null && (!Number.isSafeInteger(h.controller_port) || (h.controller_port as number) < 1 || (h.controller_port as number) > 65535)) throw new Error();
    }
    if (a.mesh_enrollments !== undefined) {
      for (const [id, raw] of Object.entries(object(a.mesh_enrollments))) {
        const e = object(raw);
        if (!UUID.test(id) || e.enrollment_id !== id || !UUID.test(string(e.subject_id, "subject_id")) || !["host", "controller"].includes(e.role as string) || !["pending", "issued", "confirmed", "revoked"].includes(e.status as string)) throw new Error();
        if (!Number.isSafeInteger(e.created_at) || !Number.isSafeInteger(e.expires_at) || typeof e.cleanup_pending !== "boolean" || !/^[1-9][0-9]*$/.test(string(e.user_id, "user_id"))) throw new Error();
        if (!(e.key_id === null || (typeof e.key_id === "string" && /^[1-9][0-9]*$/.test(e.key_id))) || !(e.receipt === null || typeof e.receipt === "string")) throw new Error();
        if (e.binding !== null) {
          const b = object(e.binding);
          if (!/^[1-9][0-9]*$/.test(string(b.node_id, "node_id")) || !/^nodekey:[0-9a-f]{64}$/.test(string(b.node_key, "node_key")) || b.user_id !== e.user_id || b.key_id !== e.key_id) throw new Error();
        }
        if (e.activated_binding !== undefined) {
          const b = object(e.activated_binding);
          if (e.role !== "host" || !/^[1-9][0-9]*$/.test(string(b.node_id, "node_id")) || !/^nodekey:[0-9a-f]{64}$/.test(string(b.node_key, "node_key")) || !/^[1-9][0-9]*$/.test(string(b.user_id, "user_id")) || !/^[1-9][0-9]*$/.test(string(b.key_id, "key_id"))) throw new Error();
        }
        if (e.status === "confirmed" && e.binding === null) throw new Error();
        if (["issued", "confirmed"].includes(e.status as string) && (e.key_id === null || e.receipt === null)) throw new Error();
      }
    }
    if (a.legacy_mesh_mints !== undefined) {
      const m = object(a.legacy_mesh_mints);
      if (m.reservations !== undefined) for (const [id, raw] of Object.entries(object(m.reservations))) {
        const r = object(raw);
        if (!UUID.test(id) || !Number.isSafeInteger(r.expires_at) || typeof r.external_started !== "boolean" || (r.uncertain !== undefined && (!Number.isSafeInteger(r.uncertain) || (r.uncertain as number) < 0))) throw new Error();
      }
      if (!Number.isSafeInteger(m.in_flight) || (m.in_flight as number) < 0 || !Number.isSafeInteger(m.expires_at)) throw new Error();
    }
    if (a.deletion !== undefined) {
      const d = object(a.deletion);
      if (d.cleanup_lease !== null) {
        const lease = object(d.cleanup_lease);
        if (!UUID.test(string(lease.id, "id")) || !Number.isSafeInteger(lease.expires_at)) throw new Error();
      }
      if (!UUID.test(string(d.request_id, "request_id")) || !UUID.test(string(d.actor_device_id, "actor_device_id")) || !ACCOUNT_KEY.test(string(d.token_hash, "token_hash")) || !["sending", "ready", "failed", "pending", "deleted"].includes(d.status as string) || !Number.isSafeInteger(d.attempts) || !Number.isSafeInteger(d.expires_at) || !Array.isArray(d.pending) || !d.pending.every(p => typeof p === "string") || !Array.isArray(d.retained) || !d.retained.every(r => typeof r === "string")) throw new Error();
    }
    if (a.completed_deletion !== undefined) {
      const d = object(a.completed_deletion);
      if (!UUID.test(string(d.request_id, "request_id")) || !ACCOUNT_KEY.test(string(d.token_hash, "token_hash")) || typeof d.completed_at !== "string" || !Array.isArray(d.retained) || !d.retained.every(r => typeof r === "string")) throw new Error();
    }
    if (a.legacy_revoked_before !== undefined && (!Number.isSafeInteger(a.legacy_revoked_before) || (a.legacy_revoked_before as number) < 0)) throw new Error();
    if (a.legacy_operations !== undefined) {
      for (const [id, raw] of Object.entries(object(a.legacy_operations))) {
        const op = object(raw);
        if (!UUID.test(id) || !Number.isSafeInteger(op.expires_at) || !Number.isSafeInteger(op.external_pending) || (op.external_pending as number) < 0) throw new Error();
      }
    }
    if (a.generation_recoveries !== undefined) object(a.generation_recoveries);
    return a as unknown as AccountState;
  } catch { throw new Error("corrupt controller account registry"); }
}

export function assertAccountActive(state: AccountState | null): void {
  if (state?.deletion && ["pending", "deleted"].includes(state.deletion.status)) fail(401, "account_deleted", "Account deletion has revoked access");
}

export class Registry {
  constructor(readonly store: DeviceRegistryStore) {}
  async read(key: string): Promise<AccountState | null> {
    const row = await this.store.read(key);
    return row === null ? null : parseAccount(row.value);
  }
  async update<T>(key: string, operation: (state: AccountState | null) => Promise<{ state: AccountState; result: T }> | { state: AccountState; result: T }, allowDeletion = false): Promise<T> {
    for (;;) {
      const row = await this.store.read(key);
      const before = row === null ? null : parseAccount(row.value);
      if (!allowDeletion) assertAccountActive(before);
      const { state, result } = await operation(before);
      if (await this.store.compareAndSwap(key, row?.revision ?? null, JSON.stringify(state))) return result;
    }
  }
}

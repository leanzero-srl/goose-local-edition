import { constantTimeEqual } from "../lib/otp";
import { verifyJwt } from "../lib/jwt";
import { AUTH_LEASE_SECONDS, HOST_LEASE_SECONDS, digest, randomSecret, seal, unseal, verifyAccess } from "./crypto";
import { fail, splitCredential } from "./registry";
import { ControllerService, deviceFrom, newAccount } from "./service";
import type { AccountState, AuthorizationLease, HostState, ControllerScope } from "./types";

export interface HostRegistration {
  owner_node_id: string; registration_id: string; host_name: string; protocol_version: 1;
  runtime_id?: string;
  tls_spki_sha256: string | null; controller_port?: number | null;
}
export interface LeaseRequest {
  owner_node_id: string; runtime_id: string; store_generation: string; catalog_revision: number;
  mesh_node_key: string; capabilities?: ControllerScope[];
}

export function checkedHost(state: AccountState | null, id: string, hash: string): HostState {
  const h = state?.hosts[id];
  if (!h || h.suspended || typeof h.credential_hash !== "string" || !constantTimeEqual(h.credential_hash, hash)) fail(401, "invalid_credential", "Invalid host credential");
  if (h.owner_node_id !== id || h.protocol_version !== 1 || !Number.isSafeInteger(h.revision) || !Number.isSafeInteger(h.catalog_high_water) || !(h.generation === null || typeof h.generation === "string") || !(h.lease === null || typeof h.lease === "object")) throw new Error("corrupt host registry");
  return h;
}

export class HostService {
  constructor(private readonly controller: ControllerService) {}

  async bootstrap(credential: string): Promise<{ account_id: string; owner_node_id: string }> {
    const { key, id } = splitCredential(credential, "h1");
    const state = await this.controller.registry.read(key);
    checkedHost(state, id, await digest(credential));
    return { account_id: state!.account_id, owner_node_id: id };
  }

  async register(accountToken: string, existingCredential: string | null, req: HostRegistration): Promise<{ host_credential: string; host_revision: number }> {
    const c = this.controller;
    const verified = await verifyJwt(c.secret, accountToken, Math.floor(c.now() / 1000));
    if (!verified.ok) fail(401, "invalid_credential", "A host account credential is required");
    const key = await digest(verified.claims.sub);
    const existingHash = existingCredential === null ? null : await digest(existingCredential);
    return c.registry.update(key, async state => {
      const account = state ?? newAccount(verified.claims.sub);
      const previous = account.hosts[req.owner_node_id];
      if (previous && !previous.suspended) {
        if (existingHash === null || (previous.registration_pending && !constantTimeEqual(previous.credential_hash, existingHash))) {
          // Recover a lost INITIAL response only for the same account/request and before
          // activation. Once leased, the existing host credential is always required.
          if (previous.registration_id === req.registration_id && (previous.registration_pending || previous.generation === null) && previous.host_name === req.host_name && previous.tls_spki_sha256 === req.tls_spki_sha256 && (previous.controller_port ?? null) === (req.controller_port ?? null)) {
            return { state: account, result: await unseal<{ host_credential: string; host_revision: number }>(c.secret, `${key}.${req.registration_id}`, previous.registration_receipt) };
          }
          fail(409, "owner_already_registered", "Existing owner requires its host credential");
        }
        checkedHost(account, req.owner_node_id, existingHash);
        if (previous.lease && previous.lease.expires_at > c.now() && previous.lease.runtime_id !== req.runtime_id) fail(409, "owner_identity_in_use", "Another runtime owns the active host lease");
        if (previous.registration_id !== req.registration_id) fail(409, "registration_conflict", "Owner registration ID changed");
        if (previous.tls_spki_sha256 !== req.tls_spki_sha256 || previous.host_name !== req.host_name || (previous.controller_port ?? null) !== (req.controller_port ?? null)) {
          previous.tls_spki_sha256 = req.tls_spki_sha256; previous.host_name = req.host_name; previous.controller_port = req.controller_port ?? null;
          previous.revision += 1;
        }
        return { state: account, result: { host_credential: existingCredential!, host_revision: previous.revision } };
      }
      if (previous && previous.registration_id !== req.registration_id) fail(409, "registration_conflict", "Owner registration ID changed");
      const credential = `h1.${key}.${req.owner_node_id}.${randomSecret()}`;
      const result = { host_credential: credential, host_revision: 1 };
      result.host_revision = previous ? previous.revision + 1 : 1;
      account.hosts[req.owner_node_id] = { ...req, registration_pending: true, revision: result.host_revision, credential_hash: await digest(credential), registration_receipt: await seal(c.secret, `${key}.${req.registration_id}`, result), lease: null, generation: previous?.generation ?? null, catalog_high_water: previous?.catalog_high_water ?? 0 };
      return { state: account, result };
    });
  }

  async release(credential: string, runtimeId: string): Promise<{ released: true }> {
    const { key, id } = splitCredential(credential, "h1");
    const hash = await digest(credential); const c = this.controller;
    return c.registry.update(key, state => {
      // Retry of the same authenticated logout is safe after a lost response.
      const old = state?.hosts[id];
      if (old?.suspended && constantTimeEqual(old.credential_hash, hash)) return { state: state!, result: { released: true as const } };
      const h = checkedHost(state, id, hash);
      if (h.lease && h.lease.expires_at > c.now() && h.lease.runtime_id !== runtimeId) fail(409, "owner_identity_in_use", "Another runtime owns the active host lease");
      h.suspended = true; h.lease = null; h.revision += 1;
      for (const e of Object.values(state!.mesh_enrollments ?? {})) if (e.role === "host" && e.subject_id === id) { e.status = "revoked"; e.cleanup_pending = true; }
      return { state: state!, result: { released: true as const } };
    });
  }

  async withdraw(credential: string, runtimeId: string): Promise<{ withdrawn: true }> {
    const { key, id } = splitCredential(credential, "h1"); const hash = await digest(credential); const c = this.controller;
    return c.registry.update(key, state => {
      const h = checkedHost(state, id, hash);
      if (h.lease && h.lease.expires_at > c.now() && h.lease.runtime_id !== runtimeId) fail(409, "owner_identity_in_use", "Another runtime owns the active host lease");
      h.lease = null;
      return { state: state!, result: { withdrawn: true as const } };
    });
  }

  async lease(credential: string, req: LeaseRequest, providerExpiresAt: number | null = null): Promise<{ lease_id: string; lease_expires_at: string; host_revision: number }> {
    const { key, id } = splitCredential(credential, "h1");
    if (id !== req.owner_node_id) fail(403, "wrong_owner", "Credential belongs to another owner");
    const hash = await digest(credential); const c = this.controller;
    return c.registry.update(key, state => {
      const h = checkedHost(state, id, hash);
      if (h.lease && h.lease.expires_at > c.now() && h.lease.runtime_id !== req.runtime_id) fail(409, "owner_identity_in_use", "Another runtime owns the active host lease");
      if (h.generation !== null && h.generation !== req.store_generation) fail(409, "generation_changed", "Store generation changed; explicit owner recovery is required");
      if (req.catalog_revision < h.catalog_high_water) fail(409, "store_rollback", "Catalog is older than the registered high-water mark");
      h.generation = req.store_generation; h.catalog_high_water = req.catalog_revision;
      h.registration_pending = false;
      const same = h.lease?.runtime_id === req.runtime_id && h.lease.expires_at > c.now();
      h.lease = { runtime_id: req.runtime_id, lease_id: same ? h.lease!.lease_id : crypto.randomUUID(), renewed_at: c.now(), expires_at: Math.min(c.now() + HOST_LEASE_SECONDS * 1000, providerExpiresAt ?? Infinity), store_generation: req.store_generation, catalog_revision: req.catalog_revision, mesh_node_key: req.mesh_node_key, capabilities: req.capabilities ?? [] };
      return { state: state!, result: { lease_id: h.lease.lease_id, lease_expires_at: new Date(h.lease.expires_at).toISOString(), host_revision: h.revision } };
    });
  }

  async recover(credential: string, token: string, req: {
    owner_node_id: string; request_id: string; expected_generation: string;
    expected_host_revision: number; expected_catalog_revision: number; new_generation: string;
  }): Promise<{ host_revision: number; store_generation: string }> {
    const { key, id } = splitCredential(credential, "h1");
    if (id !== req.owner_node_id) fail(403, "wrong_owner", "Credential belongs to another owner");
    if (req.new_generation === req.expected_generation) fail(400, "invalid_request", "Recovery must rotate the generation");
    const c = this.controller; const hash = await digest(credential);
    const auth = await c.authenticate(token, "devices.manage");
    if (auth.claims.account_key !== key) fail(403, "wrong_account", "Recovery authorization belongs to another account");
    const requestHash = await digest(JSON.stringify(req));
    return c.registry.update(key, state => {
      const h = checkedHost(state, id, hash);
      const d = deviceFrom(state, auth.claims, c.now());
      if (!d.record.scopes.includes("devices.manage")) fail(403, "scope_denied", "Device management is not allowed");
      const receipts = state!.generation_recoveries ??= {};
      const receiptKey = auth.claims.device_id + "." + req.request_id;
      const previous = receipts[receiptKey];
      if (previous) {
        if (previous.digest !== requestHash) fail(409, "request_id_conflict", "Recovery request changed");
        return { state: state!, result: { host_revision: previous.host_revision, store_generation: previous.store_generation } };
      }
      if (h.generation !== req.expected_generation || h.revision !== req.expected_host_revision || h.catalog_high_water !== req.expected_catalog_revision) fail(409, "recovery_conflict", "Host generation, revision or catalog high-water changed");
      h.generation = req.new_generation; h.catalog_high_water = 0; h.lease = null; h.revision += 1;
      const result = { host_revision: h.revision, store_generation: h.generation };
      receipts[receiptKey] = { digest: requestHash, ...result };
      return { state: state!, result };
    });
  }

  async introspect(credential: string, token: string, ownerId: string, scope: string, runtimeId: string, leaseId: string, providerExpiresAt: number | null = null): Promise<AuthorizationLease | { active: false }> {
    const { key, id } = splitCredential(credential, "h1");
    if (id !== ownerId) fail(403, "wrong_owner", "Credential belongs to another owner");
    const c = this.controller; const hash = await digest(credential);
    const claims = await verifyAccess(c.secret, token, Math.floor(c.now() / 1000));
    const state = await c.registry.read(key);
    const host = checkedHost(state, id, hash);
    if (!host.lease || host.lease.expires_at <= c.now()) fail(403, "host_lease_expired", "Host must hold a current lease");
    if (host.lease.runtime_id !== runtimeId || host.lease.lease_id !== leaseId) fail(403, "host_lease_conflict", "Request does not hold the current host lease");
    if (!host.tls_spki_sha256) fail(503, "host_trust_unconfigured", "Host TLS identity is not registered");
    if (!claims || claims.account_key !== key) return { active: false };
    let d;
    try { d = deviceFrom(state, claims, c.now()); } catch { return { active: false }; }
    if (!d.record.scopes.includes(scope as typeof d.record.scopes[number])) return { active: false };
    const leaseSeconds = Math.min(AUTH_LEASE_SECONDS, claims.exp - Math.ceil(c.now() / 1000), Math.floor((host.lease.expires_at - c.now()) / 1000), providerExpiresAt === null ? Infinity : Math.floor((providerExpiresAt - c.now()) / 1000));
    if (leaseSeconds <= 0) return { active: false };
    return { active: true, account_id: state!.account_id, device_id: d.record.device_id, device_revision: d.record.revision, role: "controller", owner_node_id: id, scopes: [...d.record.scopes], token_expires_at: new Date(claims.exp * 1000).toISOString(), lease_seconds: leaseSeconds, host_revision: host.revision, store_generation: host.lease.store_generation, host_lease_id: host.lease.lease_id };
  }
}

import { constantTimeEqual } from "../lib/otp";
import { verifyJwt } from "../lib/jwt";
import { AUTH_LEASE_SECONDS, HOST_LEASE_SECONDS, digest, randomSecret, seal, unseal, verifyAccess } from "./crypto";
import { fail, splitCredential } from "./registry";
import { ControllerService, deviceFrom, newAccount } from "./service";
import type { AccountState, AuthorizationLease, HostState } from "./types";

export interface HostRegistration {
  owner_node_id: string; registration_id: string; host_name: string; protocol_version: 1;
  tls_spki_sha256: string | null;
}
export interface LeaseRequest {
  owner_node_id: string; runtime_id: string; store_generation: string; catalog_revision: number;
  mesh_node_key: string;
}

function checkedHost(state: AccountState | null, id: string, hash: string): HostState {
  const h = state?.hosts[id];
  if (!h || typeof h.credential_hash !== "string" || !constantTimeEqual(h.credential_hash, hash)) fail(401, "invalid_credential", "Invalid host credential");
  if (h.owner_node_id !== id || h.protocol_version !== 1 || !Number.isSafeInteger(h.revision) || !Number.isSafeInteger(h.catalog_high_water) || !(h.generation === null || typeof h.generation === "string") || !(h.lease === null || typeof h.lease === "object")) throw new Error("corrupt host registry");
  return h;
}

export class HostService {
  constructor(private readonly controller: ControllerService) {}

  async register(accountToken: string, existingCredential: string | null, req: HostRegistration): Promise<{ host_credential: string; host_revision: number }> {
    const c = this.controller;
    const verified = await verifyJwt(c.secret, accountToken, Math.floor(c.now() / 1000));
    if (!verified.ok) fail(401, "invalid_credential", "A host account credential is required");
    const key = await digest(verified.claims.sub);
    const existingHash = existingCredential === null ? null : await digest(existingCredential);
    return c.registry.update(key, async state => {
      const account = state ?? newAccount(verified.claims.sub);
      const previous = account.hosts[req.owner_node_id];
      if (previous) {
        if (existingHash === null) {
          // Recover a lost INITIAL response only for the same account/request and before
          // activation. Once leased, the existing host credential is always required.
          if (previous.registration_id === req.registration_id && previous.generation === null && previous.host_name === req.host_name && previous.tls_spki_sha256 === req.tls_spki_sha256) {
            return { state: account, result: await unseal<{ host_credential: string; host_revision: number }>(c.secret, `${key}.${req.registration_id}`, previous.registration_receipt) };
          }
          fail(409, "owner_already_registered", "Existing owner requires its host credential");
        }
        checkedHost(account, req.owner_node_id, existingHash);
        if (previous.registration_id !== req.registration_id) fail(409, "registration_conflict", "Owner registration ID changed");
        if (previous.tls_spki_sha256 !== req.tls_spki_sha256 || previous.host_name !== req.host_name) {
          previous.tls_spki_sha256 = req.tls_spki_sha256; previous.host_name = req.host_name;
          previous.revision += 1;
        }
        return { state: account, result: { host_credential: existingCredential!, host_revision: previous.revision } };
      }
      const credential = `h1.${key}.${req.owner_node_id}.${randomSecret()}`;
      const result = { host_credential: credential, host_revision: 1 };
      account.hosts[req.owner_node_id] = { ...req, revision: 1, credential_hash: await digest(credential), registration_receipt: await seal(c.secret, `${key}.${req.registration_id}`, result), lease: null, generation: null, catalog_high_water: 0 };
      return { state: account, result };
    });
  }

  async lease(credential: string, req: LeaseRequest): Promise<{ lease_id: string; lease_expires_at: string; host_revision: number }> {
    const { key, id } = splitCredential(credential, "h1");
    if (id !== req.owner_node_id) fail(403, "wrong_owner", "Credential belongs to another owner");
    const hash = await digest(credential); const c = this.controller;
    return c.registry.update(key, state => {
      const h = checkedHost(state, id, hash);
      if (h.lease && h.lease.expires_at > c.now() && h.lease.runtime_id !== req.runtime_id) fail(409, "owner_identity_in_use", "Another runtime owns the active host lease");
      if (h.generation !== null && h.generation !== req.store_generation) fail(409, "generation_changed", "Store generation changed; explicit owner recovery is required");
      if (req.catalog_revision < h.catalog_high_water) fail(409, "store_rollback", "Catalog is older than the registered high-water mark");
      h.generation = req.store_generation; h.catalog_high_water = req.catalog_revision;
      const same = h.lease?.runtime_id === req.runtime_id && h.lease.expires_at > c.now();
      h.lease = { runtime_id: req.runtime_id, lease_id: same ? h.lease!.lease_id : crypto.randomUUID(), expires_at: c.now() + HOST_LEASE_SECONDS * 1000, store_generation: req.store_generation, catalog_revision: req.catalog_revision, mesh_node_key: req.mesh_node_key };
      return { state: state!, result: { lease_id: h.lease.lease_id, lease_expires_at: new Date(h.lease.expires_at).toISOString(), host_revision: h.revision } };
    });
  }

  async introspect(credential: string, token: string, ownerId: string, scope: string, runtimeId: string, leaseId: string): Promise<AuthorizationLease | { active: false }> {
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
    const leaseSeconds = Math.min(AUTH_LEASE_SECONDS, claims.exp - Math.ceil(c.now() / 1000), Math.floor((host.lease.expires_at - c.now()) / 1000));
    if (leaseSeconds <= 0) return { active: false };
    return { active: true, account_id: state!.account_id, device_id: d.record.device_id, device_revision: d.record.revision, role: "controller", owner_node_id: id, scopes: [...d.record.scopes], token_expires_at: new Date(claims.exp * 1000).toISOString(), lease_seconds: leaseSeconds, host_revision: host.revision, store_generation: host.lease.store_generation, host_lease_id: host.lease.lease_id };
  }
}

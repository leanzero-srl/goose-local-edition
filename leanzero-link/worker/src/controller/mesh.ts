import type { Deps } from "../lib/deps";
import { AUTH_LEASE_SECONDS, digest, seal, unseal } from "./crypto";
import { checkedHost } from "./hosts";
import { fail, parseAccount, splitCredential } from "./registry";
import { ControllerService, deviceFrom } from "./service";
import { MeshProvider, usable, type ProviderNode } from "./mesh-provider";
import { type AccessClaims, type AccountState, type MeshEnrollment, type MeshJoinCredential, type RegisteredPeers } from "./types";

type Role = "host" | "controller";
interface Actor { key: string; id: string; role: Role; hash?: string; claims?: AccessClaims }
function enrollments(state: AccountState): MeshEnrollment[] { return Object.values(state.mesh_enrollments ?? {}); }
function live(e: MeshEnrollment, state: AccountState, now: number): boolean {
  if (e.status === "revoked") return false;
  if (e.role === "controller") {
    const d = state.devices[e.subject_id];
    if (!d || d.record.revoked_at !== null || d.refresh_expires_at <= now) return false;
  }
  return e.status === "confirmed" || e.expires_at > now;
}
function matches(e: MeshEnrollment, node: ProviderNode): boolean {
  const b = e.binding;
  return b !== null && b.node_id === node.id && b.node_key === node.node_key && b.user_id === node.user_id && b.key_id === node.key_id;
}
function activatedHost(state: AccountState, e: MeshEnrollment, n: ProviderNode): boolean {
  const h = state.hosts[e.subject_id]; const activated = e.activated_binding;
  return e.role === "host" && e.status === "confirmed" && matches(e, n) && !!h?.controller_port && !!h.tls_spki_sha256 && !h.suspended && !h.registration_pending && activated !== undefined && activated.node_id === n.id && activated.node_key === n.node_key && activated.user_id === n.user_id && activated.key_id === n.key_id;
}
export class MeshService {
  readonly provider: MeshProvider;
  constructor(private readonly c: ControllerService, private readonly deps: Deps) { this.provider = new MeshProvider(deps); }
  private check(actor: Actor, state: AccountState | null): AccountState {
    if (actor.role === "host") checkedHost(state, actor.id, actor.hash!);
    else {
      const device = deviceFrom(state, actor.claims!, this.c.now());
      if (device.record.platform === "desktop") fail(403, "desktop_mesh_denied", "Desktop controllers use their existing host mesh");
    }
    return state!;
  }
  private async actor(token: string, role: Role): Promise<Actor> {
    if (role === "host") {
      const { key, id } = splitCredential(token, "h1");
      const actor: Actor = { key, id, role, hash: await digest(token) };
      this.check(actor, await this.c.registry.read(key)); return actor;
    }
    const a = await this.c.authenticate(token);
    if (a.device.record.platform === "desktop") fail(403, "desktop_mesh_denied", "Desktop controllers use their existing host mesh");
    return { key: a.claims.account_key, id: a.claims.device_id, role, claims: a.claims };
  }
  private compatible(state: AccountState, nodes: ProviderNode[]): void {
    const es = enrollments(state);
    for (const n of nodes.filter(n => usable(n, this.c.now()))) {
      const e = es.find(e => live(e, state, this.c.now()) && matches(e, n));
      if (!e) fail(409, "update_required", "Update Goose Swarm on the Macs linked to this account, reconnect LeanZero Link on each Mac, then try again.");
      if (e.role === "host" && !activatedHost(state, e, n)) fail(409, "update_required", "Update Goose Swarm on the Macs linked to this account, reconnect LeanZero Link on each Mac, then try again.");
    }
    if (state.legacy_mesh_mints && (state.legacy_mesh_mints.in_flight > 0 || state.legacy_mesh_mints.expires_at > this.c.now())) fail(409, "update_required", "Legacy host enrollment credentials must expire first");
  }
  async join(token: string, role: Role, enrollmentId: string): Promise<MeshJoinCredential> {
    const actor = await this.actor(token, role);
    let state = this.check(actor, await this.c.registry.read(actor.key));
    const snapshot = await this.provider.nodes(state.email);
    if (role === "controller") {
      this.compatible(state, snapshot.nodes);
      await this.provider.assertNoUntrackedKeys(snapshot.user.id, new Set(enrollments(state).map(e => e.key_id).filter((x): x is string => x !== null)));
    }
    // A CAS reservation owns the external side effect. Never mint inside the retrying
    // transaction. An interrupted/ambiguous mint is loud pending, never a duplicate key.
    const reservation = await this.c.registry.update(actor.key, s => {
      state = this.check(actor, s);
      if (role === "controller") this.compatible(state, snapshot.nodes);
      const all = state.mesh_enrollments ??= {};
      const old = all[enrollmentId];
      if (old) {
        if (old.role !== role || old.subject_id !== actor.id) fail(409, "enrollment_conflict", "Enrollment ID belongs to another identity");
        if (old.status === "revoked" || old.expires_at <= this.c.now()) fail(409, "enrollment_expired", "Enrollment expired or was revoked");
        if (old.status === "pending") fail(409, "enrollment_pending", "Provider enrollment outcome is pending; do not remint");
        return { state, result: { existing: old.receipt, created: false } };
      }
      const previous = enrollments(state).find(e => e.role === role && e.subject_id === actor.id && live(e, state, this.c.now()));
      if (previous) {
        // A new enrollment ID explicitly reconnects an identity whose provider node
        // has expired or disappeared from the complete account listing. Never infer
        // takeover from a changed public key on a still-live provider node.
        const stillPresent = previous.binding && snapshot.nodes.some(n => n.id === previous.binding!.node_id && usable(n, this.c.now()));
        if (previous.status !== "confirmed" || stillPresent) fail(409, "already_enrolled", "This identity already has an active mesh enrollment");
        previous.status = "revoked";
      }
      if (role === "host") {
        const h = state.hosts[actor.id]!;
        if (!h.controller_port || !h.tls_spki_sha256) fail(409, "update_required", "Host must register its TLS pin and controller port");
      }
      all[enrollmentId] = { enrollment_id: enrollmentId, role, subject_id: actor.id, status: "pending", created_at: this.c.now(), expires_at: this.c.now() + this.provider.keySeconds * 1000, user_id: snapshot.user.id, key_id: null, receipt: null, binding: null, cleanup_pending: false };
      return { state, result: { existing: null, created: true } };
    });
    if (!reservation.created) {
      if (!reservation.existing) fail(503, "enrollment_corrupt", "Enrollment receipt is missing");
      return unseal(this.c.secret, actor.key + "." + enrollmentId, reservation.existing);
    }
    const pending = (await this.c.registry.read(actor.key))!.mesh_enrollments![enrollmentId]!;
    const key = await this.provider.mint(snapshot.user.id, pending.expires_at);
    const result: MeshJoinCredential = { enrollment_id: enrollmentId, auth_key: key.key, login_server: this.provider.loginServer, expiry_seconds: Math.floor((key.expires_at - this.c.now()) / 1000) };
    const receipt = await seal(this.c.secret, actor.key + "." + enrollmentId, result);
    // Persist key identity even when the device was revoked while the provider minted.
    const allowed = await this.c.registry.update(actor.key, s => {
      if (!s) throw new Error("missing mesh reservation");
      const e = s.mesh_enrollments?.[enrollmentId];
      if (!e || !["pending", "revoked"].includes(e.status)) throw new Error("mesh reservation changed");
      e.key_id = key.id; e.receipt = receipt; e.expires_at = key.expires_at;
      let authorized = e.status !== "revoked";
      try { this.check(actor, s); } catch { authorized = false; }
      e.status = authorized ? "issued" : "revoked"; e.cleanup_pending = !authorized;
      return { state: s, result: authorized };
    }, true);
    if (!allowed) { await this.cleanup(actor.key, actor.id); fail(401, "invalid_credential", "Enrollment authorization changed"); }
    return result;
  }
  async confirm(token: string, role: Role, enrollmentId: string, key: string): Promise<unknown> {
    const actor = await this.actor(token, role);
    const state = this.check(actor, await this.c.registry.read(actor.key));
    const initial = state.mesh_enrollments?.[enrollmentId];
    if (!initial || initial.role !== role || initial.subject_id !== actor.id) fail(403, "enrollment_mismatch", "Enrollment belongs to another identity");
    const { nodes } = await this.provider.nodes(state.email);
    const found = nodes.filter(n => n.node_key === key && n.user_id === initial.user_id && n.key_id === initial.key_id && usable(n, this.c.now()));
    if (found.length !== 1) fail(403, "mesh_binding_unverified", "Provider did not attest this enrollment and node key");
    const n = found[0]!;
    return this.c.registry.update(actor.key, s => {
      const current = this.check(actor, s);
      const e = current.mesh_enrollments?.[enrollmentId];
      if (!e || !["issued", "confirmed"].includes(e.status) || (e.status !== "confirmed" && e.expires_at <= this.c.now())) fail(409, "enrollment_expired", "Enrollment is not confirmable");
      if (e.key_id !== n.key_id || e.user_id !== n.user_id) fail(403, "mesh_binding_unverified", "Enrollment authority changed");
      if (e.binding && !matches(e, n)) fail(409, "mesh_binding_changed", "Confirmed enrollment cannot change node identity");
      if (enrollments(current).some(other => other.enrollment_id !== enrollmentId && (other.status !== "revoked" || other.role !== role || other.subject_id !== actor.id) && other.binding && (other.binding.node_id === n.id || other.binding.node_key === n.node_key))) fail(409, "mesh_identity_in_use", "Node identity is already bound");
      e.binding = { node_id: n.id, node_key: n.node_key, user_id: n.user_id, key_id: n.key_id! }; e.status = "confirmed";
      const result = role === "controller" ? current.devices[actor.id]!.record : { owner_node_id: actor.id, mesh_node_key: n.node_key, host_revision: current.hosts[actor.id]!.revision };
      return { state: current, result };
    });
  }
  async requireHost(token: string, expectedKey?: string): Promise<{ actor: Actor; state: AccountState; nodes: ProviderNode[]; node: ProviderNode; enrollment_id: string }> {
    const actor = await this.actor(token, "host");
    const before = this.check(actor, await this.c.registry.read(actor.key));
    const { nodes } = await this.provider.nodes(before.email);
    const state = this.check(actor, await this.c.registry.read(actor.key));
    const e = enrollments(state).find(e => e.role === "host" && e.subject_id === actor.id && e.status === "confirmed");
    const n = e && nodes.find(n => matches(e, n) && usable(n, this.c.now()));
    if (!n || (expectedKey !== undefined && expectedKey !== n.node_key)) fail(403, "host_mesh_unverified", "Host has no current provider-verified mesh binding");
    return { actor, state, nodes, node: n, enrollment_id: e!.enrollment_id };
  }
  async peers(token: string): Promise<RegisteredPeers> {
    const { actor, nodes } = await this.requireHost(token);
    const row = await this.c.registry.store.read(actor.key);
    const state = this.check(actor, row === null ? null : parseAccount(row.value));
    const host = state.hosts[actor.id]!;
    if (!host.lease || host.lease.expires_at <= this.c.now()) fail(403, "host_lease_expired", "Host must hold a current lease");
    let duration = Math.min(AUTH_LEASE_SECONDS, Math.floor((host.lease.expires_at - this.c.now()) / 1000));
    if (duration <= 0) fail(403, "host_lease_expired", "Host lease is expiring");
    const peers: RegisteredPeers["peers"] = [];
    for (const e of enrollments(state).filter(e => e.status === "confirmed" && live(e, state, this.c.now()))) {
      const n = nodes.find(n => matches(e, n) && usable(n, this.c.now()));
      if (!n) continue;
      if (e.role === "host") {
        const h = state.hosts[e.subject_id];
        if (!h?.controller_port || !h.tls_spki_sha256 || !h.lease || h.lease.expires_at <= this.c.now() || h.lease.mesh_node_key !== n.node_key) continue;
      }
      if (n.expires_at !== null) duration = Math.min(duration, Math.floor((n.expires_at - this.c.now()) / 1000));
      if (e.role === "host") duration = Math.min(duration, Math.floor((state.hosts[e.subject_id]!.lease!.expires_at - this.c.now()) / 1000));
      else duration = Math.min(duration, Math.floor((state.devices[e.subject_id]!.refresh_expires_at - this.c.now()) / 1000));
      peers.push({ mesh_node_key: n.node_key, role: e.role, owner_node_id: e.role === "host" ? e.subject_id : null });
    }
    if (duration <= 0) fail(503, "admission_expiring", "Peer admission authority is expiring");
    return { protocol_version: 1, revision: row!.revision, lease_seconds: duration, peers };
  }
  async hostContext(token: string): Promise<unknown> {
    const actor = await this.actor(token, "host");
    const before = this.check(actor, await this.c.registry.read(actor.key));
    const snapshot = await this.provider.nodes(before.email);
    const state = this.check(actor, await this.c.registry.read(actor.key));
    const active = enrollments(state).filter(e => e.role === "host" && e.subject_id === actor.id && live(e, state, this.c.now()));
    if (active.length > 1) fail(503, "enrollment_corrupt", "Multiple active host enrollments");
    const e = active[0];
    if (!e) return { login_server: this.provider.loginServer, enrollment: null };
    const node = e.binding ? snapshot.nodes.find(n => n.id === e.binding!.node_id && usable(n, this.c.now())) : undefined;
    if (node && !matches(e, node)) fail(409, "mesh_binding_changed", "Provider host identity changed");
    // A disappeared confirmed node is an explicit re-enrollment state, not an outage.
    if (e.status === "confirmed" && !node) return { login_server: this.provider.loginServer, enrollment: null };
    return { login_server: this.provider.loginServer, enrollment: { enrollment_id: e.enrollment_id, status: e.status, mesh_node_key: node?.node_key ?? null } };
  }
  async hosts(token: string): Promise<unknown> {
    const before = await this.c.authenticate(token, "catalog.read");
    const { nodes } = await this.provider.nodes(before.state.email);
    const auth = await this.c.authenticate(token, "catalog.read");
    const es = enrollments(auth.state);
    return { revision: auth.revision, hosts: Object.values(auth.state.hosts).map(h => {
      const e = es.find(e => e.role === "host" && e.subject_id === h.owner_node_id && e.status === "confirmed");
      const n = e && nodes.find(n => matches(e, n) && usable(n, this.c.now()));
      const leased = h.lease !== null && h.lease.expires_at > this.c.now();
      const compatible = e !== undefined && n !== undefined && activatedHost(auth.state, e, n);
      const online = compatible && leased && h.lease?.mesh_node_key === n.node_key && n.online && n.ips.length > 0;
      return { owner_node_id: h.owner_node_id, name: h.host_name, device_role: "host", protocol_version: h.protocol_version, availability: !compatible ? "update_required" : online ? "online" : "offline", registration_state: compatible ? "verified" : "awaiting_mesh_binding", last_seen_at: h.lease?.renewed_at !== undefined ? new Date(h.lease.renewed_at).toISOString() : null, endpoints: online ? n.ips.map(ip => ({ url: "https://" + (ip.includes(":") ? "[" + ip + "]" : ip) + ":" + h.controller_port, tls_spki_sha256: h.tls_spki_sha256 })) : [], capabilities: online ? [...(h.lease?.capabilities ?? [])] : [] };
    }) };
  }
  async cleanup(accountKey: string, deviceId: string): Promise<"complete" | "pending"> {
    const state = await this.c.registry.read(accountKey);
    if (!state) return "complete";
    const d = state.devices[deviceId];
    if (!d) return "complete";
    const revokedDevice = d.record.revoked_at !== null || d.refresh_expires_at <= this.c.now();
    if (!enrollments(state).some(e => e.role === "controller" && e.subject_id === deviceId && (e.cleanup_pending || (revokedDevice && e.status !== "revoked")))) return "complete";
    await this.c.registry.update(accountKey, s => {
      if (!s) throw new Error("missing revoked account");
      for (const e of enrollments(s)) if (e.role === "controller" && e.subject_id === deviceId && revokedDevice && e.status !== "revoked") { e.status = "revoked"; e.cleanup_pending = true; }
      return { state: s, result: undefined };
    });
    const current = (await this.c.registry.read(accountKey))!;
    const targets = enrollments(current).filter(e => e.role === "controller" && e.subject_id === deviceId && e.status === "revoked" && e.cleanup_pending);
    if (targets.length === 0) return "complete";
    try {
      // Expire enrollment keys FIRST, then list/delete every node they admitted,
      // including a join that happened before confirm or raced revocation.
      for (const e of targets) if (e.key_id) await this.provider.expireKey(e.key_id);
      const { nodes } = await this.provider.nodes(current.email);
      for (const n of nodes) if (targets.some(e => e.key_id !== null && e.key_id === n.key_id && e.user_id === n.user_id)) await this.provider.deleteNode(n.id);
      // A missing mint response has no key ID to revoke. Keep this explicit until
      // its bounded key lifetime expires; it never becomes an admitted peer.
      const complete = targets.every(e => e.key_id !== null || e.expires_at <= this.c.now());
      if (complete) await this.c.registry.update(accountKey, s => {
        if (!s) throw new Error("missing revoked account");
        // A mint finalizer may have learned a new key since this cleanup began.
        for (const e of targets) { const saved = s.mesh_enrollments?.[e.enrollment_id]; if (saved && saved.key_id === e.key_id) saved.cleanup_pending = false; }
        return { state: s, result: undefined };
      });
      return complete ? "complete" : "pending";
    } catch { return "pending"; }
  }
}

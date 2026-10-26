import { reapUnstartedLegacyMints } from "./legacy-mesh";
import type { Deps } from "../lib/deps";
import { EMAIL_RATE_LIMIT, OTP_MAX_ATTEMPTS, OTP_TTL_SECONDS } from "../lib/config";
import { constantTimeEqual } from "../lib/otp";
import { sendOtpEmail } from "../lib/resend";
import { digest, randomSecret, seal, unseal } from "./crypto";
import { fail, splitCredential } from "./registry";
import { ControllerService, deviceFrom, spendLimit } from "./service";
import type { AccountDeletion, AccountState } from "./types";
import { deleteAccountMesh, deleteLinkContact, deleteLinkEvents } from "./account-deletion-cleanup";

// A transport lease, not a task/work deadline. Every provider request renews it;
// its fetch timeout is shorter so a crashed cleanup can be reclaimed on retry.
const CLEANUP_LEASE_MS = 60_000;
const CLEANUP_FETCH_MS = 30_000;

export interface DeletionChallenge { request_id: string; deletion_token: string; expires_at: string }
export interface DeletionStatus { request_id: string; status: "pending" | "deleted"; pending: string[]; retained: string[]; completed_at: string | null }

/** Apple permits purpose-specific emailed reauthentication for account deletion:
 * https://developer.apple.com/help/app-review/guideline-reference/5-1-1-account-deletion
 * The receipt is issued BEFORE confirmation, so a dropped confirming response never
 * strands the caller behind the access revocation it just requested. */
export class AccountDeletionService {
  constructor(private readonly c: ControllerService, private readonly deps: Deps) {}

  async requestCode(token: string, requestId: string): Promise<DeletionChallenge> {
    const auth = await this.c.authenticate(token, "devices.manage");
    const key = auth.claims.account_key; const code = this.deps.randomOtp();
    const codeHash = await digest(`account-deletion\n${requestId}\n${code}`);
    const deletionToken = `d1.${key}.${requestId}.${randomSecret()}`;
    const tokenHash = await digest(deletionToken);
    const reservation = await this.c.registry.update(key, async state => {
      const actor = deviceFrom(state, auth.claims, this.c.now());
      if (!actor.record.scopes.includes("devices.manage")) fail(403, "scope_denied", "Account management is not allowed");
      const old = state!.deletion;
      if (old && old.request_id === requestId) {
        if (old.actor_device_id !== auth.claims.device_id) fail(403, "wrong_device", "Deletion request belongs to another device");
        if (old.status === "sending") fail(409, "mail_pending", "Deletion code delivery is still in progress");
        if (old.status !== "ready" || old.expires_at <= this.c.now() || !old.token_receipt) fail(409, "deletion_challenge_expired", "Request a new deletion code");
        return { state: state!, result: { send: false, challenge: await unseal<DeletionChallenge>(this.c.secret, deletionTokenContext(key, requestId), old.token_receipt) } };
      }
      spendLimit(state!, "account_deletion_code", EMAIL_RATE_LIMIT, this.c.now());
      const expires = this.c.now() + OTP_TTL_SECONDS * 1000;
      const challenge: DeletionChallenge = { request_id: requestId, deletion_token: deletionToken, expires_at: new Date(expires).toISOString() };
      state!.deletion = { request_id: requestId, actor_device_id: auth.claims.device_id, token_hash: tokenHash, token_receipt: await seal(this.c.secret, deletionTokenContext(key, requestId), challenge), code_hash: codeHash, attempts: 0, expires_at: expires, status: "sending", cleanup_lease: null, requested_at: null, completed_at: null, pending: [], retained: ["security_revocation_record", "historical_service_records"] };
      return { state: state!, result: { send: true, challenge } };
    });
    if (reservation.send) {
      const sent = await sendOtpEmail(this.deps, auth.state.email, code, "account-deletion");
      await this.c.registry.update(key, state => {
        if (state?.deletion?.request_id !== requestId || state.deletion.status !== "sending") fail(409, "deletion_challenge_changed", "Deletion challenge changed");
        state.deletion.status = sent.ok ? "ready" : "failed";
        return { state, result: undefined };
      });
      if (!sent.ok) fail(502, "mail_failed", "Could not deliver deletion code");
    }
    return reservation.challenge;
  }

  private async identity(token: string): Promise<{ key: string; id: string; hash: string }> {
    return { ...splitCredential(token, "d1"), hash: await digest(token) };
  }
  private check(state: AccountState | null, id: string, hash: string): AccountDeletion {
    const deletion = state?.deletion;
    if (!deletion || deletion.request_id !== id || !constantTimeEqual(deletion.token_hash, hash)) fail(401, "invalid_deletion_credential", "Invalid deletion receipt");
    return deletion;
  }
  async confirm(token: string, code: string): Promise<DeletionStatus> {
    const { key, id, hash } = await this.identity(token);
    const completed = this.completed(await this.c.registry.read(key), id, hash);
    if (completed) return completed;
    const codeHash = await digest(`account-deletion\n${id}\n${code}`);
    const accepted = await this.c.registry.update(key, state => {
      const d = this.check(state, id, hash);
      if (["pending", "deleted"].includes(d.status)) return { state: state!, result: true };
      if (d.status !== "ready" || d.expires_at <= this.c.now() || d.attempts >= OTP_MAX_ATTEMPTS) fail(401, "invalid_code", "Invalid or expired deletion code");
      d.attempts += 1;
      if (!constantTimeEqual(d.code_hash, codeHash)) return { state: state!, result: false };
      d.status = "pending"; d.requested_at = this.c.now();
      state!.legacy_revoked_before = Math.floor(this.c.now() / 1000); d.code_hash = ""; d.token_receipt = null;
      d.pending = ["mesh", "contact", "activity", "account_records"];
      // The barrier is authoritative immediately. Keep only the fields needed by
      // in-flight mesh finalizers until external cleanup has reconciled their keys.
      state!.devices = {}; state!.hosts = {}; state!.challenges = {}; state!.revocations = {};
      delete state!.desktop_enrollments; delete state!.generation_recoveries;
      return { state: state!, result: true };
    }, true);
    if (!accepted) fail(401, "invalid_code", "Invalid or expired deletion code");
    return this.status(token);
  }

  private completed(state: AccountState | null, id: string, hash: string): DeletionStatus | null {
    const old = state?.completed_deletion;
    return old?.request_id === id && constantTimeEqual(old.token_hash, hash) ? { request_id: id, status: "deleted", pending: [], retained: [...old.retained], completed_at: old.completed_at } : null;
  }

  async status(token: string): Promise<DeletionStatus> {
    const { key, id, hash } = await this.identity(token);
    const completed = this.completed(await this.c.registry.read(key), id, hash);
    if (completed) return completed;
    const leaseId = crypto.randomUUID();
    const state = await this.c.registry.update(key, current => {
      const d = this.check(current, id, hash);
      if (d.status === "deleted") return { state: current!, result: current! };
      if (d.status !== "pending") fail(409, "deletion_not_confirmed", "Enter the emailed deletion code first");
      if (d.cleanup_lease && d.cleanup_lease.expires_at > this.c.now()) return { state: current!, result: structuredClone(current!) };
      reapUnstartedLegacyMints(current!, this.c.now());
      d.cleanup_lease = { id: leaseId, expires_at: this.c.now() + CLEANUP_LEASE_MS };
      return { state: current!, result: structuredClone(current!) };
    }, true);
    if (state.deletion!.status === "deleted") return result(state.deletion!);
    if (state.deletion!.cleanup_lease?.id !== leaseId) return { ...result(state.deletion!), pending: ["in_flight_cleanup"] };
    let leaseUntil = this.c.now() + CLEANUP_LEASE_MS;
    const renew = async (): Promise<void> => this.c.registry.update(key, current => {
      const d = this.check(current, id, hash);
      if (d.status !== "pending" || d.cleanup_lease?.id !== leaseId || d.cleanup_lease.expires_at <= this.c.now()) fail(409, "cleanup_lease_lost", "Another request owns cleanup recovery");
      leaseUntil = this.c.now() + CLEANUP_LEASE_MS;
      d.cleanup_lease.expires_at = leaseUntil;
      return { state: current!, result: undefined };
    }, true);
    const cleanupDeps: Deps = { ...this.deps, fetchFn: async (url, init) => {
      await renew();
      const response = await this.deps.fetchFn(url, { ...init, signal: AbortSignal.timeout(CLEANUP_FETCH_MS) });
      await renew();
      return response;
    } };
    const pending: string[] = [];
    const operations = Object.values(state.legacy_operations ?? {});
    const retained = new Set(state.deletion!.retained);
    if (Object.values(state.legacy_mesh_mints?.reservations ?? {}).some(r => (r.uncertain ?? 0) > 0)) {
      pending.push("legacy_mesh_outcome_unknown");
    } else if (operations.some(op => op.expires_at > this.c.now()) || (state!.legacy_mesh_mints?.in_flight ?? 0) > 0) {
      pending.push("in_flight_requests");
    } else if (operations.some(op => op.external_pending)) {
      pending.push("legacy_write_outcome_unknown");
    } else {
      for (const [name, clean] of [["mesh", deleteAccountMesh], ["contact", deleteLinkContact], ["activity", deleteLinkEvents]] as const) {
        try { const kept = await clean(cleanupDeps, state!.email); if (kept) for (const item of kept) retained.add(item); } catch { pending.push(name); }
      }
      if (pending.length === 0) {
        try {
          for (const prefix of ["nodesecret:", "otp:", "rl:email:", "rl:verify:"]) {
            await renew();
            const name = prefix + state!.email; const oldValue = await this.deps.kv.get(name);
            // A resumed old cleanup must never remove a fresh enrollment's value.
            await renew();
            await this.deps.kv.update(name, value => {
              if (this.c.now() >= leaseUntil) fail(409, "cleanup_lease_lost", "Cleanup lease expired before erasure");
              return value === oldValue ? "delete" : "keep";
            });
          }
        } catch { pending.push("account_records"); }
      }
    }
    return this.c.registry.update(key, current => {
      const d = this.check(current, id, hash);
      if (d.status === "deleted") return { state: current!, result: result(d) };
      if (d.cleanup_lease?.id !== leaseId) return { state: current!, result: result(d) };
      if (d.cleanup_lease.expires_at <= this.c.now()) pending.push("cleanup_interrupted");
      d.cleanup_lease = null;
      const remaining = pending;
      if (remaining.length === 0) {
        d.status = "deleted"; d.completed_at = new Date(this.c.now()).toISOString();
        current!.legacy_revoked_before = Math.floor(this.c.now() / 1000);
        // Retain only a non-authenticating revocation tombstone and deletion receipt.
        current!.email = ""; current!.rate_windows = {};
        delete current!.mesh_enrollments; delete current!.legacy_mesh_mints; delete current!.legacy_operations;
      }
      d.pending = remaining; d.retained = [...retained];
      return { state: current!, result: result(d) };
    }, true);
  }
}
function deletionTokenContext(key: string, id: string): string { return `${key}.account-deletion.${id}`; }
function result(d: AccountDeletion): DeletionStatus {
  return { request_id: d.request_id, status: d.status === "deleted" ? "deleted" : "pending", pending: [...d.pending], retained: [...d.retained], completed_at: d.completed_at };
}

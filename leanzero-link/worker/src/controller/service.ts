import { constantTimeEqual } from "../lib/otp";
import { EMAIL_RATE_LIMIT, OTP_MAX_ATTEMPTS, OTP_TTL_SECONDS, RATE_WINDOW_SECONDS, VERIFY_RATE_LIMIT } from "../lib/config";
import { ACCESS_TTL_SECONDS, REFRESH_TTL_SECONDS, digest, randomSecret, seal, signAccess, unseal, verifyAccess } from "./crypto";
import { CONTROLLER_SCOPES, DESKTOP_CONTROLLER_SCOPES, type AccessClaims, type AccountState, type DeviceCredentials, type DeviceRecord, type DeviceState, type Platform } from "./types";
import { ControllerError, Registry, fail, splitCredential, parseAccount, assertAccountActive } from "./registry";

// Constructed only after a recognized refresh secret revokes its family in CAS.
export class AuthenticatedRefreshReplay extends ControllerError {
  constructor(readonly accountKey: string, readonly deviceId: string) {
    super(401, "refresh_replayed", "Refresh replay revoked this device");
  }
}

export function newAccount(email: string): AccountState {
  return { schema: 1, account_id: crypto.randomUUID(), email, challenges: {}, devices: {}, hosts: {}, revocations: {}, rate_windows: {} };
}
export function enrollmentAccount(state: AccountState | null, email: string): AccountState {
  if (state?.deletion?.status === "pending") assertAccountActive(state);
  if (state?.deletion?.status !== "deleted") return state ?? newAccount(email);
  const old = state.deletion;
  return { ...newAccount(email), legacy_revoked_before: state.legacy_revoked_before!, completed_deletion: { request_id: old.request_id, token_hash: old.token_hash, completed_at: old.completed_at!, retained: [...old.retained] } };
}
export function spendLimit(state: AccountState, name: string, limit: number, now: number): void {
  let window = state.rate_windows[name];
  if (!window || now >= window.start + RATE_WINDOW_SECONDS * 1000) window = { start: now, count: 0 };
  if (window.count >= limit) fail(429, "rate_limited", "Too many enrollment attempts");
  window.count += 1; state.rate_windows[name] = window;
}
export function deviceFrom(state: AccountState | null, claims: AccessClaims, now: number): DeviceState {
  assertAccountActive(state);
  const device = state?.devices[claims.device_id];
  if (!state || state.account_id !== claims.sub || !device || device.record.revoked_at !== null || device.record.revision !== claims.device_revision || claims.exp <= Math.floor(now / 1000)) {
    fail(401, "invalid_credential", "Device credential is expired or revoked");
  }
  return device;
}

export class ControllerService {
  constructor(readonly registry: Registry, readonly secret: string, readonly now: () => number) {}

  async claims(token: string): Promise<AccessClaims> {
    const claims = await verifyAccess(this.secret, token, Math.floor(this.now() / 1000));
    if (!claims) fail(401, "invalid_credential", "Invalid device credential");
    return claims;
  }
  async authenticate(token: string, scope?: string): Promise<{ claims: AccessClaims; state: AccountState; device: DeviceState; revision: number }> {
    const claims = await this.claims(token);
    const row = await this.registry.store.read(claims.account_key);
    const state = row === null ? null : parseAccount(row.value);
    const device = deviceFrom(state, claims, this.now());
    if (scope && !device.record.scopes.includes(scope as typeof CONTROLLER_SCOPES[number])) fail(403, "scope_denied", "Device lacks the required scope");
    return { claims, state: state!, device, revision: row!.revision };
  }

  async prepareChallenge(email: string, enrollmentId: string, code: string): Promise<{ challenge_id: string; expires_at: string; send: boolean }> {
    const key = await digest(email);
    const hash = await digest(`${enrollmentId}\n${code}`);
    return this.registry.update<{ challenge_id: string; expires_at: string; send: boolean }>(key, state => {
      const account = enrollmentAccount(state, email);
      const existing = account.challenges[enrollmentId];
      if (existing && existing.expires_at > this.now() && existing.status !== "failed") {
        if (existing.status === "sending") fail(409, "mail_pending", "Code delivery is still in progress");
        return { state: account, result: { challenge_id: existing.challenge_id, expires_at: new Date(existing.expires_at).toISOString(), send: false } };
      }
      spendLimit(account, "request_code", EMAIL_RATE_LIMIT, this.now());
      const challenge_id = `${key}.${crypto.randomUUID()}`;
      const expires = this.now() + OTP_TTL_SECONDS * 1000;
      account.challenges[enrollmentId] = { challenge_id, enrollment_id: enrollmentId, hash, attempts: 0, expires_at: expires, status: "sending" };
      return { state: account, result: { challenge_id, expires_at: new Date(expires).toISOString(), send: true } };
    }, true);
  }

  async challengeSent(challengeId: string, enrollmentId: string, sent: boolean): Promise<void> {
    const key = challengeId.split(".")[0]!;
    await this.registry.update(key, state => {
      const c = state?.challenges[enrollmentId];
      if (!state || !c || c.challenge_id !== challengeId || c.status !== "sending") fail(409, "challenge_changed", "Enrollment challenge changed");
      c.status = sent ? "ready" : "failed";
      return { state, result: undefined };
    });
  }

  async credentials(key: string, account: AccountState, record: DeviceRecord): Promise<DeviceCredentials> {
    const iat = Math.floor(this.now() / 1000);
    const exp = iat + ACCESS_TTL_SECONDS;
    return {
      access_token: await signAccess(this.secret, { ver: 2, token_use: "controller", iss: "leanzero-link-auth", aud: "leanzero-link/controller", sub: account.account_id, account_key: key, device_id: record.device_id, device_revision: record.revision, jti: crypto.randomUUID(), iat, exp }),
      token_type: "Bearer", expires_at: new Date(exp * 1000).toISOString(),
      refresh_token: `r1.${key}.${record.device_id}.${randomSecret()}`, device: structuredClone(record),
    };
  }

  async verify(challengeId: string, enrollmentId: string, code: string, name: string, platform: Platform): Promise<DeviceCredentials> {
    const key = challengeId.split(".")[0]!;
    // Also bounds attempts against a consumed challenge's recovery receipt. This cannot use
    // eventually consistent Workers KV: guessing a consumed OTP must not be an unbounded door.
    await this.registry.update(key, state => {
      if (!state) fail(401, "invalid_code", "Invalid or expired code");
      spendLimit(state, "verify", VERIFY_RATE_LIMIT, this.now());
      return { state, result: undefined };
    });
    const hash = await digest(`${enrollmentId}\n${code}`);
    const requestHash = await digest(JSON.stringify({ challengeId, enrollmentId, hash, name, platform }));
    const result = await this.registry.update<{ credentials: DeviceCredentials | null }>(key, async state => {
      const c = state?.challenges[enrollmentId];
      if (!state || !c || c.challenge_id !== challengeId || c.expires_at <= this.now() || !["ready", "consumed"].includes(c.status)) fail(401, "invalid_code", "Invalid or expired code");
      if (c.status === "consumed") {
        const device = c.device_id === undefined ? undefined : state.devices[c.device_id];
        if (c.request_hash !== requestHash || !c.receipt || !device || device.record.revoked_at !== null || device.record.revision !== 1) fail(409, "enrollment_consumed", "Enrollment has already completed");
        return { state, result: { credentials: await unseal<DeviceCredentials>(this.secret, challengeId, c.receipt) } };
      }
      if (c.attempts >= OTP_MAX_ATTEMPTS) fail(401, "invalid_code", "Invalid or expired code");
      c.attempts += 1;
      if (!constantTimeEqual(c.hash, hash)) return { state, result: { credentials: null } };
      const record: DeviceRecord = { device_id: crypto.randomUUID(), name, platform, role: "controller", revision: 1, scopes: [...(platform === "desktop" ? DESKTOP_CONTROLLER_SCOPES : CONTROLLER_SCOPES)], created_at: new Date(this.now()).toISOString(), revoked_at: null };
      const credentials = await this.credentials(key, state, record);
      state.devices[record.device_id] = { record, refresh_hash: await digest(credentials.refresh_token), refresh_expires_at: this.now() + REFRESH_TTL_SECONDS * 1000, used_refreshes: {} };
      c.status = "consumed"; c.request_hash = requestHash; c.device_id = record.device_id;
      c.receipt = await seal(this.secret, challengeId, credentials);
      return { state, result: { credentials } };
    });
    if (!result.credentials) fail(401, "invalid_code", "Invalid or expired code");
    return result.credentials;
  }

  async refresh(deviceId: string, token: string, requestId: string): Promise<DeviceCredentials> {
    const { key, id } = splitCredential(token, "r1");
    if (id !== deviceId) fail(401, "invalid_credential", "Device does not match credential");
    const hash = await digest(token);
    const outcome = await this.registry.update<{ credentials: DeviceCredentials | null }>(key, async state => {
      const d = state?.devices[id];
      if (!state || !d || d.record.revoked_at !== null || d.refresh_expires_at <= this.now()) fail(401, "invalid_credential", "Refresh credential is expired or revoked");
      if (!constantTimeEqual(d.refresh_hash, hash)) {
        const used = d.used_refreshes[hash];
        if (!used) fail(401, "invalid_credential", "Invalid refresh credential");
        if (used.request_id === requestId && used.revision === d.record.revision) {
          return { state, result: { credentials: await unseal<DeviceCredentials>(this.secret, `${key}.${id}.${requestId}`, used.receipt) } };
        }
        d.record.revoked_at = new Date(this.now()).toISOString(); d.record.revision += 1;
        return { state, result: { credentials: null } };
      }
      if (Object.values(d.used_refreshes).some(r => r.request_id === requestId)) fail(409, "request_id_conflict", "Refresh request ID was already used");
      d.record.revision += 1;
      const credentials = await this.credentials(key, state, d.record);
      d.used_refreshes[hash] = { request_id: requestId, revision: d.record.revision, receipt: await seal(this.secret, `${key}.${id}.${requestId}`, credentials) };
      d.refresh_hash = await digest(credentials.refresh_token);
      return { state, result: { credentials } };
    });
    if (!outcome.credentials) throw new AuthenticatedRefreshReplay(key, id);
    return outcome.credentials;
  }

  async logout(token: string, requestId: string): Promise<{ revoked: true }> {
    const claims = await this.claims(token);
    return this.registry.update(claims.account_key, state => {
      const key = `${claims.device_id}.${requestId}`;
      const old = state?.revocations[key];
      if (old && old.request_id === claims.device_id) return { state: state!, result: { revoked: true as const } };
      const device = deviceFrom(state, claims, this.now());
      device.record.revoked_at = new Date(this.now()).toISOString(); device.record.revision += 1;
      state!.revocations[key] = { request_id: claims.device_id, expected_revision: claims.device_revision, revision: device.record.revision, revoked_at: device.record.revoked_at };
      return { state: state!, result: { revoked: true as const } };
    });
  }

  async revoke(token: string, deviceId: string, requestId: string, expectedRevision: number): Promise<{ device_id: string; revision: number; revoked_at: string }> {
    const claims = await this.claims(token);
    return this.registry.update(claims.account_key, state => {
      const actor = deviceFrom(state, claims, this.now());
      if (!actor.record.scopes.includes("devices.manage")) fail(403, "scope_denied", "Device management is not allowed");
      const target = state!.devices[deviceId];
      if (!target) fail(404, "device_missing", "Device was not found");
      const receiptKey = `${claims.device_id}.${requestId}`;
      const old = state!.revocations[receiptKey];
      if (old) {
        if (old.expected_revision !== expectedRevision || old.request_id !== deviceId) fail(409, "request_id_conflict", "Request ID has different contents");
        return { state: state!, result: { device_id: deviceId, revision: old.revision, revoked_at: old.revoked_at } };
      }
      if (target.record.revision !== expectedRevision || target.record.revoked_at !== null) fail(409, "stale_revision", "Device state changed");
      target.record.revision += 1; target.record.revoked_at = new Date(this.now()).toISOString();
      state!.revocations[receiptKey] = { request_id: deviceId, expected_revision: expectedRevision, revision: target.record.revision, revoked_at: target.record.revoked_at };
      return { state: state!, result: { device_id: deviceId, revision: target.record.revision, revoked_at: target.record.revoked_at } };
    });
  }
}

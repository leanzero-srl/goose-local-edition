import { JWT_TTL_SECONDS, OTP_MAX_ATTEMPTS, RATE_WINDOW_SECONDS, VERIFY_RATE_LIMIT } from "../lib/config";
import type { Deps, KVStore } from "../lib/deps";
import { jsonResponse, readJsonBody } from "../lib/http";
import { signJwt } from "../lib/jwt";
import { ensureNodeSecret } from "../lib/nodeSecret";
import { constantTimeEqual, hashOtp } from "../lib/otp";
import { bumpFixedWindow } from "../lib/ratelimit";
import { upsertAudienceContact } from "../lib/resend";
import { normalizeCode, normalizeEmail } from "../lib/validate";
import { otpKey, type OtpRecord } from "./requestCode";

function parseOtpRecord(raw: string): OtpRecord | null {
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null) {
      return null;
    }
    const record = value as Record<string, unknown>;
    if (
      typeof record.hash !== "string" ||
      typeof record.attempts !== "number" ||
      typeof record.expiresAtMs !== "number"
    ) {
      return null;
    }
    return { hash: record.hash, attempts: record.attempts, expiresAtMs: record.expiresAtMs };
  } catch {
    return null;
  }
}

type Claim =
  | { kind: "absent" }
  | { kind: "corrupt" }
  | { kind: "expired" }
  | { kind: "exhausted" }
  | { kind: "claimed"; record: OtpRecord };

/// Spend one attempt on the email's code BEFORE the hash is compared, as one atomic
/// step on the record: N concurrent guesses claim N attempts (up to the cap), never one.
/// The get→compare→put shape this replaces recorded 500 concurrent wrong codes as a
/// single attempt and then accepted the correct code (measured).
async function claimAttempt(kv: KVStore, key: string, nowMs: number): Promise<Claim> {
  let claim: Claim = { kind: "absent" };
  await kv.update(key, (raw) => {
    if (raw === null) {
      claim = { kind: "absent" };
      return "keep";
    }
    const record = parseOtpRecord(raw);
    if (record === null) {
      claim = { kind: "corrupt" };
      return "delete";
    }
    if (nowMs >= record.expiresAtMs) {
      claim = { kind: "expired" };
      return "delete";
    }
    if (record.attempts >= OTP_MAX_ATTEMPTS) {
      claim = { kind: "exhausted" };
      return "keep";
    }
    claim = { kind: "claimed", record };
    const remainingSeconds = Math.ceil((record.expiresAtMs - nowMs) / 1000);
    return {
      value: JSON.stringify({ ...record, attempts: record.attempts + 1 } satisfies OtpRecord),
      expirationTtl: Math.max(remainingSeconds, 60),
    };
  });
  return claim;
}

export async function handleVerify(request: Request, deps: Deps): Promise<Response> {
  const secret = deps.config.jwtSecret;
  if (!secret) {
    const error = deps.config.jwtSecretError ?? "LINK_JWT_SECRET not configured on this deployment";
    deps.log("config_error", { error });
    return jsonResponse(500, { error });
  }
  const body = await readJsonBody(request);
  if (!body.ok || typeof body.value !== "object" || body.value === null) {
    return jsonResponse(400, { error: "invalid JSON body" });
  }
  const fields = body.value as Record<string, unknown>;
  const email = normalizeEmail(fields.email);
  if (email === null) {
    return jsonResponse(400, { error: "invalid email" });
  }
  const code = normalizeCode(fields.code);
  if (code === null) {
    return jsonResponse(400, { error: "code must be a 6-digit string" });
  }

  const verifyLimit = await bumpFixedWindow(deps.kv, `rl:verify:${email}`, VERIFY_RATE_LIMIT, RATE_WINDOW_SECONDS, deps.now());
  if (verifyLimit.limited) {
    deps.log("rate_limited", { scope: "verify", email, retryAfterSeconds: verifyLimit.retryAfterSeconds });
    return jsonResponse(
      429,
      { error: "rate limited", scope: "email", retryAfterSeconds: verifyLimit.retryAfterSeconds },
      { "Retry-After": String(verifyLimit.retryAfterSeconds) },
    );
  }

  const key = otpKey(email);
  const claim = await claimAttempt(deps.kv, key, deps.now());
  switch (claim.kind) {
    case "absent":
    case "expired":
      return jsonResponse(401, { error: "invalid or expired code" });
    case "corrupt":
      deps.log("otp_record_corrupt", { email });
      return jsonResponse(401, { error: "invalid or expired code" });
    case "exhausted":
      // Indistinguishable from any other failure on the wire: a distinct status told a
      // caller that a code HAD been issued for this email. The operator sees it in the log.
      deps.log("otp_attempts_exhausted", { email });
      return jsonResponse(401, { error: "invalid or expired code" });
    case "claimed":
      break;
  }
  const submittedHash = await hashOtp(email, code);
  if (!constantTimeEqual(submittedHash, claim.record.hash)) {
    return jsonResponse(401, { error: "invalid or expired code" });
  }

  await deps.kv.delete(key);
  const iat = Math.floor(deps.now() / 1000);
  const token = await signJwt(secret, { sub: email, iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
  const nodeSecret = await ensureNodeSecret(deps.kv, email, deps.log);
  const audienceSync = await upsertAudienceContact(deps, email);
  deps.log("auth_verified", { email, audienceSync });
  return jsonResponse(200, { token, email, audienceSync, nodeSecret });
}

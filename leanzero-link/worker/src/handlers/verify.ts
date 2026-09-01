import { JWT_TTL_SECONDS, OTP_MAX_ATTEMPTS } from "../lib/config";
import type { Deps } from "../lib/deps";
import { jsonResponse, readJsonBody } from "../lib/http";
import { signJwt } from "../lib/jwt";
import { constantTimeEqual, hashOtp } from "../lib/otp";
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

export async function handleVerify(request: Request, deps: Deps): Promise<Response> {
  const secret = deps.config.jwtSecret;
  if (!secret) {
    deps.log("config_error", { error: "LINK_JWT_SECRET is not configured" });
    return jsonResponse(500, { error: "LINK_JWT_SECRET not configured on this deployment" });
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

  const key = otpKey(email);
  const raw = await deps.kv.get(key);
  if (raw === null) {
    return jsonResponse(401, { error: "invalid or expired code" });
  }
  const record = parseOtpRecord(raw);
  if (record === null) {
    deps.log("otp_record_corrupt", { email });
    await deps.kv.delete(key);
    return jsonResponse(401, { error: "invalid or expired code" });
  }
  if (deps.now() >= record.expiresAtMs) {
    await deps.kv.delete(key);
    return jsonResponse(401, { error: "invalid or expired code" });
  }
  if (record.attempts >= OTP_MAX_ATTEMPTS) {
    deps.log("otp_attempts_exhausted", { email });
    return jsonResponse(429, { error: "too many attempts; request a new code" });
  }
  const submittedHash = await hashOtp(email, code);
  if (!constantTimeEqual(submittedHash, record.hash)) {
    const updated: OtpRecord = { ...record, attempts: record.attempts + 1 };
    const remainingSeconds = Math.ceil((record.expiresAtMs - deps.now()) / 1000);
    await deps.kv.put(key, JSON.stringify(updated), { expirationTtl: Math.max(remainingSeconds, 60) });
    return jsonResponse(401, { error: "invalid or expired code" });
  }

  await deps.kv.delete(key);
  const iat = Math.floor(deps.now() / 1000);
  const token = await signJwt(secret, { sub: email, iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
  const audienceSync = await upsertAudienceContact(deps, email);
  deps.log("auth_verified", { email, audienceSync });
  return jsonResponse(200, { token, email, audienceSync });
}

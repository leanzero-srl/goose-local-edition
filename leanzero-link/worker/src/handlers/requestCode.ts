import { EMAIL_RATE_LIMIT, IP_RATE_LIMIT, OTP_TTL_SECONDS, RATE_WINDOW_SECONDS } from "../lib/config";
import type { Deps } from "../lib/deps";
import { jsonResponse, readJsonBody } from "../lib/http";
import { hashOtp } from "../lib/otp";
import { bumpFixedWindow } from "../lib/ratelimit";
import { sendOtpEmail } from "../lib/resend";
import { normalizeEmail } from "../lib/validate";

export interface OtpRecord {
  hash: string;
  attempts: number;
  expiresAtMs: number;
}

export function otpKey(email: string): string {
  return `otp:${email}`;
}

export async function handleRequestCode(request: Request, deps: Deps): Promise<Response> {
  const { config } = deps;
  if (!config.resendApiKey || !config.mailFrom) {
    return jsonResponse(501, { error: "mail not configured on this deployment" });
  }
  const body = await readJsonBody(request);
  if (!body.ok || typeof body.value !== "object" || body.value === null) {
    return jsonResponse(400, { error: "invalid JSON body" });
  }
  const email = normalizeEmail((body.value as Record<string, unknown>).email);
  if (email === null) {
    return jsonResponse(400, { error: "invalid email" });
  }
  const ip = request.headers.get("CF-Connecting-IP") ?? "unknown";
  const emailLimit = await bumpFixedWindow(deps.kv, `rl:email:${email}`, EMAIL_RATE_LIMIT, RATE_WINDOW_SECONDS, deps.now());
  if (emailLimit.limited) {
    deps.log("rate_limited", { scope: "email", email, retryAfterSeconds: emailLimit.retryAfterSeconds });
    return jsonResponse(
      429,
      { error: "rate limited", scope: "email", retryAfterSeconds: emailLimit.retryAfterSeconds },
      { "Retry-After": String(emailLimit.retryAfterSeconds) },
    );
  }
  const ipLimit = await bumpFixedWindow(deps.kv, `rl:ip:${ip}`, IP_RATE_LIMIT, RATE_WINDOW_SECONDS, deps.now());
  if (ipLimit.limited) {
    deps.log("rate_limited", { scope: "ip", ip, retryAfterSeconds: ipLimit.retryAfterSeconds });
    return jsonResponse(
      429,
      { error: "rate limited", scope: "ip", retryAfterSeconds: ipLimit.retryAfterSeconds },
      { "Retry-After": String(ipLimit.retryAfterSeconds) },
    );
  }
  const code = deps.randomOtp();
  const sent = await sendOtpEmail(deps, email, code);
  if (!sent.ok) {
    return jsonResponse(502, { error: "failed to send code email", status: sent.status });
  }
  const record: OtpRecord = {
    hash: await hashOtp(email, code),
    attempts: 0,
    expiresAtMs: deps.now() + OTP_TTL_SECONDS * 1000,
  };
  await deps.kv.put(otpKey(email), JSON.stringify(record), { expirationTtl: OTP_TTL_SECONDS });
  deps.log("otp_issued", { email });
  return jsonResponse(200, { ok: true, email, expiresInSeconds: OTP_TTL_SECONDS });
}

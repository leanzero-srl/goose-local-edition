import { resolveClientIp } from "../lib/clientIp";
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

const PATH = "/v1/auth/request-code";

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
  // The ip bucket is checked BEFORE the email bucket so one exhausted source cannot keep
  // burning a victim's 3-per-hour email budget with requests that would be refused anyway.
  const client = resolveClientIp(request);
  if (client.ok) {
    deps.log("client_ip_resolved", { path: PATH, ip: client.ip, source: client.source, forwardedFor: client.forwardedFor });
    const ipLimit = await bumpFixedWindow(deps.kv, `rl:ip:${client.ip}`, IP_RATE_LIMIT, RATE_WINDOW_SECONDS, deps.now());
    if (ipLimit.limited) {
      deps.log("rate_limited", { scope: "ip", ip: client.ip, retryAfterSeconds: ipLimit.retryAfterSeconds });
      return jsonResponse(
        429,
        { error: "rate limited", scope: "ip", retryAfterSeconds: ipLimit.retryAfterSeconds },
        { "Retry-After": String(ipLimit.retryAfterSeconds) },
      );
    }
  } else {
    deps.log("client_ip_unresolved", { path: PATH, forwardedFor: client.forwardedFor });
  }
  const emailLimit = await bumpFixedWindow(deps.kv, `rl:email:${email}`, EMAIL_RATE_LIMIT, RATE_WINDOW_SECONDS, deps.now());
  if (emailLimit.limited) {
    deps.log("rate_limited", { scope: "email", email, retryAfterSeconds: emailLimit.retryAfterSeconds });
    return jsonResponse(
      429,
      { error: "rate limited", scope: "email", retryAfterSeconds: emailLimit.retryAfterSeconds },
      { "Retry-After": String(emailLimit.retryAfterSeconds) },
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

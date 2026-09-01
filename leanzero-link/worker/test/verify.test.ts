import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { handleRequestCode, otpKey, type OtpRecord } from "../src/handlers/requestCode";
import { handleVerify } from "../src/handlers/verify";
import { JWT_TTL_SECONDS, OTP_MAX_ATTEMPTS, VERIFY_RATE_LIMIT } from "../src/lib/config";
import { createFsKvStore } from "../src/lib/fs-kv";
import { verifyJwt } from "../src/lib/jwt";
import { hashOtp } from "../src/lib/otp";
import {
  FULL_ENV,
  RESEND_CONTACTS_URL,
  RESEND_EMAILS_URL,
  jsonBody,
  jsonRes,
  makeHarness,
  postJson,
  resendHappyFetch,
  responseJson,
  type TestHarness,
} from "./helpers";

const IP = { "X-Forwarded-For": "203.0.113.7" };
const EMAIL = "user@example.com";

async function seedCode(h: TestHarness, code = "123456"): Promise<void> {
  const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: EMAIL }, IP), h.deps);
  expect(response.status).toBe(200);
  expect(code).toBeDefined();
}

describe("POST /v1/auth/verify", () => {
  it("verifies the code, mints a 180-day v1 JWT, and syncs the audience contact", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(200);
    const body = await responseJson(response);
    expect(body.email).toBe(EMAIL);
    expect(body.audienceSync).toBe("synced");

    const verified = await verifyJwt("unit-test-jwt-secret", String(body.token), Math.floor(h.clock.now() / 1000) + 1);
    expect(verified.ok).toBe(true);
    if (verified.ok) {
      expect(verified.claims.sub).toBe(EMAIL);
      expect(verified.claims.ver).toBe(1);
      expect(verified.claims.iat).toBe(Math.floor(h.clock.now() / 1000));
      expect(verified.claims.exp - verified.claims.iat).toBe(JWT_TTL_SECONDS);
    }

    // Create Contact — https://resend.com/docs/api-reference/contacts/create-contact
    expect(h.calls.map((c) => c.url)).toEqual([RESEND_EMAILS_URL, RESEND_CONTACTS_URL]);
    const contactCall = h.calls[1]!;
    expect(contactCall.init?.method).toBe("POST");
    const headers = contactCall.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer re_test_key");
    expect(headers["Content-Type"]).toBe("application/json");
    expect(jsonBody(contactCall.init)).toEqual({
      email: EMAIL,
      unsubscribed: false,
      segments: [{ id: "seg_test_audience" }],
    });
  });

  it("is single-use: the same code fails a second time", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    const first = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(first.status).toBe(200);
    const second = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(second.status).toBe(401);
    expect(await responseJson(second)).toEqual({ error: "invalid or expired code" });
  });

  it("rejects a wrong code with 401 and counts the attempt", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "999999" }), h.deps);
    expect(response.status).toBe(401);
    const record = JSON.parse((await h.kv.get(otpKey(EMAIL)))!) as OtpRecord;
    expect(record.attempts).toBe(1);
  });

  it("refuses even the correct code after 5 failed attempts", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    for (let i = 0; i < 5; i++) {
      const wrong = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: `00000${i}` }), h.deps);
      expect(wrong.status).toBe(401);
    }
    const correct = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(correct.status).toBe(429);
    expect(await responseJson(correct)).toEqual({ error: "too many attempts; request a new code" });
  });

  it("rejects a code past its 10-minute expiry", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    h.clock.advanceSeconds(601);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(401);
  });

  it("enforces logical expiry even when the KV TTL has not fired", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    const record: OtpRecord = {
      hash: await hashOtp(EMAIL, "123456"),
      attempts: 0,
      expiresAtMs: h.clock.now() - 1,
    };
    await h.kv.put(otpKey(EMAIL), JSON.stringify(record));
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(401);
    expect(h.kv.entries.has(otpKey(EMAIL))).toBe(false);
  });

  it("surfaces an audience upsert failure as audienceSync failed while auth succeeds", async () => {
    const h = makeHarness({
      fetchHandler: (url) => {
        if (url === RESEND_EMAILS_URL) return jsonRes(200, { id: "email_1" });
        if (url === RESEND_CONTACTS_URL) return jsonRes(500, { message: "contact store down" });
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(200);
    const body = await responseJson(response);
    expect(body.audienceSync).toBe("failed");
    expect(typeof body.token).toBe("string");
    expect(h.logs.some((l) => l.event === "resend_contact_create_failed")).toBe(true);
  });

  it("attaches an already-existing contact to the segment on 409", async () => {
    const attachUrl = "https://api.resend.com/contacts/user%40example.com/segments/seg_test_audience";
    const h = makeHarness({
      fetchHandler: (url) => {
        if (url === RESEND_EMAILS_URL) return jsonRes(200, { id: "email_1" });
        if (url === RESEND_CONTACTS_URL) return jsonRes(409, { message: "Contact already exists" });
        // Add Contact to Segment — https://resend.com/docs/api-reference/contacts/add-contact-to-segment
        if (url === attachUrl) return jsonRes(200, { id: "seg_test_audience" });
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).audienceSync).toBe("synced");
    expect(h.calls.map((c) => c.url)).toEqual([RESEND_EMAILS_URL, RESEND_CONTACTS_URL, attachUrl]);
    expect(h.calls[2]!.init?.method).toBe("POST");
  });

  it("reports audienceSync skipped when no audience id is configured", async () => {
    const h = makeHarness({
      env: { ...FULL_ENV, RESEND_AUDIENCE_ID: undefined },
      fetchHandler: resendHappyFetch,
    });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).audienceSync).toBe("skipped");
    expect(h.calls.map((c) => c.url)).toEqual([RESEND_EMAILS_URL]);
  });

  it("reports audienceSync failed when the contact call throws", async () => {
    const h = makeHarness({
      fetchHandler: (url) => {
        if (url === RESEND_EMAILS_URL) return jsonRes(200, { id: "email_1" });
        throw new Error("network down");
      },
    });
    await seedCode(h);
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).audienceSync).toBe("failed");
  });

  it("rejects an unknown email with 401", async () => {
    const h = makeHarness();
    const response = await handleVerify(postJson("/v1/auth/verify", { email: "nobody@example.com", code: "123456" }), h.deps);
    expect(response.status).toBe(401);
  });

  it("rejects malformed inputs with 400", async () => {
    const h = makeHarness();
    expect((await handleVerify(postJson("/v1/auth/verify", { email: "bad", code: "123456" }), h.deps)).status).toBe(400);
    expect((await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "12345" }), h.deps)).status).toBe(400);
    expect((await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: 123456 }), h.deps)).status).toBe(400);
  });

  // W-H2, the refuter's measured case: 500 concurrent wrong codes were recorded as ONE
  // attempt on disk and the correct code was then accepted. The attempt is now claimed
  // atomically BEFORE the compare, on the real filesystem store the deployment uses.
  it("claims attempts atomically: 200 concurrent wrong codes exhaust the code on disk and the correct code is refused", async () => {
    const dir = await mkdtemp(join(tmpdir(), "link-verify-race-"));
    try {
      const h = makeHarness({ fetchHandler: resendHappyFetch });
      const deps = { ...h.deps, kv: createFsKvStore(dir) };
      const seeded = await handleRequestCode(postJson("/v1/auth/request-code", { email: EMAIL }, IP), deps);
      expect(seeded.status).toBe(200);

      const guesses = Array.from({ length: 200 }, (_, i) => String(900000 + i));
      const responses = await Promise.all(
        guesses.map((code) => handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code }), deps)),
      );
      expect(responses.every((r) => r.status === 401 || r.status === 429)).toBe(true);
      expect(responses.some((r) => r.status === 200)).toBe(false);

      const onDisk = JSON.parse((await deps.kv.get(otpKey(EMAIL)))!) as OtpRecord;
      expect(onDisk.attempts).toBeGreaterThanOrEqual(OTP_MAX_ATTEMPTS);
      // Exactly the cap: the atomic claim stops spending once the cap is reached.
      expect(onDisk.attempts).toBe(OTP_MAX_ATTEMPTS);

      const correct = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), deps);
      expect(correct.status).not.toBe(200);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it(`rate limits /verify per email: the ${VERIFY_RATE_LIMIT + 1}th call in an hour is 429 with Retry-After`, async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch });
    await seedCode(h);
    for (let i = 0; i < VERIFY_RATE_LIMIT; i++) {
      const wrong = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "000000" }), h.deps);
      expect(wrong.status).not.toBe(200);
      expect((await responseJson(wrong)).error).not.toBe("rate limited");
    }
    const limited = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(limited.status).toBe(429);
    expect(limited.headers.get("Retry-After")).toBe("800");
    expect(await responseJson(limited)).toEqual({ error: "rate limited", scope: "email", retryAfterSeconds: 800 });
    expect(h.logs.some((l) => l.event === "rate_limited" && l.fields?.scope === "verify")).toBe(true);
  });

  it("the verify limiter is per email — another account is unaffected", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["123456", "654321"] });
    await seedCode(h);
    for (let i = 0; i < VERIFY_RATE_LIMIT; i++) {
      await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "000000" }), h.deps);
    }
    const other = await handleRequestCode(postJson("/v1/auth/request-code", { email: "other@example.com" }, IP), h.deps);
    expect(other.status).toBe(200);
    const ok = await handleVerify(postJson("/v1/auth/verify", { email: "other@example.com", code: "654321" }), h.deps);
    expect(ok.status).toBe(200);
  });

  it("fails loudly with 500 when LINK_JWT_SECRET is missing", async () => {
    const h = makeHarness({ env: { ...FULL_ENV, LINK_JWT_SECRET: undefined } });
    const response = await handleVerify(postJson("/v1/auth/verify", { email: EMAIL, code: "123456" }), h.deps);
    expect(response.status).toBe(500);
    expect(await responseJson(response)).toEqual({ error: "LINK_JWT_SECRET not configured on this deployment" });
  });
});

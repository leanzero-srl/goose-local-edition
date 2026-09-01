import { describe, expect, it } from "vitest";
import { handleRequestCode, otpKey } from "../src/handlers/requestCode";
import { hashOtp } from "../src/lib/otp";
import {
  FULL_ENV,
  RESEND_EMAILS_URL,
  jsonBody,
  jsonRes,
  makeHarness,
  postJson,
  resendHappyFetch,
  responseJson,
} from "./helpers";

// The proxy-set header Tailscale Funnel/Serve puts on every forwarded request (Set, not Add).
const IP = { "X-Forwarded-For": "203.0.113.7" };

describe("POST /v1/auth/request-code", () => {
  it("issues a code, stores its hash, and sends the documented Resend body", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["424242"] });
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: " User@Example.COM " }, IP), h.deps);
    expect(response.status).toBe(200);
    expect(await responseJson(response)).toEqual({ ok: true, email: "user@example.com", expiresInSeconds: 600 });

    expect(h.calls).toHaveLength(1);
    const call = h.calls[0]!;
    // Send Email API — https://resend.com/docs/api-reference/emails/send-email
    expect(call.url).toBe(RESEND_EMAILS_URL);
    expect(call.init?.method).toBe("POST");
    const headers = call.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer re_test_key");
    expect(headers["Content-Type"]).toBe("application/json");
    const body = jsonBody(call.init) as Record<string, unknown>;
    expect(Object.keys(body).sort()).toEqual(["from", "html", "subject", "text", "to"]);
    expect(body.from).toBe("LeanZero Link <link@leanzero.test>");
    expect(body.to).toBe("user@example.com");
    expect(body.subject).toBe("424242 is your LeanZero Link sign-in code");
    expect(String(body.text)).toContain("424242");
    expect(String(body.html)).toContain("424242");

    const raw = await h.kv.get(otpKey("user@example.com"));
    expect(raw).not.toBeNull();
    const record = JSON.parse(raw!) as { hash: string; attempts: number; expiresAtMs: number };
    expect(record.hash).toBe(await hashOtp("user@example.com", "424242"));
    expect(record.attempts).toBe(0);
    expect(record.expiresAtMs).toBe(h.clock.now() + 600_000);
  });

  it("rejects an invalid email with 400", async () => {
    const h = makeHarness();
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: "not-an-email" }, IP), h.deps);
    expect(response.status).toBe(400);
    expect(h.calls).toHaveLength(0);
  });

  it("rejects a non-JSON body with 400", async () => {
    const h = makeHarness();
    const request = new Request("https://link.example/v1/auth/request-code", { method: "POST", body: "not json" });
    const response = await handleRequestCode(request, h.deps);
    expect(response.status).toBe(400);
  });

  it("returns 501 when mail is not configured", async () => {
    const h = makeHarness({ env: { ...FULL_ENV, RESEND_API_KEY: undefined } });
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
    expect(response.status).toBe(501);
    expect(await responseJson(response)).toEqual({ error: "mail not configured on this deployment" });
    expect(h.calls).toHaveLength(0);
  });

  it("returns 502 and stores no code when Resend refuses the send", async () => {
    const h = makeHarness({ fetchHandler: () => jsonRes(500, { message: "boom" }) });
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "failed to send code email", status: 500 });
    expect(await h.kv.get(otpKey("user@example.com"))).toBeNull();
  });

  it("rate limits the 4th request per email in an hour with Retry-After", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["111111", "222222", "333333"] });
    for (let i = 0; i < 3; i++) {
      const ok = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
      expect(ok.status).toBe(200);
    }
    const limited = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
    expect(limited.status).toBe(429);
    // FakeClock starts at 1_756_000_000s = 2800s into its hour window → 800s remain.
    expect(limited.headers.get("Retry-After")).toBe("800");
    expect(await responseJson(limited)).toEqual({ error: "rate limited", scope: "email", retryAfterSeconds: 800 });
  });

  it("allows the same email again after the window rolls over", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["111111", "222222", "333333", "444444"] });
    for (let i = 0; i < 3; i++) {
      await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
    }
    h.clock.advanceSeconds(3600);
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }, IP), h.deps);
    expect(response.status).toBe(200);
  });

  it("rate limits the 11th request per IP in an hour", async () => {
    const h = makeHarness({
      fetchHandler: resendHappyFetch,
      otp: Array.from({ length: 10 }, (_, i) => String(100000 + i)),
    });
    for (let i = 0; i < 10; i++) {
      const ok = await handleRequestCode(postJson("/v1/auth/request-code", { email: `user${i}@example.com` }, IP), h.deps);
      expect(ok.status).toBe(200);
    }
    const limited = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user10@example.com" }, IP), h.deps);
    expect(limited.status).toBe(429);
    expect(limited.headers.get("Retry-After")).toBe("800");
    expect(await responseJson(limited)).toEqual({ error: "rate limited", scope: "ip", retryAfterSeconds: 800 });
  });

  // W-H1, the refuter's measured case: 15 requests from 15 distinct clients. Before the fix
  // every one of them keyed on rl:ip:unknown (10/h) — a GLOBAL sign-in lockout at the 11th.
  it("keys the ip bucket on the proxy-set X-Forwarded-For: 15 distinct clients → 15×200", async () => {
    const h = makeHarness({
      fetchHandler: resendHappyFetch,
      otp: Array.from({ length: 15 }, (_, i) => String(100000 + i)),
    });
    for (let i = 0; i < 15; i++) {
      const response = await handleRequestCode(
        postJson("/v1/auth/request-code", { email: `user${i}@example.com` }, { "X-Forwarded-For": `198.51.100.${i + 1}` }),
        h.deps,
      );
      expect(response.status).toBe(200);
    }
    const ipKeys = [...h.kv.entries.keys()].filter((k) => k.startsWith("rl:ip:"));
    expect(ipKeys).toHaveLength(15);
    expect(ipKeys.some((k) => k.includes("unknown"))).toBe(false);
  });

  it("uses the RIGHTMOST X-Forwarded-For value, so a client-prepended address cannot rotate buckets", async () => {
    const h = makeHarness({
      fetchHandler: resendHappyFetch,
      otp: Array.from({ length: 10 }, (_, i) => String(100000 + i)),
    });
    for (let i = 0; i < 10; i++) {
      // Spoofed left-hand values; the trusted proxy's own value stays rightmost.
      const forwarded = `10.0.0.${i}, 203.0.113.7`;
      const ok = await handleRequestCode(
        postJson("/v1/auth/request-code", { email: `user${i}@example.com` }, { "X-Forwarded-For": forwarded }),
        h.deps,
      );
      expect(ok.status).toBe(200);
    }
    const limited = await handleRequestCode(
      postJson("/v1/auth/request-code", { email: "user10@example.com" }, { "X-Forwarded-For": "10.0.0.99, 203.0.113.7" }),
      h.deps,
    );
    expect(limited.status).toBe(429);
    expect((await responseJson(limited)).scope).toBe("ip");
    expect([...h.kv.entries.keys()].filter((k) => k.startsWith("rl:ip:"))).toEqual([
      expect.stringMatching(/^rl:ip:203\.0\.113\.7:/),
    ]);
  });

  it("with no resolvable client address it logs client_ip_unresolved and skips the ip scope — never an 'unknown' bucket", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["424242"] });
    const response = await handleRequestCode(postJson("/v1/auth/request-code", { email: "user@example.com" }), h.deps);
    expect(response.status).toBe(200);
    const unresolved = h.logs.find((l) => l.event === "client_ip_unresolved");
    expect(unresolved?.fields).toEqual({ path: "/v1/auth/request-code", forwardedFor: null });
    expect([...h.kv.entries.keys()].some((k) => k.startsWith("rl:ip:"))).toBe(false);
  });

  it("falls back to CF-Connecting-IP only when there is no X-Forwarded-For at all", async () => {
    const h = makeHarness({ fetchHandler: resendHappyFetch, otp: ["424242", "424243"] });
    await handleRequestCode(
      postJson("/v1/auth/request-code", { email: "a@example.com" }, { "CF-Connecting-IP": "203.0.113.50" }),
      h.deps,
    );
    await handleRequestCode(
      postJson("/v1/auth/request-code", { email: "b@example.com" }, { "CF-Connecting-IP": "203.0.113.50", "X-Forwarded-For": "203.0.113.51" }),
      h.deps,
    );
    const ipKeys = [...h.kv.entries.keys()].filter((k) => k.startsWith("rl:ip:")).sort();
    expect(ipKeys).toEqual([expect.stringMatching(/^rl:ip:203\.0\.113\.50:/), expect.stringMatching(/^rl:ip:203\.0\.113\.51:/)]);
  });

  it("checks the ip bucket BEFORE the email bucket, so an exhausted source cannot burn a victim's email budget", async () => {
    const h = makeHarness({
      fetchHandler: resendHappyFetch,
      otp: Array.from({ length: 10 }, (_, i) => String(100000 + i)),
    });
    for (let i = 0; i < 10; i++) {
      await handleRequestCode(postJson("/v1/auth/request-code", { email: `user${i}@example.com` }, IP), h.deps);
    }
    const refused = await handleRequestCode(postJson("/v1/auth/request-code", { email: "victim@example.com" }, IP), h.deps);
    expect(refused.status).toBe(429);
    expect((await responseJson(refused)).scope).toBe("ip");
    expect([...h.kv.entries.keys()].some((k) => k.startsWith("rl:email:victim@example.com"))).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import { JWT_TTL_SECONDS } from "../src/lib/config";
import { signJwt, verifyJwt, type JwtClaims } from "../src/lib/jwt";

const SECRET = "unit-test-jwt-secret-with-at-least-32-bytes";
const NOW = 1_756_000_000;

function claims(overrides: Partial<JwtClaims> = {}): JwtClaims {
  return { sub: "user@example.com", iat: NOW, exp: NOW + JWT_TTL_SECONDS, ver: 1, ...overrides };
}

describe("jwt round trip", () => {
  it("signs and verifies with the full claim set", async () => {
    const token = await signJwt(SECRET, claims());
    const result = await verifyJwt(SECRET, token, NOW + 10);
    expect(result).toEqual({ ok: true, claims: claims() });
  });

  it("keeps a 180-day expiry", () => {
    expect(JWT_TTL_SECONDS).toBe(180 * 86400);
    const c = claims();
    expect(c.exp - c.iat).toBe(180 * 86400);
  });

  it("rejects an expired token", async () => {
    const token = await signJwt(SECRET, claims());
    const result = await verifyJwt(SECRET, token, NOW + JWT_TTL_SECONDS);
    expect(result).toEqual({ ok: false, reason: "expired" });
  });

  it("rejects a token signed with another secret", async () => {
    const token = await signJwt("some-other-secret", claims());
    const result = await verifyJwt(SECRET, token, NOW + 10);
    expect(result).toEqual({ ok: false, reason: "bad_signature" });
  });

  it("rejects a tampered payload", async () => {
    const token = await signJwt(SECRET, claims());
    const parts = token.split(".");
    const forged = JSON.parse(atob(parts[1]!.replace(/-/g, "+").replace(/_/g, "/"))) as JwtClaims;
    forged.sub = "attacker@example.com";
    const forgedPayload = btoa(JSON.stringify(forged)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
    const result = await verifyJwt(SECRET, `${parts[0]}.${forgedPayload}.${parts[2]}`, NOW + 10);
    expect(result).toEqual({ ok: false, reason: "bad_signature" });
  });

  it("rejects malformed tokens", async () => {
    expect(await verifyJwt(SECRET, "garbage", NOW)).toEqual({ ok: false, reason: "malformed" });
    expect(await verifyJwt(SECRET, "a.b", NOW)).toEqual({ ok: false, reason: "malformed" });
  });

  it("rejects a claim-set without ver=1", async () => {
    const token = await signJwt(SECRET, claims({ ver: 2 }));
    const result = await verifyJwt(SECRET, token, NOW + 10);
    expect(result).toEqual({ ok: false, reason: "bad_claims" });
  });
});

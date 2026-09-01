import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { handleJoinKey } from "../src/handlers/joinKey";
import { handleRequestCode } from "../src/handlers/requestCode";
import { handleVerify } from "../src/handlers/verify";
import { JWT_TTL_SECONDS } from "../src/lib/config";
import { createFsKvStore } from "../src/lib/fs-kv";
import { signJwt } from "../src/lib/jwt";
import { ensureNodeSecret, nodeSecretKey } from "../src/lib/nodeSecret";
import {
  TAILSCALE_CREATE_KEY_FIXTURE,
  jsonRes,
  makeHarness,
  postJson,
  resendHappyFetch,
  responseJson,
  type TestHarness,
} from "./helpers";

const IP = { "X-Forwarded-For": "203.0.113.7" };
const KEYS_URL = "https://api.tailscale.com/api/v2/tailnet/example.ts.net/keys";
const HEX64 = /^[0-9a-f]{64}$/;

function fullFetch(url: string): Response {
  if (url === KEYS_URL) return jsonRes(200, TAILSCALE_CREATE_KEY_FIXTURE);
  return resendHappyFetch(url, undefined) as Response;
}

async function signIn(h: TestHarness, email: string, code: string): Promise<string> {
  const requested = await handleRequestCode(postJson("/v1/auth/request-code", { email }, IP), h.deps);
  expect(requested.status).toBe(200);
  const verified = await handleVerify(postJson("/v1/auth/verify", { email, code }), h.deps);
  expect(verified.status).toBe(200);
  const body = await responseJson(verified);
  expect(body.nodeSecret).toMatch(HEX64);
  return String(body.nodeSecret);
}

async function joinKeyFor(h: TestHarness, email: string): Promise<Record<string, unknown>> {
  const iat = Math.floor(h.clock.now() / 1000);
  const token = await signJwt("unit-test-jwt-secret-with-at-least-32-bytes", { sub: email, iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
  const response = await handleJoinKey(
    new Request("https://link.example/v1/mesh/join-key", { method: "POST", headers: { Authorization: `Bearer ${token}` } }),
    h.deps,
  );
  expect(response.status).toBe(200);
  return responseJson(response);
}

describe("nodeSecret — per-account, stable, minted once (R-H2)", () => {
  it("is the SAME value on /verify and on every /mesh/join-key for the account", async () => {
    const h = makeHarness({ fetchHandler: fullFetch, otp: ["111111"] });
    const fromVerify = await signIn(h, "alice@example.com", "111111");
    const first = await joinKeyFor(h, "alice@example.com");
    const second = await joinKeyFor(h, "alice@example.com");
    expect(first.nodeSecret).toBe(fromVerify);
    expect(second.nodeSecret).toBe(fromVerify);
    expect(await h.kv.get(nodeSecretKey("alice@example.com"))).toBe(fromVerify);
  });

  it("survives a re-login (a second device's /verify returns the same secret)", async () => {
    const h = makeHarness({ fetchHandler: fullFetch, otp: ["111111", "222222"] });
    const laptop = await signIn(h, "alice@example.com", "111111");
    h.clock.advanceSeconds(3600 * 24 * 30);
    const phone = await signIn(h, "alice@example.com", "222222");
    expect(phone).toBe(laptop);
  });

  it("differs between accounts", async () => {
    const h = makeHarness({ fetchHandler: fullFetch, otp: ["111111", "222222"] });
    const alice = await signIn(h, "alice@example.com", "111111");
    const bob = await signIn(h, "bob@example.com", "222222");
    expect(alice).not.toBe(bob);
  });

  it("is stored WITHOUT a TTL — it must outlive OTPs, rate windows and the identity JWT", async () => {
    const h = makeHarness({ fetchHandler: fullFetch, otp: ["111111"] });
    const secret = await signIn(h, "alice@example.com", "111111");
    expect(h.kv.entries.get(nodeSecretKey("alice@example.com"))?.expiresAtMs).toBeNull();
    h.clock.advanceSeconds(10 * 365 * 86400);
    expect(await h.kv.get(nodeSecretKey("alice@example.com"))).toBe(secret);
  });

  it("converges when two devices race the very first mint (filesystem store, 50 concurrent)", async () => {
    const dir = await mkdtemp(join(tmpdir(), "link-nodesecret-"));
    try {
      const kv = createFsKvStore(dir);
      const secrets = await Promise.all(Array.from({ length: 50 }, () => ensureNodeSecret(kv, "alice@example.com", () => {})));
      expect(new Set(secrets).size).toBe(1);
      expect(secrets[0]).toMatch(HEX64);
      expect(await kv.get(nodeSecretKey("alice@example.com"))).toBe(secrets[0]);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("re-mints loudly when the stored value is not a 64-hex secret", async () => {
    const h = makeHarness({ fetchHandler: fullFetch });
    await h.kv.put(nodeSecretKey("alice@example.com"), "not-a-secret");
    const secret = await ensureNodeSecret(h.deps.kv, "alice@example.com", h.deps.log);
    expect(secret).toMatch(HEX64);
    expect(h.logs.some((l) => l.event === "node_secret_corrupt")).toBe(true);
    expect(h.logs.some((l) => l.event === "node_secret_minted")).toBe(true);
  });

  it("is not returned when the mint fails — no secret rides a 502", async () => {
    const h = makeHarness({ fetchHandler: (url) => (url === KEYS_URL ? jsonRes(500, { message: "internal" }) : fullFetch(url)) });
    const iat = Math.floor(h.clock.now() / 1000);
    const token = await signJwt("unit-test-jwt-secret-with-at-least-32-bytes", { sub: "alice@example.com", iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
    const response = await handleJoinKey(
      new Request("https://link.example/v1/mesh/join-key", { method: "POST", headers: { Authorization: `Bearer ${token}` } }),
      h.deps,
    );
    expect(response.status).toBe(502);
    expect(await responseJson(response)).not.toHaveProperty("nodeSecret");
    expect(h.kv.entries.has(nodeSecretKey("alice@example.com"))).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import { handleJoinKey } from "../src/handlers/joinKey";
import { JWT_TTL_SECONDS } from "../src/lib/config";
import { policyIsolates, usernameForEmail } from "../src/lib/headscale";
import { signJwt } from "../src/lib/jwt";
import {
  jsonBody,
  jsonRes,
  makeHarness,
  responseJson,
  type FetchHandler,
  type TestHarness,
} from "./helpers";

const EMAIL = "user@example.com";
const HS_ENV = {
  LINK_JWT_SECRET: "unit-test-jwt-secret-with-at-least-32-bytes",
  HEADSCALE_API_URL: "http://hs.test",
  HEADSCALE_API_KEY: "hskey-api-test",
  HEADSCALE_LOGIN_SERVER: "https://control.leanzero.test",
} as const;

const POLICY_URL = "http://hs.test/api/v1/policy";
const USER_URL = "http://hs.test/api/v1/user";
const PREAUTH_URL = "http://hs.test/api/v1/preauthkey";

const ISOLATED_POLICY = {
  policy: '{"acls":[{"action":"accept","src":["*"],"dst":["autogroup:self:*"]}]}',
  updatedAt: "2026-09-01T00:00:00Z",
};
const ALLOW_ALL_POLICY = {
  policy: '{"acls":[{"action":"accept","src":["*"],"dst":["*:*"]}]}',
  updatedAt: "2026-09-01T00:00:00Z",
};
// W-M3's measured case: the self rule PLUS an allow-all rule passed `.some()` and minted.
const SELF_PLUS_ALLOW_ALL_POLICY = {
  policy:
    '{"acls":[{"action":"accept","src":["*"],"dst":["autogroup:self:*"]},{"action":"accept","src":["*"],"dst":["*:*"]}]}',
  updatedAt: "2026-09-01T00:00:00Z",
};
// W-M4's measured case: an isolating policy written as HuJSON (comments, trailing comma)
// failed JSON.parse, was judged "wrong", and was overwritten.
const HUJSON_ISOLATED_POLICY = {
  policy:
    "// LeanZero Link isolation policy\n{\n  /* one rule: a node reaches only its own account */\n" +
    '  "acls": [\n    { "action": "accept", "src": ["*"], "dst": ["autogroup:self:*"], },\n  ],\n}\n',
  updatedAt: "2026-09-01T00:00:00Z",
};
const UNPARSEABLE_POLICY = { policy: '{"acls": [ {"action": "accept", "src": ["*"', updatedAt: "2026-09-01T00:00:00Z" };
const EMPTY_POLICY = { policy: "", updatedAt: "2026-09-01T00:00:00Z" };
const SSH_POLICY = {
  policy:
    '{"acls":[{"action":"accept","src":["*"],"dst":["autogroup:self:*"]}],"ssh":[{"action":"accept","src":["*"],"dst":["*"],"users":["root"]}]}',
  updatedAt: "2026-09-01T00:00:00Z",
};

async function mintToken(h: TestHarness): Promise<string> {
  const iat = Math.floor(h.clock.now() / 1000);
  return signJwt("unit-test-jwt-secret-with-at-least-32-bytes", { sub: EMAIL, iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
}

function joinRequest(token: string): Request {
  return new Request("https://link.example/v1/mesh/join-key", {
    method: "POST",
    headers: { Authorization: `Bearer ${token}` },
  });
}

function userName(url: string): string | null {
  return new URL(url).searchParams.get("name");
}

/// A stateful fake of the Headscale REST API, seeded with the users it already knows.
function hsApi(opts: {
  policy?: unknown;
  users?: Array<{ id: string; name: string }>;
  key?: string;
  onPreauth?: (body: unknown) => Response;
  onCreateUser?: () => Response;
}): FetchHandler {
  const users = [...(opts.users ?? [])];
  return (url, init) => {
    const method = (init?.method ?? "GET").toUpperCase();
    if (url.startsWith(POLICY_URL) && method === "GET") {
      return jsonRes(200, opts.policy ?? ISOLATED_POLICY);
    }
    if (url.startsWith(POLICY_URL) && method === "PUT") {
      return jsonRes(200, { policy: (jsonBody(init) as { policy: string }).policy });
    }
    if (url.startsWith(USER_URL) && method === "GET") {
      const name = userName(url);
      return jsonRes(200, { users: users.filter((u) => u.name === name) });
    }
    if (url === USER_URL && method === "POST") {
      if (opts.onCreateUser) return opts.onCreateUser();
      const name = (jsonBody(init) as { name: string }).name;
      const created = { id: String(users.length + 100), name };
      users.push(created);
      return jsonRes(200, { user: created });
    }
    if (url === PREAUTH_URL && method === "POST") {
      if (opts.onPreauth) return opts.onPreauth(jsonBody(init));
      return jsonRes(200, { preAuthKey: { key: opts.key ?? "hskey-auth-DEFAULT", used: false } });
    }
    throw new Error(`unexpected fetch: ${method} ${url}`);
  };
}

describe("POST /v1/mesh/join-key — Headscale (multi-tenant)", () => {
  it("mints a per-account ephemeral key and returns it with the public login server", async () => {
    const h = makeHarness({ env: HS_ENV, fetchHandler: hsApi({ key: "hskey-auth-ABC" }) });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(await responseJson(response)).toEqual({
      authKey: "hskey-auth-ABC",
      loginServer: "https://control.leanzero.test",
      expirySeconds: 600,
      nodeSecret: expect.stringMatching(/^[0-9a-f]{64}$/),
    });

    // The preauth mint uses the numeric user id, ephemeral + single-use, with an RFC3339
    // expiration derived from now + expirySeconds, and the API bearer key.
    const preauth = h.calls.find((c) => c.url === PREAUTH_URL)!;
    expect(preauth.init?.method).toBe("POST");
    expect((preauth.init?.headers as Record<string, string>).Authorization).toBe("Bearer hskey-api-test");
    expect(jsonBody(preauth.init)).toEqual({
      user: "100",
      reusable: false,
      ephemeral: true,
      expiration: new Date(h.clock.now() + 600 * 1000).toISOString(),
    });
  });

  it("uses the account's stable per-email Headscale user, creating it once", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({ env: HS_ENV, fetchHandler: hsApi({ key: "hskey-auth-1" }) });
    await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    // Looked up by the derived name, and created because absent.
    const lookup = h.calls.find((c) => c.url.startsWith(`${USER_URL}?`))!;
    expect(userName(lookup.url)).toBe(username);
    expect(username).toMatch(/^acct-[0-9a-f]{16}$/);
    const create = h.calls.find((c) => c.url === USER_URL && (c.init?.method ?? "GET") === "POST")!;
    expect(jsonBody(create.init)).toEqual({ name: username });
  });

  it("does NOT create the user when it already exists", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ users: [{ id: "42", name: username }], key: "hskey-auth-2" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(h.calls.some((c) => c.url === USER_URL && (c.init?.method ?? "GET") === "POST")).toBe(false);
    expect(jsonBody(h.calls.find((c) => c.url === PREAUTH_URL)!.init)).toMatchObject({ user: "42" });
  });

  it("self-heals the isolation policy before minting into an allow-all server", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ policy: ALLOW_ALL_POLICY, users: [{ id: "5", name: username }], key: "hskey-auth-3" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    const put = h.calls.find((c) => c.url === POLICY_URL && c.init?.method === "PUT");
    expect(put).toBeDefined();
    expect((jsonBody(put!.init) as { policy: string }).policy).toContain("autogroup:self");
  });

  it("heals a policy whose self rule sits beside an allow-all rule — every rule must isolate", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ policy: SELF_PLUS_ALLOW_ALL_POLICY, users: [{ id: "5", name: username }], key: "hskey-auth-4" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    const put = h.calls.find((c) => c.url === POLICY_URL && c.init?.method === "PUT");
    expect(put).toBeDefined();
    const healed = JSON.parse((jsonBody(put!.init) as { policy: string }).policy) as unknown;
    expect(policyIsolates(healed)).toBe(true);
  });

  it("heals a policy that isolates in its acls but opens an ssh section", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ policy: SSH_POLICY, users: [{ id: "5", name: username }], key: "hskey-auth-5" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(h.calls.some((c) => c.url === POLICY_URL && c.init?.method === "PUT")).toBe(true);
  });

  it("accepts an isolating policy written as HuJSON (comments, trailing commas) WITHOUT overwriting it", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ policy: HUJSON_ISOLATED_POLICY, users: [{ id: "5", name: username }], key: "hskey-auth-6" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(h.calls.some((c) => c.url === POLICY_URL && c.init?.method === "PUT")).toBe(false);
    expect(h.logs.some((l) => l.event === "headscale_policy_healing")).toBe(false);
  });

  it("REFUSES the mint (502) on a policy it cannot parse — never overwrites what it could not read", async () => {
    const h = makeHarness({ env: HS_ENV, fetchHandler: hsApi({ policy: UNPARSEABLE_POLICY, key: "hskey-auth-7" }) });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 200 });
    expect(h.calls.some((c) => c.url === POLICY_URL && c.init?.method === "PUT")).toBe(false);
    expect(h.calls.some((c) => c.url === PREAUTH_URL)).toBe(false);
    expect(h.logs.some((l) => l.event === "headscale_policy_unparseable")).toBe(true);
  });

  it("sets the isolation policy on a server that has none yet (empty policy text)", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({ policy: EMPTY_POLICY, users: [{ id: "5", name: username }], key: "hskey-auth-8" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    const put = h.calls.find((c) => c.url === POLICY_URL && c.init?.method === "PUT");
    expect(put).toBeDefined();
  });

  it("policyIsolates: every accept rule must be self-only; ssh/grants must be absent or empty", () => {
    const self = { action: "accept", src: ["*"], dst: ["autogroup:self:*"] };
    expect(policyIsolates({ acls: [self] })).toBe(true);
    expect(policyIsolates({ acls: [self], ssh: [], grants: [] })).toBe(true);
    expect(policyIsolates({ acls: [{ ...self, dst: ["autogroup:self:22", "autogroup:self"] }] })).toBe(true);
    expect(policyIsolates({ acls: [] })).toBe(false);
    expect(policyIsolates({ acls: [self, { action: "accept", src: ["*"], dst: ["*:*"] }] })).toBe(false);
    expect(policyIsolates({ acls: [{ ...self, dst: ["autogroup:self:*", "10.0.0.0/8:*"] }] })).toBe(false);
    expect(policyIsolates({ acls: [{ ...self, dst: ["autogroup:selfish:*"] }] })).toBe(false);
    expect(policyIsolates({ acls: [self], grants: [{ src: ["*"], dst: ["*"], ip: ["*"] }] })).toBe(false);
    expect(policyIsolates({ acls: [self], ssh: [{ action: "accept", src: ["*"], dst: ["*"] }] })).toBe(false);
    expect(policyIsolates(null)).toBe(false);
    expect(policyIsolates("nope")).toBe(false);
  });

  // W-M6: a throwing fetch (Headscale down) escaped hsFetch and became the router's 500.
  it("answers 502 status 0 + headscale_unreachable when Headscale cannot be reached — not a 500", async () => {
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: () => {
        throw new TypeError("fetch failed: connect ECONNREFUSED 127.0.0.1:8790");
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 0 });
    expect(h.logs.find((l) => l.event === "headscale_unreachable")?.fields).toEqual({
      path: "/api/v1/policy",
      error: "fetch failed: connect ECONNREFUSED 127.0.0.1:8790",
    });
    expect(h.logs.some((l) => l.event === "unhandled_error")).toBe(false);
  });

  it("answers 502 when Headscale drops mid-flow (policy ok, preauth mint unreachable)", async () => {
    const username = await usernameForEmail(EMAIL);
    const happy = hsApi({ users: [{ id: "5", name: username }] });
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: (url, init) => {
        if (url === PREAUTH_URL) throw new Error("socket hang up");
        return happy(url, init);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 0 });
    expect(h.logs.find((l) => l.event === "headscale_unreachable")?.fields).toMatchObject({ path: "/api/v1/preauthkey" });
  });

  it("fails loudly (502) when Headscale refuses the mint — never a fabricated key", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({
        users: [{ id: "5", name: username }],
        onPreauth: () => jsonRes(500, { message: "internal" }),
      }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 500 });
  });

  it("returns 502 when a 2xx mint carries no key — never fabricates one", async () => {
    const username = await usernameForEmail(EMAIL);
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: hsApi({
        users: [{ id: "5", name: username }],
        onPreauth: () => jsonRes(200, { preAuthKey: { used: false } }),
      }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 200 });
  });

  it("converges on the winner when a concurrent create raced (UNIQUE constraint)", async () => {
    const username = await usernameForEmail(EMAIL);
    let created = false;
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: (url, init) => {
        const method = (init?.method ?? "GET").toUpperCase();
        if (url.startsWith(POLICY_URL) && method === "GET") return jsonRes(200, ISOLATED_POLICY);
        if (url.startsWith(USER_URL) && method === "GET") {
          // Absent on the first read; the racing create "wins" and appears on re-read.
          return jsonRes(200, { users: created ? [{ id: "77", name: username }] : [] });
        }
        if (url === USER_URL && method === "POST") {
          created = true;
          return jsonRes(500, { message: "UNIQUE constraint failed: users.name" });
        }
        if (url === PREAUTH_URL && method === "POST") return jsonRes(200, { preAuthKey: { key: "hskey-auth-race" } });
        throw new Error(`unexpected fetch: ${method} ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).authKey).toBe("hskey-auth-race");
    expect(jsonBody(h.calls.find((c) => c.url === PREAUTH_URL)!.init)).toMatchObject({ user: "77" });
  });

  it("fails the mint loudly (502) if the policy cannot even be read", async () => {
    const h = makeHarness({
      env: HS_ENV,
      fetchHandler: (url, init) => {
        const method = (init?.method ?? "GET").toUpperCase();
        if (url.startsWith(POLICY_URL) && method === "GET") return jsonRes(503, { message: "down" });
        throw new Error(`unexpected fetch: ${method} ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "headscale key mint failed", status: 503 });
  });

  it("prefers Headscale over Tailscale when both are configured", async () => {
    const h = makeHarness({
      env: { ...HS_ENV, TS_API_TOKEN: "tskey-api-x", TS_TAILNET: "example.ts.net" },
      fetchHandler: hsApi({ key: "hskey-auth-pref" }),
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    // No Tailscale API call was made — the fake only knows Headscale URLs.
    expect((await responseJson(response)).authKey).toBe("hskey-auth-pref");
    expect(h.calls.every((c) => c.url.startsWith("http://hs.test"))).toBe(true);
  });

  it("derives a stable, PII-free username from the email", async () => {
    const a = await usernameForEmail("Alice@Example.com");
    const b = await usernameForEmail("alice@example.com  ");
    expect(a).toBe(b); // case- and whitespace-normalized
    expect(a).toMatch(/^acct-[0-9a-f]{16}$/);
    expect(a).not.toContain("alice");
  });
});

import { describe, expect, it } from "vitest";
import { handleJoinKey } from "../src/handlers/joinKey";
import { JWT_TTL_SECONDS } from "../src/lib/config";
import { usernameForEmail } from "../src/lib/headscale";
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
  LINK_JWT_SECRET: "unit-test-jwt-secret",
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

async function mintToken(h: TestHarness): Promise<string> {
  const iat = Math.floor(h.clock.now() / 1000);
  return signJwt("unit-test-jwt-secret", { sub: EMAIL, iat, exp: iat + JWT_TTL_SECONDS, ver: 1 });
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

import { describe, expect, it } from "vitest";
import { handleJoinKey } from "../src/handlers/joinKey";
import { JWT_TTL_SECONDS } from "../src/lib/config";
import { signJwt } from "../src/lib/jwt";
import {
  FULL_ENV,
  TAILSCALE_CREATE_KEY_FIXTURE,
  jsonBody,
  jsonRes,
  makeHarness,
  responseJson,
  type TestHarness,
} from "./helpers";

const EMAIL = "user@example.com";
const KEYS_URL = "https://api.tailscale.com/api/v2/tailnet/example.ts.net/keys";
const OAUTH_URL = "https://api.tailscale.com/api/v2/oauth/token";

async function mintToken(h: TestHarness, overrides: { secret?: string } = {}): Promise<string> {
  const iat = Math.floor(h.clock.now() / 1000);
  return signJwt(overrides.secret ?? "unit-test-jwt-secret", {
    sub: EMAIL,
    iat,
    exp: iat + JWT_TTL_SECONDS,
    ver: 1,
  });
}

function joinRequest(token?: string): Request {
  return new Request("https://link.example/v1/mesh/join-key", {
    method: "POST",
    headers: token === undefined ? {} : { Authorization: `Bearer ${token}` },
  });
}

describe("POST /v1/mesh/join-key", () => {
  it("returns 501 explicitly when Tailscale is not configured — never a dummy key", async () => {
    const h = makeHarness({ env: { ...FULL_ENV, TS_API_TOKEN: undefined, TS_TAILNET: undefined } });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(501);
    expect(await responseJson(response)).toEqual({ error: "mesh keys not configured on this deployment" });
    expect(h.calls).toHaveLength(0);
  });

  it("rejects a missing bearer token with 401", async () => {
    const h = makeHarness();
    const response = await handleJoinKey(joinRequest(), h.deps);
    expect(response.status).toBe(401);
    expect(await responseJson(response)).toEqual({ error: "missing bearer token" });
  });

  it("rejects a token signed with another secret with 401", async () => {
    const h = makeHarness();
    const response = await handleJoinKey(joinRequest(await mintToken(h, { secret: "wrong" })), h.deps);
    expect(response.status).toBe(401);
    expect(await responseJson(response)).toEqual({ error: "invalid token", reason: "bad_signature" });
  });

  it("rejects an expired token with 401", async () => {
    const h = makeHarness();
    const token = await mintToken(h);
    h.clock.advanceSeconds(JWT_TTL_SECONDS + 1);
    const response = await handleJoinKey(joinRequest(token), h.deps);
    expect(response.status).toBe(401);
    expect(await responseJson(response)).toEqual({ error: "invalid token", reason: "expired" });
  });

  it("mints an ephemeral preauthorized tagged key with the exact documented request", async () => {
    const h = makeHarness({
      fetchHandler: (url) => {
        // Create key — https://tailscale.com/api (POST /api/v2/tailnet/{tailnet}/keys)
        if (url === KEYS_URL) return jsonRes(200, TAILSCALE_CREATE_KEY_FIXTURE);
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(await responseJson(response)).toEqual({
      authKey: TAILSCALE_CREATE_KEY_FIXTURE.key,
      expirySeconds: 600,
    });

    expect(h.calls).toHaveLength(1);
    const call = h.calls[0]!;
    expect(call.url).toBe(KEYS_URL);
    expect(call.init?.method).toBe("POST");
    const headers = call.init?.headers as Record<string, string>;
    // "the username portion of HTTP Basic authentication (leave the password blank)"
    expect(headers.Authorization).toBe(`Basic ${btoa("tskey-api-test-token:")}`);
    expect(headers["Content-Type"]).toBe("application/json");
    expect(jsonBody(call.init)).toEqual({
      capabilities: {
        devices: {
          create: {
            reusable: false,
            ephemeral: true,
            preauthorized: true,
            tags: ["tag:leanzero-link"],
          },
        },
      },
      expirySeconds: 600,
      description: "leanzero-link join key for user@example.com",
    });
  });

  it("mints UNTAGGED with no tags field when TS_NODE_TAG is explicitly empty", async () => {
    const h = makeHarness({
      env: { ...FULL_ENV, TS_NODE_TAG: "" },
      fetchHandler: (url) => {
        if (url === KEYS_URL) return jsonRes(200, TAILSCALE_CREATE_KEY_FIXTURE);
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(await responseJson(response)).toEqual({
      authKey: TAILSCALE_CREATE_KEY_FIXTURE.key,
      expirySeconds: 600,
    });

    expect(h.calls).toHaveLength(1);
    // The mint body must OMIT `tags` entirely — an untagged key, not tags:[] or tags:[""].
    const create = (
      jsonBody(h.calls[0]!.init) as {
        capabilities: { devices: { create: Record<string, unknown> } };
      }
    ).capabilities.devices.create;
    expect(create).not.toHaveProperty("tags");
    expect(create).toEqual({
      reusable: false,
      ephemeral: true,
      preauthorized: true,
    });
  });

  it("exchanges an OAuth client id:secret pair for an access token first", async () => {
    const h = makeHarness({
      env: { ...FULL_ENV, TS_API_TOKEN: "clientid123:supersecret456" },
      fetchHandler: (url, init) => {
        // Token exchange — https://tailscale.com/kb/1215/oauth-clients
        if (url === OAUTH_URL) {
          const form = new URLSearchParams(String(init?.body));
          expect(form.get("grant_type")).toBe("client_credentials");
          expect(form.get("client_id")).toBe("clientid123");
          expect(form.get("client_secret")).toBe("supersecret456");
          return jsonRes(200, { access_token: "at-test-1", token_type: "Bearer", expires_in: 3600 });
        }
        if (url === KEYS_URL) return jsonRes(200, TAILSCALE_CREATE_KEY_FIXTURE);
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect(h.calls.map((c) => c.url)).toEqual([OAUTH_URL, KEYS_URL]);
    const keysHeaders = h.calls[1]!.init?.headers as Record<string, string>;
    expect(keysHeaders.Authorization).toBe("Bearer at-test-1");
  });

  it("returns 502 when the OAuth exchange is refused", async () => {
    const h = makeHarness({
      env: { ...FULL_ENV, TS_API_TOKEN: "clientid123:badsecret" },
      fetchHandler: (url) => {
        if (url === OAUTH_URL) return jsonRes(401, { message: "invalid client" });
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "tailscale key mint failed", status: 401 });
  });

  it("returns 502 when Tailscale refuses the mint", async () => {
    const h = makeHarness({
      fetchHandler: (url) => {
        if (url === KEYS_URL) return jsonRes(500, { message: "internal" });
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "tailscale key mint failed", status: 500 });
  });

  it("returns 502 when a 2xx response carries no key — never fabricates one", async () => {
    const h = makeHarness({
      fetchHandler: (url) => {
        if (url === KEYS_URL) return jsonRes(200, { id: "k1" });
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(502);
    expect(await responseJson(response)).toEqual({ error: "tailscale key mint failed", status: 200 });
  });

  it("honors TS_NODE_TAG and TS_KEY_EXPIRY_SECONDS", async () => {
    const h = makeHarness({
      env: { ...FULL_ENV, TS_NODE_TAG: "tag:custom-fleet", TS_KEY_EXPIRY_SECONDS: "900" },
      fetchHandler: (url) => {
        if (url === KEYS_URL) return jsonRes(200, TAILSCALE_CREATE_KEY_FIXTURE);
        throw new Error(`unexpected fetch: ${url}`);
      },
    });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).expirySeconds).toBe(900);
    const body = jsonBody(h.calls[0]!.init) as { capabilities: { devices: { create: { tags: string[] } } }; expirySeconds: number };
    expect(body.capabilities.devices.create.tags).toEqual(["tag:custom-fleet"]);
    expect(body.expirySeconds).toBe(900);
  });

  it("fails loudly with 500 on a malformed TS_KEY_EXPIRY_SECONDS", async () => {
    const h = makeHarness({ env: { ...FULL_ENV, TS_KEY_EXPIRY_SECONDS: "ten-minutes" } });
    const response = await handleJoinKey(joinRequest(await mintToken(h)), h.deps);
    expect(response.status).toBe(500);
    expect(await responseJson(response)).toEqual({ error: "TS_KEY_EXPIRY_SECONDS is not a positive integer" });
    expect(h.calls).toHaveLength(0);
  });
});

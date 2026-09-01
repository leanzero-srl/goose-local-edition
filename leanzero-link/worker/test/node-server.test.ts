import type { Server } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { parseConfig } from "../src/lib/config";
import type { Deps } from "../src/lib/deps";
import { createFsKvStore } from "../src/lib/fs-kv";
import { generateOtp } from "../src/lib/otp";
import { createNodeServer } from "../src/node-server";
import { FULL_ENV, resendHappyFetch } from "./helpers";

// A real loopback http server driven by the same createNodeServer the entrypoint uses.
// The KV is a mkdtemp dir (never a real LINK_KV_DIR) and the config is built from an
// EMPTY env so every capability is false — proving the adapter reaches the handlers and
// that unconfigured capabilities degrade loudly (501), not silently.
let dir: string;
let server: Server;
let base: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "link-node-"));
  const deps: Deps = {
    kv: createFsKvStore(dir),
    fetchFn: (url, init) => fetch(url, init),
    now: () => Date.now(),
    randomOtp: generateOtp,
    log: () => {},
    config: parseConfig({}),
  };
  server = createNodeServer(deps, 0);
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const addr = server.address() as AddressInfo;
  base = `http://127.0.0.1:${addr.port}`;
});

afterEach(async () => {
  await new Promise<void>((resolve, reject) => {
    server.close((err) => (err ? reject(err) : resolve()));
  });
  await rm(dir, { recursive: true, force: true });
});

async function bodyOf(res: Response): Promise<Record<string, unknown>> {
  return (await res.json()) as Record<string, unknown>;
}

describe("node-server adapter", () => {
  it("serves GET /v1/health as 200 with ok=false and all capabilities present", async () => {
    const res = await fetch(`${base}/v1/health`);
    expect(res.status).toBe(200);
    const body = await bodyOf(res);
    expect(body.ok).toBe(false);
    expect(body.capabilities).toEqual({ mail: false, audience: false, mesh: false });
    expect(typeof body.version).toBe("string");
  });

  it("runs the handler through the adapter: POST /v1/auth/request-code with mail unconfigured → 501", async () => {
    const res = await fetch(`${base}/v1/auth/request-code`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email: "user@example.com" }),
    });
    expect(res.status).toBe(501);
    const body = await bodyOf(res);
    expect(body.error).toBe("mail not configured on this deployment");
  });

  it("returns 404 for an unknown path through the router", async () => {
    const res = await fetch(`${base}/nope`);
    expect(res.status).toBe(404);
  });

  it("returns 405 with an Allow header for a wrong method on a known route", async () => {
    const res = await fetch(`${base}/v1/health`, { method: "POST" });
    expect(res.status).toBe(405);
    expect(res.headers.get("Allow")).toContain("GET");
  });

  it("answers a CORS preflight (OPTIONS) with 204", async () => {
    const res = await fetch(`${base}/v1/auth/request-code`, { method: "OPTIONS" });
    expect(res.status).toBe(204);
  });

  it("drops a client-supplied CF-Connecting-IP before the handler: no proxy header → client_ip_unresolved, no ip bucket", async () => {
    const logs: Array<{ event: string; fields?: Record<string, unknown> }> = [];
    const mailDir = await mkdtemp(join(tmpdir(), "link-node-mail-"));
    const kv = createFsKvStore(mailDir);
    const mailDeps: Deps = {
      kv,
      fetchFn: async (url, init) => resendHappyFetch(url, init),
      now: () => Date.now(),
      randomOtp: () => "424242",
      log: (event, fields) => logs.push({ event, fields }),
      config: parseConfig(FULL_ENV),
    };
    const mailServer = createNodeServer(mailDeps, 0);
    await new Promise<void>((resolve) => mailServer.listen(0, "127.0.0.1", () => resolve()));
    try {
      const port = (mailServer.address() as AddressInfo).port;
      const res = await fetch(`http://127.0.0.1:${port}/v1/auth/request-code`, {
        method: "POST",
        headers: { "Content-Type": "application/json", "CF-Connecting-IP": "203.0.113.99" },
        body: JSON.stringify({ email: "user@example.com" }),
      });
      expect(res.status).toBe(200);
      expect(logs.some((l) => l.event === "client_ip_resolved")).toBe(false);
      expect(logs.find((l) => l.event === "client_ip_unresolved")?.fields).toEqual({
        path: "/v1/auth/request-code",
        forwardedFor: null,
      });
      expect(await kv.get("rl:ip:203.0.113.99:" + Math.floor(Date.now() / 1000 / 3600))).toBeNull();
    } finally {
      await new Promise<void>((resolve, reject) => mailServer.close((err) => (err ? reject(err) : resolve())));
      await rm(mailDir, { recursive: true, force: true });
    }
  });

  it("keeps X-Forwarded-For (the header Funnel SETS) and keys the ip bucket on it", async () => {
    const logs: Array<{ event: string; fields?: Record<string, unknown> }> = [];
    const mailDir = await mkdtemp(join(tmpdir(), "link-node-mail-"));
    const kv = createFsKvStore(mailDir);
    const mailDeps: Deps = {
      kv,
      fetchFn: async (url, init) => resendHappyFetch(url, init),
      now: () => Date.now(),
      randomOtp: () => "424242",
      log: (event, fields) => logs.push({ event, fields }),
      config: parseConfig(FULL_ENV),
    };
    const mailServer = createNodeServer(mailDeps, 0);
    await new Promise<void>((resolve) => mailServer.listen(0, "127.0.0.1", () => resolve()));
    try {
      const port = (mailServer.address() as AddressInfo).port;
      const res = await fetch(`http://127.0.0.1:${port}/v1/auth/request-code`, {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-Forwarded-For": "198.51.100.23" },
        body: JSON.stringify({ email: "user@example.com" }),
      });
      expect(res.status).toBe(200);
      expect(logs.find((l) => l.event === "client_ip_resolved")?.fields).toMatchObject({
        ip: "198.51.100.23",
        source: "x-forwarded-for",
        forwardedFor: "198.51.100.23",
      });
      expect(await kv.get("rl:ip:198.51.100.23:" + Math.floor(Date.now() / 1000 / 3600))).toBe("1");
    } finally {
      await new Promise<void>((resolve, reject) => mailServer.close((err) => (err ? reject(err) : resolve())));
      await rm(mailDir, { recursive: true, force: true });
    }
  });
});

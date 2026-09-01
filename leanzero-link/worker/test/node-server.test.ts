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
});

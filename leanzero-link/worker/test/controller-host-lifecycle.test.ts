import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { RequestListener } from "node:http";
import { build } from "esbuild";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";
import { SqliteDeviceRegistry } from "../src/controller/node-registry";
import { DurableDeviceRegistry } from "../src/controller/durable-registry";
import type { DeviceRegistryStore } from "../src/controller/registry-store";
import { ControllerService } from "../src/controller/service";
import { Registry } from "../src/controller/registry";
import { digest, signAccess, verifyAccess } from "../src/controller/crypto";
import { signJwt } from "../src/lib/jwt";
import { handleRequest } from "../src/router";
import { FULL_ENV, makeHarness } from "./helpers";
import { HeadscaleWireMock } from "./controller-mesh-provider";
import type { ControllerScope, MeshJoinCredential } from "../src/controller/types";

interface ApiResponse { status: number; headers: { get(name: string): string | null }; json(): Promise<unknown> }
type Backend = "node-sqlite" | "cloudflare-worker-durable-object";
let dir: string; let mf: Miniflare; let sqlite: SqliteDeviceRegistry; let durable: DurableDeviceRegistry;
const provider = new HeadscaleWireMock();
const loginServer = "https://mesh.example.test";
let redirectProvider = false; let redirectDestinationCalls = 0;

beforeAll(async () => {
  dir = await mkdtemp(join(tmpdir(), "link-host-lifecycle-"));
  sqlite = new SqliteDeviceRegistry(join(dir, "registry.sqlite"));
  await provider.start();
  const providerHandler = provider.server.listeners("request")[0] as RequestListener;
  provider.server.removeListener("request", providerHandler);
  provider.server.on("request", (request, response) => {
    if (request.url === "/redirect-target") {
      redirectDestinationCalls += 1; response.writeHead(200); response.end("{}"); return;
    }
    if (redirectProvider) {
      response.writeHead(307, { Location: provider.url + "/redirect-target" }); response.end(); return;
    }
    providerHandler.call(provider.server, request, response);
  });
  const bundle = await build({ entryPoints: ["src/index.ts"], bundle: true, write: false, format: "esm", platform: "browser" });
  mf = new Miniflare(convertV4MiniflareOptions({
    modules: true, script: bundle.outputFiles![0]!.text, compatibilityDate: "2026-08-01",
    bindings: { LINK_JWT_SECRET: FULL_ENV.LINK_JWT_SECRET!, HEADSCALE_API_URL: provider.url, HEADSCALE_API_KEY: "test-headscale-api", HEADSCALE_LOGIN_SERVER: loginServer },
    kvNamespaces: ["LINK_KV"], durableObjects: { LINK_DEVICE_REGISTRY: { className: "DeviceRegistryDurableObject", useSQLite: true } },
    // Only the local HTTP fixture is reachable from the worker runtime.
    // https://github.com/cloudflare/workers-sdk/tree/main/packages/miniflare#outboundservice
    outboundService: async request => {
      if (new URL(request.url).origin !== provider.url) throw new Error("Unexpected fixture provider origin");
      const response = await fetch(request.url, {
        method: request.method, redirect: "manual", headers: Object.fromEntries(request.headers),
        ...(request.method === "GET" || request.method === "HEAD" ? {} : { body: await request.text() }),
      });
      return new MiniflareResponse(await response.arrayBuffer(), { status: response.status, headers: Object.fromEntries(response.headers) });
    },
    resourcePersistencePath: join(dir, "durable"), telemetry: { enabled: false },
  }));
  durable = new DurableDeviceRegistry(await mf.getDurableObjectNamespace("LINK_DEVICE_REGISTRY") as unknown as DurableObjectNamespace);
}, 30_000);
afterAll(async () => { sqlite?.close(); await mf?.dispose(); await provider.stop(); if (dir) await rm(dir, { recursive: true, force: true }); });

async function setup(backend: Backend) {
  const store: DeviceRegistryStore = backend === "node-sqlite" ? sqlite : durable;
  const email = crypto.randomUUID() + "@example.test";
  const user = await provider.account(email);
  const h = makeHarness({ env: { ...FULL_ENV, HEADSCALE_API_URL: provider.url, HEADSCALE_API_KEY: "test-headscale-api", HEADSCALE_LOGIN_SERVER: loginServer }, fetchHandler: (url, init) => fetch(url, init) });
  h.deps.deviceRegistry = store; h.deps.now = Date.now;
  const service = new ControllerService(new Registry(store), FULL_ENV.LINK_JWT_SECRET!, Date.now);
  const iat = Math.floor(Date.now() / 1000);
  const accountToken = await signJwt(service.secret, { sub: email, iat, exp: iat + 3600, ver: 1 });
  const send = async (path: string, method: "GET" | "POST", token?: string, body?: unknown): Promise<ApiResponse> => {
    const url = "https://worker.test/v1/controller" + path;
    const headers = { ...(token === undefined ? {} : { Authorization: "Bearer " + token }), ...(body === undefined ? {} : { "Content-Type": "application/json" }) };
    const init = { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) };
    return backend === "node-sqlite" ? handleRequest(new Request(url, init), h.deps) : mf.dispatchFetch(url, init);
  };
  const get = (path: string, token?: string) => send(path, "GET", token);
  const post = (path: string, body: unknown, token: string) => send(path, "POST", token, body);
  const owner_node_id = crypto.randomUUID(); const runtime_id = crypto.randomUUID(); const store_generation = crypto.randomUUID();
  const registration = { owner_node_id, registration_id: crypto.randomUUID(), host_name: "Fixture owner", protocol_version: 1, tls_spki_sha256: "a".repeat(64), controller_port: 41227 };
  const registered = await post("/hosts/register", registration, accountToken);
  expect(registered.status).toBe(200);
  const reg = await registered.json() as { host_credential: string; host_revision: number };
  const accountKey = await digest(email);
  const state = () => service.registry.read(accountKey);
  const enrollment = crypto.randomUUID(); const challenge = await service.prepareChallenge(email, enrollment, "123456");
  await service.challengeSent(challenge.challenge_id, enrollment, true);
  const phone = await service.verify(challenge.challenge_id, enrollment, "123456", "Fixture phone", "ios");
  const enroll = async (credential = reg.host_credential, char = "a") => {
    const enrollment_id = crypto.randomUUID();
    const issued = await post("/hosts/mesh/join-key", { enrollment_id }, credential);
    expect(issued.status).toBe(200);
    const key = await issued.json() as MeshJoinCredential;
    const node = provider.join(key.auth_key, char);
    const confirm = { enrollment_id, mesh_node_key: node.nodeKey };
    const confirmed = await post("/hosts/mesh/confirm", confirm, credential);
    expect(confirmed.status).toBe(200);
    const leaseRequest = { owner_node_id, runtime_id, store_generation, catalog_revision: 100, mesh_node_key: node.nodeKey };
    const leased = await post("/hosts/lease", leaseRequest, credential);
    expect(leased.status).toBe(200);
    return { enrollment_id, key, node, confirm, leaseRequest, lease: await leased.json() as { lease_id: string } };
  };
  return { user, service, accountKey, accountToken, state, phone, get, post, registration, reg, owner_node_id, runtime_id, store_generation, enroll };
}

async function error(response: ApiResponse, status: number, code: string): Promise<void> {
  expect(response.status).toBe(status);
  expect(await response.json()).toMatchObject({ error: { code } });
  expect(response.headers.get("Cache-Control")).toBe("no-store");
}

for (const backend of ["node-sqlite", "cloudflare-worker-durable-object"] as const) {
  describe("host lifecycle over " + backend, () => {
    it("bootstraps only a registered host and exposes precisely account and owner IDs", async () => {
      const f = await setup(backend); const other = await setup(backend);
      const bootstrap = await f.get("/hosts/bootstrap", f.reg.host_credential);
      expect(bootstrap.status).toBe(200);
      expect(bootstrap.headers.get("Cache-Control")).toBe("no-store");
      expect(await bootstrap.json()).toEqual({ account_id: (await f.state())!.account_id, owner_node_id: f.owner_node_id });
      const claims = (await verifyAccess(f.service.secret, f.phone.access_token, Math.floor(Date.now() / 1000)))!;
      const wrongAudience = await signAccess(f.service.secret, { ...claims, aud: "leanzero-link/host" as typeof claims.aud });
      const parts = f.reg.host_credential.split(".");
      const alienParts = other.reg.host_credential.split(".");
      const invalidTokens = [
        f.phone.access_token, f.phone.refresh_token, f.accountToken, wrongAudience,
        `h1.${f.accountKey}.${crypto.randomUUID()}.${parts[3]}`,
        `h1.${other.accountKey}.${f.owner_node_id}.${parts[3]}`,
        `h1.${other.accountKey}.${other.owner_node_id}.${parts[3]}`,
        `h1.${f.accountKey}.${f.owner_node_id}.${alienParts[3]}`,
      ];
      const before = await f.state(); const calls = provider.calls.length;
      for (const token of invalidTokens) {
        await error(await f.get("/hosts/bootstrap", token), 401, "invalid_credential");
        await error(await f.get("/hosts/mesh/context", token), 401, "invalid_credential");
        await error(await f.post("/hosts/release", { runtime_id: f.runtime_id }, token), 401, "invalid_credential");
      }
      await error(await f.get("/hosts/bootstrap"), 401, "invalid_credential");
      expect(await f.state()).toEqual(before);
      expect(provider.calls.slice(calls)).toEqual([]);
      // The other real host is a positive control for the forged account/key combinations.
      expect(await (await other.get("/hosts/bootstrap", other.reg.host_credential)).json()).toEqual({ account_id: (await other.state())!.account_id, owner_node_id: other.owner_node_id });
    });

    it("resumes pending and issued enrollment and recovers a lost confirmation ACK without reminting", async () => {
      const f = await setup(backend); const credential = f.reg.host_credential;
      expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual({ login_server: loginServer, enrollment: null });
      const enrollment_id = crypto.randomUUID();
      let entered!: () => void; let finish!: () => void;
      const started = new Promise<void>(resolve => { entered = resolve; });
      const held = new Promise<void>(resolve => { finish = resolve; });
      provider.onMint = async () => { entered(); await held; };
      const issuing = f.post("/hosts/mesh/join-key", { enrollment_id }, credential);
      await started;
      try {
        expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual({ login_server: loginServer, enrollment: { enrollment_id, status: "pending", mesh_node_key: null } });
        await error(await f.post("/hosts/mesh/join-key", { enrollment_id }, credential), 409, "enrollment_pending");
      } finally { finish(); provider.onMint = null; }
      const issued = await issuing; expect(issued.status).toBe(200);
      const key = await issued.json() as MeshJoinCredential;
      const mintCount = provider.keys.filter(k => k.user.id === f.user.id).length;
      expect(mintCount).toBe(1);
      expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual({ login_server: loginServer, enrollment: { enrollment_id, status: "issued", mesh_node_key: null } });
      expect(await (await f.post("/hosts/mesh/join-key", { enrollment_id }, credential)).json()).toEqual(key);
      const node = provider.join(key.auth_key, "a"); const confirm = { enrollment_id, mesh_node_key: node.nodeKey };
      // The first confirmation commits, but the client discards its response.
      expect((await f.post("/hosts/mesh/confirm", confirm, credential)).status).toBe(200);
      const context = { login_server: loginServer, enrollment: { enrollment_id, status: "confirmed", mesh_node_key: node.nodeKey } };
      expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual(context);
      const confirmed = await f.post("/hosts/mesh/confirm", confirm, credential);
      expect(confirmed.status).toBe(200);
      expect(await confirmed.json()).toEqual({ owner_node_id: f.owner_node_id, mesh_node_key: node.nodeKey, host_revision: 1 });
      await error(await f.post("/hosts/mesh/join-key", { enrollment_id: crypto.randomUUID() }, credential), 409, "already_enrolled");
      expect(provider.keys.filter(k => k.user.id === f.user.id)).toHaveLength(mintCount);
      await f.service.registry.update(f.accountKey, state => {
        state!.mesh_enrollments![enrollment_id]!.expires_at = Date.now() - 1;
        return { state: state!, result: undefined };
      });
      expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual(context);
    });

    it("fails closed on provider outage and changed identity, while an attested missing node permits reconnect", async () => {
      const f = await setup(backend); const host = await f.enroll(); const credential = f.reg.host_credential;
      const before = await f.state();
      provider.unavailable = true;
      try { await error(await f.get("/hosts/mesh/context", credential), 502, "provider_failed"); }
      finally { provider.unavailable = false; }
      expect(await f.state()).toEqual(before);
      const savedKey = host.node.nodeKey; host.node.nodeKey = "nodekey:" + "c".repeat(64);
      try { await error(await f.get("/hosts/mesh/context", credential), 409, "mesh_binding_changed"); }
      finally { host.node.nodeKey = savedKey; }
      provider.paginated = true;
      try { await error(await f.get("/hosts/mesh/context", credential), 502, "provider_pagination_unsupported"); }
      finally { provider.paginated = false; }
      provider.nodes = provider.nodes.filter(node => node.id !== host.node.id);
      const missing = await f.get("/hosts/mesh/context", credential); expect(missing.status).toBe(200);
      expect(await missing.json()).toEqual({ login_server: loginServer, enrollment: null });
      await error(await f.post("/hosts/lease", host.leaseRequest, credential), 403, "host_mesh_unverified");
      const next = await f.enroll(credential, "c");
      expect(next.enrollment_id).not.toBe(host.enrollment_id);
      expect((await f.state())!.mesh_enrollments![host.enrollment_id]!.status).toBe("revoked");
      expect(await (await f.get("/hosts/mesh/context", credential)).json()).toEqual({ login_server: loginServer, enrollment: { enrollment_id: next.enrollment_id, status: "confirmed", mesh_node_key: next.node.nodeKey } });
    });

    it("rejects provider redirects without reaching the redirect destination", async () => {
      const f = await setup(backend); await f.enroll();
      const before = await f.state(); const destinationCalls = redirectDestinationCalls;
      redirectProvider = true;
      try { await error(await f.get("/hosts/mesh/context", f.reg.host_credential), 502, "provider_failed"); }
      finally { redirectProvider = false; }
      expect(redirectDestinationCalls).toBe(destinationCalls);
      expect(await f.state()).toEqual(before);
      expect((await f.get("/hosts/mesh/context", f.reg.host_credential)).status).toBe(200);
    });

    it("releases only the owning runtime, revokes its credential, and preserves rollback fences through registration", async () => {
      const f = await setup(backend); const host = await f.enroll(); const credential = f.reg.host_credential;
      await error(await f.post("/hosts/release", { runtime_id: crypto.randomUUID() }, credential), 409, "owner_identity_in_use");
      expect((await f.get("/hosts/bootstrap", credential)).status).toBe(200);
      const released = await f.post("/hosts/release", { runtime_id: f.runtime_id }, credential);
      expect(released.status).toBe(200); expect(await released.json()).toEqual({ released: true });
      expect(await (await f.post("/hosts/release", { runtime_id: f.runtime_id }, credential)).json()).toEqual({ released: true });
      for (const path of ["/hosts/bootstrap", "/hosts/mesh/context", "/hosts/peers"]) await error(await f.get(path, credential), 401, "invalid_credential");
      await error(await f.post("/hosts/lease", host.leaseRequest, credential), 401, "invalid_credential");
      const releasedState = (await f.state())!;
      expect(releasedState.hosts[f.owner_node_id]).toMatchObject({ suspended: true, lease: null, generation: f.store_generation, catalog_high_water: 100 });
      expect(releasedState.mesh_enrollments![host.enrollment_id]).toMatchObject({ status: "revoked" });
      const listed = await f.get("/hosts", f.phone.access_token); expect(listed.status).toBe(200);
      expect(await listed.json()).toMatchObject({ hosts: [{ owner_node_id: f.owner_node_id, endpoints: [], capabilities: [] }] });
      await error(await f.post("/hosts/register", { ...f.registration, registration_id: crypto.randomUUID() }, f.accountToken), 409, "registration_conflict");
      const registered = await f.post("/hosts/register", f.registration, f.accountToken); expect(registered.status).toBe(200);
      const next = await registered.json() as { host_credential: string; host_revision: number };
      expect(next.host_credential).not.toBe(credential); expect(next.host_revision).toBeGreaterThan(f.reg.host_revision);
      await error(await f.get("/hosts/bootstrap", credential), 401, "invalid_credential");
      expect(await (await f.get("/hosts/bootstrap", next.host_credential)).json()).toEqual({ account_id: releasedState.account_id, owner_node_id: f.owner_node_id });
      const reconnected = await f.enroll(next.host_credential, "c");
      await error(await f.post("/hosts/lease", { ...reconnected.leaseRequest, catalog_revision: 99 }, next.host_credential), 409, "store_rollback");
      await error(await f.post("/hosts/lease", { ...reconnected.leaseRequest, store_generation: crypto.randomUUID() }, next.host_credential), 409, "generation_changed");
      await error(await f.post("/hosts/lease", { ...reconnected.leaseRequest, runtime_id: crypto.randomUUID() }, next.host_credential), 409, "owner_identity_in_use");
    });

    it("advertises exactly the capabilities supplied by the current host lease", async () => {
      const f = await setup(backend); const host = await f.enroll();
      const advertised = async () => {
        const response = await f.get("/hosts", f.phone.access_token); expect(response.status).toBe(200);
        const body = await response.json() as { hosts: { owner_node_id: string; availability: string; capabilities: ControllerScope[] }[] };
        const owner = body.hosts.find(h => h.owner_node_id === f.owner_node_id)!;
        expect(owner.availability).toBe("online"); return owner.capabilities;
      };
      expect(await advertised()).toEqual([]);
      const capabilities: ControllerScope[] = ["catalog.read", "history.read", "resources.read"];
      expect((await f.post("/hosts/lease", { ...host.leaseRequest, capabilities }, f.reg.host_credential)).status).toBe(200);
      expect(await advertised()).toEqual(capabilities);
      for (const invalid of [["shell.execute"], ["catalog.read", "catalog.read"], "catalog.read", [null]]) {
        await error(await f.post("/hosts/lease", { ...host.leaseRequest, capabilities: invalid }, f.reg.host_credential), 400, "invalid_request");
        expect(await advertised()).toEqual(capabilities);
      }
      expect((await f.post("/hosts/lease", { ...host.leaseRequest, capabilities: ["catalog.read"] }, f.reg.host_credential)).status).toBe(200);
      expect(await advertised()).toEqual(["catalog.read"]);
      expect((await f.post("/hosts/lease", host.leaseRequest, f.reg.host_credential)).status).toBe(200);
      expect(await advertised()).toEqual([]);
      host.node.online = false;
      const offline = await f.get("/hosts", f.phone.access_token);
      expect(await offline.json()).toMatchObject({ hosts: [{ availability: "offline", endpoints: [], capabilities: [] }] });
    });
  });
}

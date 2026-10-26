import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build } from "esbuild";
import { Miniflare, convertV4MiniflareOptions } from "miniflare";
import { SqliteDeviceRegistry } from "../src/controller/node-registry";
import { DurableDeviceRegistry } from "../src/controller/durable-registry";
import type { DeviceRegistryStore } from "../src/controller/registry-store";
import { ControllerService } from "../src/controller/service";
import { Registry } from "../src/controller/registry";
import { digest } from "../src/controller/crypto";
import { reserveLegacyMint } from "../src/controller/legacy-mesh";
import { signJwt } from "../src/lib/jwt";
import { handleRequest } from "../src/router";
import { FULL_ENV, makeHarness, postJson } from "./helpers";
import { HeadscaleWireMock } from "./controller-mesh-provider";
import type { DeviceCredentials, MeshJoinCredential } from "../src/controller/types";

let dir: string; let mf: Miniflare; let sqlite: SqliteDeviceRegistry; let durable: DurableDeviceRegistry;
const provider = new HeadscaleWireMock();
beforeAll(async () => {
  dir = await mkdtemp(join(tmpdir(), "link-mesh-authority-"));
  sqlite = new SqliteDeviceRegistry(join(dir, "registry.sqlite"));
  const bundle = await build({ entryPoints: ["src/index.ts"], bundle: true, write: false, format: "esm", platform: "browser" });
  mf = new Miniflare(convertV4MiniflareOptions({ modules: true, script: bundle.outputFiles![0]!.text, compatibilityDate: "2026-08-01", bindings: { LINK_JWT_SECRET: FULL_ENV.LINK_JWT_SECRET! }, kvNamespaces: ["LINK_KV"], durableObjects: { LINK_DEVICE_REGISTRY: { className: "DeviceRegistryDurableObject", useSQLite: true } }, resourcePersistencePath: join(dir, "durable"), telemetry: { enabled: false } }));
  durable = new DurableDeviceRegistry(await mf.getDurableObjectNamespace("LINK_DEVICE_REGISTRY") as unknown as DurableObjectNamespace);
  await provider.start();
}, 30_000);
afterAll(async () => { sqlite?.close(); await mf?.dispose(); await provider.stop(); if (dir) await rm(dir, { recursive: true, force: true }); });

async function setup(store: DeviceRegistryStore) {
  const email = crypto.randomUUID() + "@example.test"; const user = await provider.account(email);
  const h = makeHarness({ env: { ...FULL_ENV, HEADSCALE_API_URL: provider.url, HEADSCALE_API_KEY: "test-headscale-api", HEADSCALE_LOGIN_SERVER: "https://mesh.example.test" }, fetchHandler: (url, init) => fetch(url, init as RequestInit) });
  h.deps.deviceRegistry = store;
  const service = new ControllerService(new Registry(store), FULL_ENV.LINK_JWT_SECRET!, h.clock.now);
  const iat = Math.floor(h.clock.now() / 1000);
  const accountToken = await signJwt(service.secret, { sub: email, iat, exp: iat + 3600, ver: 1 });
  const post = (path: string, body: unknown, token: string) => handleRequest(postJson("/v1/controller" + path, body, { Authorization: "Bearer " + token }), h.deps);
  const get = (path: string, token: string) => handleRequest(new Request("https://link.test/v1/controller" + path, { headers: { Authorization: "Bearer " + token } }), h.deps);
  const device = async (): Promise<DeviceCredentials> => {
    const enrollment = crypto.randomUUID(); const challenge = await service.prepareChallenge(email, enrollment, "123456");
    await service.challengeSent(challenge.challenge_id, enrollment, true);
    return service.verify(challenge.challenge_id, enrollment, "123456", "Phone", "ios");
  };
  const owner_node_id = crypto.randomUUID(); const registration_id = crypto.randomUUID(); const runtime_id = crypto.randomUUID(); const store_generation = crypto.randomUUID();
  const register = await post("/hosts/register", { owner_node_id, registration_id, host_name: "Workhorse", protocol_version: 1, tls_spki_sha256: "a".repeat(64), controller_port: 41227 }, accountToken);
  expect(register.status).toBe(200);
  const reg = await register.json() as { host_credential: string; host_revision: number };
  const bindHost = async () => {
    const enrollment_id = crypto.randomUUID();
    const issued = await post("/hosts/mesh/join-key", { enrollment_id }, reg.host_credential);
    expect(await issued.clone().json()).not.toHaveProperty("error");
    expect(issued.status).toBe(200);
    const key = await issued.json() as MeshJoinCredential; const node = provider.join(key.auth_key, "a");
    const confirmed = await post("/hosts/mesh/confirm", { enrollment_id, mesh_node_key: node.nodeKey }, reg.host_credential);
    expect(confirmed.status).toBe(200);
    const leaseRequest = { owner_node_id, runtime_id, store_generation, catalog_revision: 100, mesh_node_key: node.nodeKey };
    const leased = await post("/hosts/lease", leaseRequest, reg.host_credential);
    expect(leased.status).toBe(200);
    return { node, enrollment_id, key, leaseRequest, lease: await leased.json() as { lease_id: string } };
  };
  return { email, user, h, service, accountToken, post, get, device, owner_node_id, runtime_id, store_generation, reg, bindHost };
}
for (const backend of ["sqlite", "durable"] as const) {
  describe("mesh authority " + backend, () => {
    const store = () => backend === "sqlite" ? sqlite : durable;
    it("binds actual provider key/account/role, derives IPv4+IPv6 endpoints and updates IP moves", async () => {
      const f = await setup(store()); const host = await f.bindHost(); const phone = await f.device();
      const enrollment_id = crypto.randomUUID();
      const issued = await f.post("/mesh/join-key", { enrollment_id }, phone.access_token);
      expect(issued.status).toBe(200);
      const joinKey = await issued.json() as MeshJoinCredential;
      expect(Object.keys(joinKey).sort()).toEqual(["auth_key", "enrollment_id", "expiry_seconds", "login_server"]);
      expect(await (await f.post("/mesh/join-key", { enrollment_id }, phone.access_token)).json()).toEqual(joinKey);
      const node = provider.join(joinKey.auth_key, "b");
      const confirm = { enrollment_id, mesh_node_key: node.nodeKey };
      expect((await f.post("/mesh/confirm", { ...confirm, mesh_node_key: host.node.nodeKey }, phone.access_token)).status).toBe(403);
      expect((await f.post("/mesh/confirm", confirm, phone.access_token)).status).toBe(200);
      expect((await f.post("/mesh/confirm", confirm, phone.access_token)).status).toBe(200);
      const peers = await (await f.get("/hosts/peers", f.reg.host_credential)).json() as { protocol_version: number; lease_seconds: number; peers: { mesh_node_key: string; role: string }[] };
      expect(peers.protocol_version).toBe(1); expect(peers.lease_seconds).toBe(30);
      expect(peers.peers).toEqual(expect.arrayContaining([expect.objectContaining({ mesh_node_key: node.nodeKey, role: "controller" }), expect.objectContaining({ mesh_node_key: host.node.nodeKey, role: "host" })]));
      const hosts = await (await f.get("/hosts", phone.access_token)).json() as { hosts: { availability: string; endpoints: { url: string; tls_spki_sha256: string }[] }[] };
      expect(hosts.hosts[0]).toMatchObject({ availability: "online", endpoints: [{ url: "https://" + host.node.ipAddresses[0] + ":41227", tls_spki_sha256: "a".repeat(64) }, { url: "https://[fd7a:115c:a1e0::1234]:41227", tls_spki_sha256: "a".repeat(64) }] });
      host.node.ipAddresses = ["100.100.1.10"];
      const moved = await (await f.get("/hosts", phone.access_token)).json() as typeof hosts;
      expect(moved.hosts[0]!.endpoints[0]!.url).toBe("https://100.100.1.10:41227");
      host.node.nodeKey = "nodekey:" + "f".repeat(64);
      expect((await (await f.get("/hosts", phone.access_token)).json() as typeof hosts).hosts[0]).toMatchObject({ availability: "update_required", endpoints: [] });
      expect((await f.get("/hosts/peers", f.reg.host_credential)).status).toBe(403);
      const raw = (await store().read(await digest(f.email)))!.value;
      expect(raw).not.toContain(joinKey.auth_key); expect(raw).not.toContain("nodeSecret");
    });
    it("rejects forged keys, other accounts and role changes; expires admission with the lease", async () => {
      const f = await setup(store()); const host = await f.bindHost(); const phone = await f.device();
      const other = await setup(store()); const alien = await other.device();
      const enrollment_id = crypto.randomUUID();
      const issued = await f.post("/mesh/join-key", { enrollment_id }, phone.access_token);
      const key = await issued.json() as MeshJoinCredential; const n = provider.join(key.auth_key, "b");
      n.user = other.user;
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(403);
      n.user = f.user;
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, alien.access_token)).status).toBe(403);
      expect((await f.post("/hosts/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(401);
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, f.reg.host_credential)).status).toBe(401);
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: "nodekey:" + "d".repeat(64) }, phone.access_token)).status).toBe(403);
      n.expiry = new Date(f.h.clock.now() - 1).toISOString();
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(403);
      n.expiry = null; n.tags = ["tag:host"];
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(403);
      n.tags = [];
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(200);
      expect((await f.post("/hosts/lease", { ...host.leaseRequest, mesh_node_key: n.nodeKey }, f.reg.host_credential)).status).toBe(403);
      f.h.clock.advanceSeconds(61);
      expect((await f.get("/hosts/peers", f.reg.host_credential)).status).toBe(403);
    });
    it("refuses old peers, outstanding keys, incomplete pages and non-isolating policy before mint", async () => {
      const f = await setup(store()); await f.bindHost(); const phone = await f.device();
      const legacy = provider.legacy(f.user);
      const attempt = () => f.post("/mesh/join-key", { enrollment_id: crypto.randomUUID() }, phone.access_token);
      expect((await attempt()).status).toBe(409);
      provider.nodes = provider.nodes.filter(n => n !== legacy);
      const untracked = await fetch(provider.url + "/api/v1/preauthkey", { method: "POST", headers: { Authorization: "Bearer test-headscale-api", "Content-Type": "application/json" }, body: JSON.stringify({ user: f.user.id, reusable: false, ephemeral: true, expiration: new Date(f.h.clock.now() + 600_000).toISOString() }) });
      expect(untracked.status).toBe(200);
      expect((await attempt()).status).toBe(409);
      provider.keys.at(-1)!.expiration = new Date(f.h.clock.now() - 1).toISOString();
      provider.paginated = true;
      expect((await attempt()).status).toBe(502);
      provider.paginated = false; provider.badPolicy = true;
      expect((await attempt()).status).toBe(503);
      provider.badPolicy = false;
      const foreign = await provider.account(crypto.randomUUID() + "@example.test");
      provider.legacy(foreign); // A different account does not block ours.
      const response = await attempt(); expect(response.status).toBe(200);
      const legacyJoin = await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: "Bearer " + f.accountToken }), f.h.deps);
      expect(legacyJoin.status).toBe(409);
      expect((await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: "Bearer " + phone.access_token }), f.h.deps)).status).toBe(401);
    });
    it("serializes duplicate enrollment and fences in-flight legacy joins", async () => {
      const f = await setup(store()); await f.bindHost(); const phone = await f.device();
      const release = await reserveLegacyMint(f.h.deps, f.email); expect(release).not.toBeNull();
      const enrollment_id = crypto.randomUUID();
      expect((await f.post("/mesh/join-key", { enrollment_id }, phone.access_token)).status).toBe(409);
      await release!();
      expect((await f.post("/mesh/join-key", { enrollment_id }, phone.access_token)).status).toBe(409);
      f.h.clock.advanceSeconds(601);
      // Renew the host's still-current generation after the legacy credential window.
      const state = (await f.service.registry.read(await digest(f.email)))!;
      const h = state.hosts[f.owner_node_id]!;
      await f.post("/hosts/lease", { owner_node_id: f.owner_node_id, runtime_id: f.runtime_id, store_generation: f.store_generation, catalog_revision: 100, mesh_node_key: h.lease!.mesh_node_key }, f.reg.host_credential);
      const responses = await Promise.all(Array.from({ length: 6 }, () => f.post("/mesh/join-key", { enrollment_id }, phone.access_token)));
      expect(responses.every(r => r.status === 200 || r.status === 409)).toBe(true);
      const ok = responses.find(r => r.status === 200)!; expect(ok).toBeDefined();
      const key = await ok.json() as MeshJoinCredential;
      expect(provider.keys.filter(k => k.key === key.auth_key)).toHaveLength(1);
      expect(await (await f.post("/mesh/join-key", { enrollment_id }, phone.access_token)).json()).toEqual(key);
    });
    it("revokes and deletes only the selected controller, retries cleanup, and blocks refresh after revoke", async () => {
      const f = await setup(store()); const host = await f.bindHost(); const first = await f.device(); const second = await f.device();
      const enroll = async (device: DeviceCredentials, char: string) => {
        const enrollment_id = crypto.randomUUID();
        const response = await f.post("/mesh/join-key", { enrollment_id }, device.access_token); expect(response.status).toBe(200);
        const key = await response.json() as MeshJoinCredential; const n = provider.join(key.auth_key, char);
        expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, device.access_token)).status).toBe(200); return n;
      };
      const a = await enroll(first, "b"); const b = await enroll(second, "c");
      provider.unavailable = true;
      const revoked = await f.post("/devices/" + first.device.device_id + "/revoke", { request_id: crypto.randomUUID(), expected_revision: 1 }, second.access_token);
      expect(revoked.status).toBe(200); expect(await revoked.json()).toMatchObject({ mesh_cleanup: "pending" });
      provider.unavailable = false;
      const peers = await (await f.get("/hosts/peers", f.reg.host_credential)).json() as { peers: { mesh_node_key: string }[] };
      expect(peers.peers.map(p => p.mesh_node_key)).toEqual(expect.arrayContaining([host.node.nodeKey, b.nodeKey]));
      expect(peers.peers.map(p => p.mesh_node_key)).not.toContain(a.nodeKey);
      const cleanup = await f.post("/devices/" + first.device.device_id + "/mesh/cleanup", {}, second.access_token);
      expect(await cleanup.json()).toMatchObject({ mesh_cleanup: "complete" });
      expect(provider.nodes.some(n => n.id === a.id)).toBe(false); expect(provider.nodes.some(n => n.id === b.id)).toBe(true);
      expect((await f.post("/auth/refresh", { device_id: first.device.device_id, refresh_token: first.refresh_token, request_id: crypto.randomUUID() }, first.access_token)).status).toBe(401);
      expect((await f.get("/devices", second.access_token)).status).toBe(200);
    });
    it("fences revoked-during-mint credentials and cleans unconfirmed provider nodes", async () => {
      const f = await setup(store()); await f.bindHost(); const first = await f.device(); const actor = await f.device();
      let entered!: () => void; let finish!: () => void;
      const started = new Promise<void>(resolve => { entered = resolve; }); const blocked = new Promise<void>(resolve => { finish = resolve; });
      provider.onMint = async () => { entered(); await blocked; };
      const pending = f.post("/mesh/join-key", { enrollment_id: crypto.randomUUID() }, first.access_token);
      await started;
      const k = provider.keys.at(-1)!; const n = provider.join(k.key, "b");
      expect((await f.post("/devices/" + first.device.device_id + "/revoke", { request_id: crypto.randomUUID(), expected_revision: 1 }, actor.access_token)).status).toBe(200);
      finish(); provider.onMint = null;
      expect((await pending).status).toBe(401);
      expect(provider.nodes.some(x => x.id === n.id)).toBe(false);
      expect(Date.parse(k.expiration)).toBeLessThan(f.h.clock.now());
      // A normal token rotation invalidates an in-flight access proof without
      // revoking the device; the newly minted enrollment key still needs cleanup.
      let rotatedEntered!: () => void; let rotatedFinish!: () => void;
      const rotatedStarted = new Promise<void>(resolve => { rotatedEntered = resolve; });
      const rotatedBlocked = new Promise<void>(resolve => { rotatedFinish = resolve; });
      provider.onMint = async () => { rotatedEntered(); await rotatedBlocked; };
      const oldAccessMint = f.post("/mesh/join-key", { enrollment_id: crypto.randomUUID() }, actor.access_token);
      await rotatedStarted;
      const rotationKey = provider.keys.at(-1)!;
      const rotation = await f.post("/auth/refresh", { device_id: actor.device.device_id, refresh_token: actor.refresh_token, request_id: crypto.randomUUID() }, actor.access_token);
      expect(rotation.status).toBe(200);
      const renewed = await rotation.json() as DeviceCredentials;
      rotatedFinish(); provider.onMint = null;
      expect((await oldAccessMint).status).toBe(401);
      expect(Date.parse(rotationKey.expiration)).toBeLessThan(f.h.clock.now());
      expect((await f.get("/devices", renewed.access_token)).status).toBe(200);
    });
    it("requires authenticated refresh replay for cleanup and never rearms completed cleanup", async () => {
      const f = await setup(store()); await f.bindHost(); const phone = await f.device(); const actor = await f.device();
      const enrollment_id = crypto.randomUUID();
      const issued = await f.post("/mesh/join-key", { enrollment_id }, phone.access_token);
      expect(issued.status).toBe(200);
      const key = await issued.json() as MeshJoinCredential; const node = provider.join(key.auth_key, "b");
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: node.nodeKey }, phone.access_token)).status).toBe(200);
      const accountKey = await digest(f.email);
      const fake = `r1.${accountKey}.${phone.device.device_id}.${"A".repeat(43)}`;
      const refresh = (body: unknown) => handleRequest(postJson("/v1/controller/auth/refresh", body), f.h.deps);
      const request = { device_id: phone.device.device_id, refresh_token: phone.refresh_token, request_id: crypto.randomUUID() };
      const invalid = [
        { ...request, refresh_token: fake },
        { ...request, refresh_token: fake.slice(0, -10) },
        { ...request, refresh_token: `r1.${accountKey}.${crypto.randomUUID()}.${"A".repeat(43)}` },
        { ...request, device_id: actor.device.device_id },
        { ...request, device_id: "not-a-uuid", refresh_token: fake },
        { ...request, request_id: "not-a-uuid" },
      ];
      const assertNoCleanup = async () => {
        const before = await store().read(accountKey); const calls = provider.calls.length;
        for (const body of invalid) expect((await refresh(body)).status).toBeGreaterThanOrEqual(400);
        expect(provider.calls.slice(calls)).toEqual([]);
        expect(await store().read(accountKey)).toEqual(before);
      };
      await assertNoCleanup();
      const rotated = await refresh(request); expect(rotated.status).toBe(200);
      const credentials = await rotated.json() as DeviceCredentials;
      const calls = provider.calls.length;
      expect(await (await refresh(request)).json()).toEqual(credentials); // Lost ACK.
      expect(provider.calls.slice(calls)).toEqual([]);
      const replay = await refresh({ ...request, request_id: crypto.randomUUID() });
      expect(replay.status).toBe(401); expect(await replay.json()).toMatchObject({ error: { code: "refresh_replayed" } });
      expect(provider.nodes.some(n => n.id === node.id)).toBe(false);
      expect(Date.parse(provider.keys.find(k => k.key === key.auth_key)!.expiration)).toBeLessThan(f.h.clock.now());
      expect((await f.service.registry.read(accountKey))!.mesh_enrollments![enrollment_id]!.cleanup_pending).toBe(false);
      await assertNoCleanup(); // Known revoked IDs still convey no authority.
      const completedCalls = provider.calls.length;
      for (let i = 0; i < 3; i++) {
        expect((await refresh(request)).status).toBe(401);
        const retry = await f.post("/devices/" + phone.device.device_id + "/mesh/cleanup", {}, actor.access_token);
        expect(retry.status).toBe(200); expect(await retry.json()).toMatchObject({ mesh_cleanup: "complete" });
      }
      expect(provider.calls.slice(completedCalls)).toEqual([]);
    });
    it("caps peer and host authorization at provider expiry and rejects unsupported providers", async () => {
      const f = await setup(store()); const host = await f.bindHost(); const phone = await f.device();
      host.node.expiry = new Date(f.h.clock.now() + 5_000).toISOString();
      const peers = await (await f.get("/hosts/peers", f.reg.host_credential)).json() as { lease_seconds: number };
      expect(peers.lease_seconds).toBe(5);
      const auth = await (await f.post("/introspect", { token: phone.access_token, owner_node_id: f.owner_node_id, runtime_id: f.runtime_id, host_lease_id: host.lease.lease_id, required_scope: "history.read" }, f.reg.host_credential)).json() as { lease_seconds: number };
      expect(auth.lease_seconds).toBe(5);
      f.h.clock.advanceSeconds(6);
      expect((await f.get("/hosts/peers", f.reg.host_credential)).status).toBe(403);
      const reEnrollment = crypto.randomUUID();
      const reconnect = await f.post("/hosts/mesh/join-key", { enrollment_id: reEnrollment }, f.reg.host_credential);
      expect(reconnect.status).toBe(200);
      const nextKey = await reconnect.json() as MeshJoinCredential;
      const nextNode = provider.join(nextKey.auth_key, "c");
      expect((await f.post("/hosts/mesh/confirm", { enrollment_id: reEnrollment, mesh_node_key: nextNode.nodeKey }, f.reg.host_credential)).status).toBe(200);
      f.h.deps.config.meshProvider = "tailscale";
      const unsupported = await f.post("/mesh/join-key", { enrollment_id: crypto.randomUUID() }, phone.access_token);
      expect(unsupported.status).toBe(501);
      expect(await unsupported.json()).toMatchObject({ error: { code: "provider_binding_unsupported" } });
      f.h.deps.config.meshProvider = "none";
      expect((await f.post("/mesh/join-key", { enrollment_id: crypto.randomUUID() }, phone.access_token)).status).toBe(501);
    });
    it("refuses expired enrollment and invalid endpoint ports without trusting supplied IPs", async () => {
      const f = await setup(store()); await f.bindHost(); const phone = await f.device(); const enrollment_id = crypto.randomUUID();
      expect((await f.post("/mesh/join-key", { enrollment_id, role: "host" }, phone.access_token)).status).toBe(400);
      const issued = await f.post("/mesh/join-key", { enrollment_id, mesh_ip: "8.8.8.8" }, phone.access_token);
      expect(issued.status).toBe(200);
      const key = await issued.json() as MeshJoinCredential; const n = provider.join(key.auth_key, "b");
      f.h.clock.advanceSeconds(601);
      expect((await f.post("/mesh/confirm", { enrollment_id, mesh_node_key: n.nodeKey }, phone.access_token)).status).toBe(409);
      for (const controller_port of [0, -1, 65536, 1.5, "443"]) {
        expect((await f.post("/hosts/register", { owner_node_id: crypto.randomUUID(), registration_id: crypto.randomUUID(), host_name: "Bad port", protocol_version: 1, tls_spki_sha256: "a".repeat(64), controller_port }, f.accountToken)).status).toBe(400);
      }
    });
    it("requires explicit two-credential generation recovery and rejects stale high-water", async () => {
      const f = await setup(store()); const host = await f.bindHost(); const phone = await f.device();
      const req = { token: phone.access_token, owner_node_id: f.owner_node_id, request_id: crypto.randomUUID(), expected_generation: f.store_generation, expected_host_revision: 1, expected_catalog_revision: 100, new_generation: crypto.randomUUID() };
      expect((await f.post("/hosts/recover-generation", { ...req, expected_catalog_revision: 99 }, f.reg.host_credential)).status).toBe(409);
      expect((await f.post("/hosts/recover-generation", req, phone.access_token)).status).toBe(401);
      const recovered = await f.post("/hosts/recover-generation", req, f.reg.host_credential);
      expect(recovered.status).toBe(200); const receipt = await recovered.json();
      expect(await (await f.post("/hosts/recover-generation", req, f.reg.host_credential)).json()).toEqual(receipt);
      expect((await f.post("/hosts/lease", host.leaseRequest, f.reg.host_credential)).status).toBe(409);
      expect((await f.post("/hosts/lease", { ...host.leaseRequest, store_generation: req.new_generation, catalog_revision: 0 }, f.reg.host_credential)).status).toBe(200);
      expect((await f.post("/introspect", { token: phone.access_token, owner_node_id: f.owner_node_id, runtime_id: f.runtime_id, host_lease_id: host.lease.lease_id, required_scope: "history.read" }, f.reg.host_credential)).status).toBe(403);
    });
  });
}

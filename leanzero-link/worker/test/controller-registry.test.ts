import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build } from "esbuild";
import { Miniflare, convertV4MiniflareOptions } from "miniflare";
import { SqliteDeviceRegistry } from "../src/controller/node-registry";
import { DurableDeviceRegistry } from "../src/controller/durable-registry";
import type { DeviceRegistryStore } from "../src/controller/registry-store";
import { ControllerService } from "../src/controller/service";
import { Registry } from "../src/controller/registry";
import { HostService } from "../src/controller/hosts";
import { digest, signAccess, verifyAccess } from "../src/controller/crypto";
import { signJwt } from "../src/lib/jwt";
import { handleRequest } from "../src/router";
import { FULL_ENV, makeHarness, postJson, resendHappyFetch, type TestHarness } from "./helpers";
import type { DeviceCredentials } from "../src/controller/types";

let mf: Miniflare;
let durable: DurableDeviceRegistry;
let node: SqliteDeviceRegistry;
let dir: string;
let runtimeOptions: ConstructorParameters<typeof Miniflare>[0];

beforeAll(async () => {
  dir = await mkdtemp(join(tmpdir(), "link-controller-registry-"));
  node = new SqliteDeviceRegistry(join(dir, "registry.sqlite"));
  const bundle = await build({ entryPoints: ["src/index.ts"], bundle: true, write: false, format: "esm", platform: "browser" });
  runtimeOptions = convertV4MiniflareOptions({ modules: true, script: bundle.outputFiles![0]!.text, compatibilityDate: "2026-08-01", bindings: { LINK_JWT_SECRET: FULL_ENV.LINK_JWT_SECRET! }, kvNamespaces: ["LINK_KV"], durableObjects: { LINK_DEVICE_REGISTRY: { className: "DeviceRegistryDurableObject", useSQLite: true } }, resourcePersistencePath: join(dir, "durable"), telemetry: { enabled: false } });
  mf = new Miniflare(runtimeOptions);
  durable = new DurableDeviceRegistry(await mf.getDurableObjectNamespace("LINK_DEVICE_REGISTRY") as unknown as DurableObjectNamespace);
}, 30_000);
afterAll(async () => { node?.close(); await mf?.dispose(); if (dir) await rm(dir, { recursive: true, force: true }); });

function harness(store: DeviceRegistryStore): TestHarness {
  const h = makeHarness({ fetchHandler: resendHappyFetch });
  h.deps.deviceRegistry = store;
  h.deps.randomOtp = () => "123456";
  return h;
}
const email = (): string => `${crypto.randomUUID()}@example.test`;
async function post(h: TestHarness, path: string, body: unknown, token?: string): Promise<Response> {
  return handleRequest(postJson(`/v1/controller${path}`, body, token === undefined ? {} : { Authorization: `Bearer ${token}` }), h.deps);
}
async function enroll(h: TestHarness, account = email()): Promise<DeviceCredentials> {
  const enrollment_id = crypto.randomUUID();
  const req = await post(h, "/auth/request-code", { email: account, enrollment_id });
  expect(req.status).toBe(200);
  const challenge = await req.json() as { challenge_id: string };
  const answer = await post(h, "/auth/verify", { ...challenge, enrollment_id, code: "123456", device_name: "Phone", platform: "ios" });
  expect(answer.status).toBe(200);
  return await answer.json() as DeviceCredentials;
}

for (const backend of ["node-sqlite", "cloudflare-durable-object"] as const) {
  describe(backend, () => {
    const store = (): DeviceRegistryStore => backend === "node-sqlite" ? node : durable;
    async function recoveryFixture() {
      const h = harness(store()); const account = email();
      const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      const iat = Math.floor(h.clock.now() / 1000);
      const claims = { sub: account, iat, exp: iat + 3600, ver: 1 };
      const token = await signJwt(service.secret, claims);
      const reg = await new HostService(service).register(token, null, { owner_node_id: crypto.randomUUID(), registration_id: crypto.randomUUID(), host_name: "Saved login", protocol_version: 1, tls_spki_sha256: "a".repeat(64), controller_port: 1305 });
      const key = await digest(account); const secretKey = `nodesecret:${account}`;
      const recover = (accountToken = token, host = reg.host_credential, body = {}) => handleRequest(postJson("/v1/controller/hosts/recover-node-secret", body, { Authorization: `Bearer ${accountToken}`, "X-Link-Host-Credential": host }), h.deps);
      return { h, account, service, claims, token, reg, key, secretKey, recover };
    }
    it("recovers only the existing account secret with both credentials and no provider calls", async () => {
      const f = await recoveryFixture(); const secret = "a".repeat(64);
      await f.h.kv.put(f.secretKey, secret);
      const before = await f.service.registry.read(f.key);
      const recovered = await f.recover(); expect(recovered.status).toBe(200);
      expect(recovered.headers.get("Cache-Control")).toBe("no-store");
      expect(await recovered.json()).toEqual({ account_id: before!.account_id, owner_node_id: f.reg.host_credential.split(".")[2], email: f.account, node_secret: secret });
      expect(await f.service.registry.read(f.key)).toEqual(before);
      expect(await f.h.kv.get(f.secretKey)).toBe(secret);
      expect((await f.recover()).status).toBe(200);
      expect(f.h.calls).toEqual([]); expect(f.h.logs).toEqual([]);
    });
    it("rejects expired, revoked, controller, other-account, forged and suspended recovery authority", async () => {
      const f = await recoveryFixture(); const other = await recoveryFixture();
      await f.h.kv.put(f.secretKey, "a".repeat(64));
      const phone = await enroll(f.h, f.account);
      const claims = (await verifyAccess(f.service.secret, phone.access_token, Math.floor(f.h.clock.now() / 1000)))!;
      const desktop = await signAccess(f.service.secret, { ...claims, jti: crypto.randomUUID() });
      const expired = await signJwt(f.service.secret, { ...f.claims, exp: f.claims.iat });
      const forged = f.reg.host_credential.slice(0, -43) + "x".repeat(43);
      for (const [token, host, code] of [
        [expired, f.reg.host_credential, "invalid_credential"],
        [phone.access_token, f.reg.host_credential, "invalid_credential"],
        [desktop, f.reg.host_credential, "invalid_credential"],
        [other.token, f.reg.host_credential, "wrong_account"],
        [f.token, other.reg.host_credential, "wrong_account"],
        [f.token, forged, "invalid_credential"],
        [f.token, phone.access_token, "invalid_credential"],
      ]) {
        const res = await f.recover(token!, host!); expect(res.status).toBe(code === "wrong_account" ? 403 : 401);
        expect(await res.json()).toMatchObject({ error: { code } });
      }
      expect((await f.recover(f.token, f.reg.host_credential, { email: other.account })).status).toBe(400);
      await f.service.registry.update(f.key, state => { state!.legacy_revoked_before = f.claims.iat; return { state: state!, result: undefined }; });
      expect(await (await f.recover()).json()).toMatchObject({ error: { code: "account_credential_revoked" } });
      await f.service.registry.update(f.key, state => { delete state!.legacy_revoked_before; state!.hosts[f.reg.host_credential.split(".")[2]!]!.suspended = true; return { state: state!, result: undefined }; });
      expect(await (await f.recover()).json()).toMatchObject({ error: { code: "invalid_credential" } });
      expect(await f.h.kv.get(f.secretKey)).toBe("a".repeat(64));
      expect(f.h.calls.every(c => c.url.endsWith("/emails"))).toBe(true);
    });
    it("never creates or rotates absent/corrupt secrets and rechecks a racing revocation", async () => {
      const f = await recoveryFixture();
      for (const raw of [null, "corrupt", "A".repeat(64)]) {
        if (raw === null) await f.h.kv.delete(f.secretKey); else await f.h.kv.put(f.secretKey, raw);
        const response = await f.recover(); expect(response.status).toBe(409);
        expect(await response.json()).toMatchObject({ error: { code: raw === null ? "node_secret_missing" : "node_secret_corrupt" } });
        expect(await f.h.kv.get(f.secretKey)).toBe(raw);
      }
      await f.h.kv.put(f.secretKey, "a".repeat(64));
      const get = f.h.kv.get.bind(f.h.kv); let raced = false;
      f.h.kv.get = async key => {
        const value = await get(key);
        if (key === f.secretKey && !raced) {
          raced = true;
          await f.service.registry.update(f.key, state => { state!.legacy_revoked_before = f.claims.iat; return { state: state!, result: undefined }; });
        }
        return value;
      };
      const response = await f.recover(); expect(response.status).toBe(401);
      expect(await response.json()).toMatchObject({ error: { code: "account_credential_revoked" } });
      expect(raced).toBe(true); expect(f.h.calls).toEqual([]); expect(f.h.logs).toEqual([]);
    });
    it("refuses expiry while waiting for the existing secret", async () => {
      const f = await recoveryFixture(); await f.h.kv.put(f.secretKey, "a".repeat(64));
      const get = f.h.kv.get.bind(f.h.kv);
      f.h.kv.get = async key => { const value = await get(key); f.h.clock.advanceSeconds(3601); return value; };
      const response = await f.recover(); expect(response.status).toBe(401);
      expect(await response.json()).toMatchObject({ error: { code: "invalid_credential" } });
      expect(await get(f.secretKey)).toBe("a".repeat(64)); expect(f.h.calls).toEqual([]);
    });
    it("refuses expiry while the registry CAS commits", async () => {
      const f = await recoveryFixture(); await f.h.kv.put(f.secretKey, "a".repeat(64));
      const original = f.h.deps.deviceRegistry!;
      f.h.deps.deviceRegistry = {
        read: key => original.read(key),
        compareAndSwap: async (key, revision, value) => {
          const committed = await original.compareAndSwap(key, revision, value);
          f.h.clock.advanceSeconds(3601); return committed;
        },
      };
      const response = await f.recover(); expect(response.status).toBe(401);
      expect(await response.json()).toMatchObject({ error: { code: "invalid_credential" } });
    });
    it("serializes concurrent CAS writes on the real adapter", async () => {
      const key = await digest(crypto.randomUUID());
      await Promise.all(Array.from({ length: 30 }, async () => {
        for (;;) {
          const old = await store().read(key);
          const n = old === null ? 0 : Number(old.value);
          if (await store().compareAndSwap(key, old?.revision ?? null, String(n + 1))) return;
        }
      }));
      expect(await store().read(key)).toEqual({ revision: 30, value: "30" });
      expect(await store().compareAndSwap(key, 1, "lost update")).toBe(false);
    }, 30_000);
    it("consumes one OTP once under simultaneous verification and recovers the same enrollment", async () => {
      const h = harness(store()); const account = email(); const enrollment_id = crypto.randomUUID();
      const response = await post(h, "/auth/request-code", { email: account, enrollment_id });
      const challenge = await response.json();
      const body = { ...challenge as object, enrollment_id, code: "123456", device_name: "Phone", platform: "ios" };
      const answers = await Promise.all(Array.from({ length: 12 }, () => post(h, "/auth/verify", body)));
      expect(answers.every(r => r.status === 200)).toBe(true);
      const values = await Promise.all(answers.map(r => r.json() as Promise<DeviceCredentials>));
      expect(new Set(values.map(v => v.device.device_id)).size).toBe(1);
      expect(new Set(values.map(v => v.refresh_token)).size).toBe(1);
      const raw = (await store().read(await digest(account)))!.value;
      expect(Object.keys(JSON.parse(raw).devices)).toHaveLength(1);
      expect(JSON.parse(raw).rate_windows.verify.count).toBe(12);
      expect(raw).not.toContain(values[0]!.refresh_token);
      expect(raw).not.toContain(values[0]!.access_token);
      expect(JSON.stringify(values)).not.toMatch(/nodeSecret|node_secret/);
      expect(h.calls.filter(c => c.url.endsWith("/emails"))).toHaveLength(1);
    });
    it("bounds consumed-OTP receipt recovery using the strong registry even without KV", async () => {
      const h = harness(store()); const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      const enrollment = crypto.randomUUID();
      const challenge = await service.prepareChallenge(email(), enrollment, "123456");
      await service.challengeSent(challenge.challenge_id, enrollment, true);
      const values = await Promise.allSettled(Array.from({ length: 24 }, () => service.verify(challenge.challenge_id, enrollment, "123456", "Phone", "ios")));
      expect(values.filter(v => v.status === "fulfilled")).toHaveLength(20);
      for (const v of values) if (v.status === "rejected") expect(v.reason).toMatchObject({ code: "rate_limited" });
    }, 30_000);
    it("persists spent wrong OTP attempts and rejects the correct code after exhaustion", async () => {
      const h = harness(store()); const enrollment_id = crypto.randomUUID();
      const response = await post(h, "/auth/request-code", { email: email(), enrollment_id });
      const challenge = await response.json();
      const base = { ...challenge as object, enrollment_id, device_name: "Phone", platform: "ios" };
      const guesses = await Promise.all(Array.from({ length: 8 }, () => post(h, "/auth/verify", { ...base, code: "000000" })));
      expect(guesses.every(r => r.status === 401)).toBe(true);
      expect((await post(h, "/auth/verify", { ...base, code: "123456" })).status).toBe(401);
    });
    it("device token never authenticates the legacy join endpoint", async () => {
      const h = harness(store()); const d = await enroll(h);
      const response = await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: `Bearer ${d.access_token}` }), h.deps);
      expect(response.status).toBe(401);
      expect(h.calls.every(c => !c.url.includes("tailscale"))).toBe(true);
      expect(d.device.role).toBe("controller");
    });
    it("rejects wrong issuer, audience, expiration, signature and legacy credentials", async () => {
      const h = harness(store()); const d = await enroll(h);
      const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      const claims = (await verifyAccess(service.secret, d.access_token, Math.floor(h.clock.now() / 1000)))!;
      for (const patch of [{ aud: "legacy-node" }, { iss: "another-authority" }, { ver: 1 }, { exp: claims.iat - 1 }, { iat: claims.iat + 1 }]) {
        const token = await signAccess(service.secret, { ...claims, ...patch } as typeof claims);
        await expect(service.authenticate(token)).rejects.toMatchObject({ code: "invalid_credential" });
      }
      const badSignature = await signAccess("another-signing-secret-with-enough-entropy", claims);
      await expect(service.authenticate(badSignature)).rejects.toMatchObject({ code: "invalid_credential" });
      const legacy = await signJwt(service.secret, { sub: email(), iat: claims.iat, exp: claims.exp, ver: 1 });
      await expect(service.authenticate(legacy)).rejects.toMatchObject({ code: "invalid_credential" });
      expect((await service.authenticate(d.access_token)).device.record.device_id).toBe(d.device.device_id);
    });
    it("rotates refresh exactly once, retries the lost response, and revokes a replayed family", async () => {
      const h = harness(store()); const d = await enroll(h); const request_id = crypto.randomUUID();
      const body = { device_id: d.device.device_id, refresh_token: d.refresh_token, request_id };
      const results = await Promise.all(Array.from({ length: 8 }, () => post(h, "/auth/refresh", body)));
      expect(results.every(r => r.status === 200)).toBe(true);
      const rotated = await Promise.all(results.map(r => r.json() as Promise<DeviceCredentials>));
      expect(new Set(rotated.map(v => v.refresh_token)).size).toBe(1);
      expect(rotated[0]!.device.revision).toBe(2);
      const replay = await post(h, "/auth/refresh", { ...body, request_id: crypto.randomUUID() });
      expect(replay.status).toBe(401);
      const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      await expect(service.authenticate(rotated[0]!.access_token)).rejects.toMatchObject({ code: "invalid_credential" });
    });
    it("binds introspection to account, owner, runtime, current generation and TLS identity", async () => {
      const h = harness(store()); const account = email(); const d = await enroll(h, account);
      const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      const hosts = new HostService(service);
      const iat = Math.floor(h.clock.now() / 1000);
      const accountToken = await signJwt(service.secret, { sub: account, iat, exp: iat + 3600, ver: 1 });
      const owner_node_id = crypto.randomUUID(); const runtime_id = crypto.randomUUID();
      const reg = await hosts.register(accountToken, null, { owner_node_id, registration_id: crypto.randomUUID(), host_name: "Owner", protocol_version: 1, tls_spki_sha256: "a".repeat(64) });
      const original = (await service.registry.read(await digest(account)))!.hosts[owner_node_id]!;
      const retry = await hosts.register(accountToken, null, { owner_node_id, registration_id: original.registration_id, host_name: "Owner", protocol_version: 1, tls_spki_sha256: "a".repeat(64) });
      expect(retry).toEqual(reg);
      const lease = await hosts.lease(reg.host_credential, { owner_node_id, runtime_id, store_generation: crypto.randomUUID(), catalog_revision: 100, mesh_node_key: "nodekey:test" });
      await expect(hosts.register(accountToken, null, { owner_node_id, registration_id: original.registration_id, host_name: "Owner", protocol_version: 1, tls_spki_sha256: "a".repeat(64) })).rejects.toMatchObject({ code: "owner_already_registered" });
      const allowed = await hosts.introspect(reg.host_credential, d.access_token, owner_node_id, "history.read", runtime_id, lease.lease_id);
      expect(allowed).toMatchObject({ active: true, device_id: d.device.device_id, owner_node_id, lease_seconds: 30 });
      const alien = await enroll(h);
      expect(await hosts.introspect(reg.host_credential, alien.access_token, owner_node_id, "history.read", runtime_id, lease.lease_id)).toEqual({ active: false });
      await expect(hosts.introspect(reg.host_credential, d.access_token, crypto.randomUUID(), "history.read", runtime_id, lease.lease_id)).rejects.toMatchObject({ code: "wrong_owner" });
      await expect(hosts.introspect(reg.host_credential, d.access_token, owner_node_id, "history.read", crypto.randomUUID(), lease.lease_id)).rejects.toMatchObject({ code: "host_lease_conflict" });
      expect(await hosts.introspect(reg.host_credential, d.access_token, owner_node_id, "shell.execute", runtime_id, lease.lease_id)).toEqual({ active: false });
      await expect(hosts.register(d.access_token, null, { owner_node_id: crypto.randomUUID(), registration_id: crypto.randomUUID(), host_name: "Forged host", protocol_version: 1, tls_spki_sha256: null })).rejects.toMatchObject({ code: "invalid_credential" });
      const current = (await service.registry.read(await digest(account)))!.hosts[owner_node_id]!;
      await expect(hosts.lease(reg.host_credential, { owner_node_id, runtime_id: crypto.randomUUID(), store_generation: current.generation!, catalog_revision: 100, mesh_node_key: "nodekey:test" })).rejects.toMatchObject({ code: "owner_identity_in_use" });
      await expect(hosts.lease(reg.host_credential, { owner_node_id, runtime_id, store_generation: current.generation!, catalog_revision: 99, mesh_node_key: "nodekey:test" })).rejects.toMatchObject({ code: "store_rollback" });
      await expect(hosts.lease(reg.host_credential, { owner_node_id, runtime_id, store_generation: crypto.randomUUID(), catalog_revision: 100, mesh_node_key: "nodekey:test" })).rejects.toMatchObject({ code: "generation_changed" });
      h.clock.advanceSeconds(61);
      await expect(hosts.introspect(reg.host_credential, d.access_token, owner_node_id, "history.read", runtime_id, lease.lease_id)).rejects.toMatchObject({ code: "host_lease_expired" });
    });
    it("revokes one controller without revoking its sibling and checks stale revisions", async () => {
      const h = harness(store()); const account = email();
      const first = await enroll(h, account); const second = await enroll(h, account);
      const req = { request_id: crypto.randomUUID(), expected_revision: first.device.revision };
      const response = await post(h, `/devices/${first.device.device_id}/revoke`, req, second.access_token);
      expect(response.status).toBe(200);
      const retried = await post(h, `/devices/${first.device.device_id}/revoke`, req, second.access_token);
      expect(await retried.json()).toEqual(await response.json());
      const service = new ControllerService(new Registry(store()), h.deps.config.jwtSecret!, h.clock.now);
      await expect(service.authenticate(first.access_token)).rejects.toMatchObject({ code: "invalid_credential" });
      expect((await service.authenticate(second.access_token)).device.record.device_id).toBe(second.device.device_id);
      expect((await post(h, `/devices/${first.device.device_id}/revoke`, { ...req, request_id: crypto.randomUUID() }, second.access_token)).status).toBe(409);
    });
  });
}

it("Node registry reopens persisted data with cross-connection CAS and private files", async () => {
  const path = join(dir, "reopen.sqlite"); const a = new SqliteDeviceRegistry(path); const b = new SqliteDeviceRegistry(path);
  const key = await digest("same-account");
  expect(await a.compareAndSwap(key, null, "first")).toBe(true);
  expect(await b.compareAndSwap(key, null, "overwrite")).toBe(false);
  a.close(); b.close(); const reopened = new SqliteDeviceRegistry(path);
  expect(await reopened.read(key)).toEqual({ revision: 1, value: "first" }); reopened.close();
  expect((await stat(path)).mode & 0o777).toBe(0o600);
});

it("serves the actual Cloudflare worker routes against its bound Durable Object and persists through restart", async () => {
  const h = harness(durable); h.clock.nowMs = Date.now();
  const account = email(); const first = await enroll(h, account); const second = await enroll(h, account);
  const listed = await mf.dispatchFetch("https://worker.test/v1/controller/devices", { headers: { Authorization: `Bearer ${first.access_token}` } });
  expect(listed.status).toBe(200);
  expect((await listed.json() as { devices: unknown[] }).devices).toHaveLength(2);
  const revoked = await mf.dispatchFetch(`https://worker.test/v1/controller/devices/${first.device.device_id}/revoke`, { method: "POST", headers: { Authorization: `Bearer ${second.access_token}`, "Content-Type": "application/json" }, body: JSON.stringify({ request_id: crypto.randomUUID(), expected_revision: 1 }) });
  expect(revoked.status).toBe(200);
  await mf.dispose(); mf = new Miniflare(runtimeOptions);
  durable = new DurableDeviceRegistry(await mf.getDurableObjectNamespace("LINK_DEVICE_REGISTRY") as unknown as DurableObjectNamespace);
  const denied = await mf.dispatchFetch("https://worker.test/v1/controller/devices", { headers: { Authorization: `Bearer ${first.access_token}` } });
  expect(denied.status).toBe(401);
  const sibling = await mf.dispatchFetch("https://worker.test/v1/controller/devices", { headers: { Authorization: `Bearer ${second.access_token}` } });
  expect(sibling.status).toBe(200);
}, 30_000);
it("rejects Origin, missing registry and a corrupt account without returning a successful empty registry", async () => {
  const h = harness(node); const d = await enroll(h); const claims = await verifyAccess(h.deps.config.jwtSecret!, d.access_token, Math.floor(h.clock.now() / 1000));
  const request = (): Request => new Request("https://link.test/v1/controller/devices", { headers: { Authorization: `Bearer ${d.access_token}` } });
  const origin = request(); origin.headers.set("Origin", "https://hostile.test");
  expect((await handleRequest(origin, h.deps)).status).toBe(403);
  expect((await handleRequest(request(), { ...h.deps, deviceRegistry: undefined })).status).toBe(503);
  const record = await node.read(claims!.account_key);
  await node.compareAndSwap(claims!.account_key, record!.revision, "{}");
  const response = await handleRequest(request(), h.deps);
  expect(response.status).toBe(503); expect(JSON.stringify(await response.json())).not.toContain(d.access_token);
});

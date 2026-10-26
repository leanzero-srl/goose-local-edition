import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { SqliteDeviceRegistry } from "../src/controller/node-registry";
import { Registry } from "../src/controller/registry";
import { ControllerService } from "../src/controller/service";
import { AccountDeletionService, type DeletionChallenge } from "../src/controller/account-deletion";
import { reserveLegacyMint } from "../src/controller/legacy-mesh";
import { reserveAccountOperation } from "../src/controller/account-lifecycle";
import { digest } from "../src/controller/crypto";
import { signJwt } from "../src/lib/jwt";
import { usernameForEmail } from "../src/lib/headscale";
import { handleRequest } from "../src/router";
import { FULL_ENV, jsonRes, makeHarness, postJson } from "./helpers";

const resources: { store: SqliteDeviceRegistry; dir: string }[] = [];
afterEach(async () => { for (const r of resources.splice(0)) { r.store.close(); await rm(r.dir, { recursive: true, force: true }); } });
async function fixture() {
  const dir = await mkdtemp(join(tmpdir(), "link-deletion-"));
  const store = new SqliteDeviceRegistry(join(dir, "registry.sqlite")); resources.push({ store, dir });
  const email = "delete-me@example.test", otherEmail = "keep-me@example.test";
  const users = [{ id: "1", name: await usernameForEmail(email) }, { id: "2", name: await usernameForEmail(otherEmail) }];
  const keys = users.map((user, i) => ({ id: String(11 + i), user, used: false, reusable: true, expiration: "2099-01-01T00:00:00Z" }));
  const nodes = users.map((user, i) => ({ id: String(21 + i), user, nodeKey: "nodekey:" + String(i + 1).repeat(64), preAuthKey: keys[i], ipAddresses: ["100.100.0.1"], online: true, expiry: null, tags: [] }));
  const control = { outage: false, segmentFailure: false, paginated: false, sharedContact: false, contactDeleted: false };
  const h = makeHarness({ env: { ...FULL_ENV, TS_API_TOKEN: undefined, TS_TAILNET: undefined, HEADSCALE_API_URL: "https://mesh.test", HEADSCALE_API_KEY: "test-api", HEADSCALE_LOGIN_SERVER: "https://login.test" }, fetchHandler: (url, init) => {
    const path = new URL(url).pathname; const method = init?.method ?? "GET";
    if (path === "/emails") return jsonRes(200, { id: "email" });
    if (url.startsWith("https://api.resend.com/contacts/")) {
      if (control.segmentFailure) return jsonRes(503, {});
      if (control.contactDeleted) return jsonRes(404, {});
      if (path.endsWith("/segments") && method === "GET") return jsonRes(200, { data: [{ id: "seg_test_audience" }, ...(control.sharedContact ? [{ id: "newsletter" }] : [])], has_more: false });
      if (path.endsWith("/topics")) return jsonRes(200, { data: [], has_more: false });
      if (method === "DELETE") { if (!path.includes("/segments/")) control.contactDeleted = true; return jsonRes(200, { deleted: true }); }
      return jsonRes(200, { email, id: "11111111-1111-4111-8111-111111111111" });
    }
    if (control.outage) return jsonRes(503, {});
    if (path === "/api/v1/policy") return jsonRes(200, { policy: JSON.stringify({ acls: [{ action: "accept", src: ["*"], dst: ["autogroup:self:*"] }] }) });
    if (path === "/api/v1/user") return jsonRes(200, { users: users.filter(u => u.name === new URL(url).searchParams.get("name")) });
    if (path === "/api/v1/preauthkey") return jsonRes(200, { preAuthKeys: keys, ...(control.paginated ? { nextPageToken: "more" } : {}) });
    if (path === "/api/v1/preauthkey/expire") { const id = JSON.parse(init!.body as string).id; keys.find(k => k.id === id)!.expiration = "2000-01-01T00:00:00Z"; return jsonRes(200, {}); }
    if (path === "/api/v1/node") return jsonRes(200, { nodes });
    if (path.startsWith("/api/v1/node/") && method === "DELETE") { const i = nodes.findIndex(n => n.id === path.split("/").at(-1)); if (i >= 0) nodes.splice(i, 1); return jsonRes(200, {}); }
    if (path.startsWith("/api/v1/user/") && method === "DELETE") { const id = path.split("/").at(-1); const i = users.findIndex(u => u.id === id); if (i >= 0) users.splice(i, 1); for (let n = keys.length - 1; n >= 0; n--) if (keys[n]!.user.id === id) keys.splice(n, 1); return jsonRes(200, {}); }
    throw new Error("Unexpected request " + path);
  } });
  h.deps.deviceRegistry = store; h.deps.randomOtp = () => "123456";
  const registry = new Registry(store), service = new ControllerService(registry, h.deps.config.jwtSecret!, h.deps.now);
  const enrollmentId = crypto.randomUUID(); const ch = await service.prepareChallenge(email, enrollmentId, "999999");
  await service.challengeSent(ch.challenge_id, enrollmentId, true);
  const credentials = await service.verify(ch.challenge_id, enrollmentId, "999999", "My phone", "ios");
  const deletion = new AccountDeletionService(service, h.deps);
  const post = (path: string, body: unknown, token = credentials.access_token) => handleRequest(postJson("/v1/controller" + path, body, { Authorization: `Bearer ${token}` }), h.deps);
  const request = async (): Promise<DeletionChallenge> => { const r = await post("/account/deletion/request-code", { request_id: crypto.randomUUID() }); expect(r.status).toBe(200); return r.json() as Promise<DeletionChallenge>; };
  return { ...h, store, registry, service, deletion, credentials, email, otherEmail, users, keys, nodes, control, post, request, enrollmentId, ch };
}

describe("account deletion", () => {
  it("erases the account and provider records while preserving a second account", async () => {
    const f = await fixture();
    for (const prefix of ["nodesecret:", "otp:", "rl:email:", "rl:verify:"]) { await f.kv.put(prefix + f.email, "remove"); await f.kv.put(prefix + f.otherEmail, "keep"); }
    const ch = await f.request();
    const same = await f.post("/account/deletion/request-code", { request_id: ch.request_id }); expect(await same.json()).toEqual(ch);
    const r = await f.post("/account/deletion/confirm", { code: "123456" }, ch.deletion_token);
    expect(r.status).toBe(200); expect(await r.json()).toMatchObject({ status: "deleted", pending: [] });
    expect(f.users.map(u => u.id)).toEqual(["2"]); expect(f.keys.map(k => k.id)).toEqual(["12"]); expect(f.nodes.map(n => n.id)).toEqual(["22"]);
    for (const prefix of ["nodesecret:", "otp:", "rl:email:", "rl:verify:"]) { expect(await f.kv.get(prefix + f.email)).toBeNull(); expect(await f.kv.get(prefix + f.otherEmail)).toBe("keep"); }
    const raw = (await f.store.read(await digest(f.email)))!.value;
    expect(raw).not.toContain(f.email); expect(raw).not.toContain("My phone"); expect(raw).not.toContain(f.credentials.refresh_token);
    await expect(f.service.authenticate(f.credentials.access_token)).rejects.toMatchObject({ code: "account_deleted" });
    await expect(f.service.refresh(f.credentials.device.device_id, f.credentials.refresh_token, crypto.randomUUID())).rejects.toMatchObject({ code: "account_deleted" });
    await expect(f.service.verify(f.ch.challenge_id, f.enrollmentId, "999999", "My phone", "ios")).rejects.toMatchObject({ code: "account_deleted" });
    const legacy = await signJwt(f.deps.config.jwtSecret!, { sub: f.email, iat: Math.floor(f.clock.now() / 1000), exp: Math.floor(f.clock.now() / 1000) + 1000, ver: 1 });
    const r2 = await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: `Bearer ${legacy}` }), f.deps); expect(r2.status).toBe(401);
    const oldCalls = f.calls.length; expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" }); expect(f.calls.length).toBe(oldCalls);
  });
  it("rejects wrong, sign-in-purpose, and exhausted codes without revoking access", async () => {
    const f = await fixture(); const ch = await f.request();
    for (let n = 0; n < 5; n++) expect((await f.post("/account/deletion/confirm", { code: "999999" }, ch.deletion_token)).status).toBe(401);
    expect((await f.post("/account/deletion/confirm", { code: "123456" }, ch.deletion_token)).status).toBe(401);
    await expect(f.service.authenticate(f.credentials.access_token)).resolves.toBeDefined(); expect(f.users).toHaveLength(2);
  });
  it("persists pending cleanup and resumes after OTP and access token expiration", async () => {
    const f = await fixture(); const ch = await f.request(); f.control.outage = true;
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["mesh"] });
    await expect(f.service.authenticate(f.credentials.access_token)).rejects.toMatchObject({ code: "account_deleted" });
    f.clock.advanceSeconds(86400); f.control.outage = false;
    const restarted = new AccountDeletionService(new ControllerService(new Registry(f.store), f.deps.config.jwtSecret!, f.deps.now), f.deps);
    expect(await restarted.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
  });
  it("rejects forged receipts and normal credentials at the deletion-only boundary", async () => {
    const f = await fixture(); const ch = await f.request();
    expect((await f.post("/account/deletion/confirm", { code: "123456" })).status).toBe(401);
    const forged = ch.deletion_token.slice(0, -1) + (ch.deletion_token.endsWith("A") ? "B" : "A");
    expect((await f.post("/account/deletion/confirm", { code: "123456" }, forged)).status).toBe(401);
    expect((await f.post("/account/deletion/status", {}, ch.deletion_token)).status).toBe(409); expect(f.users).toHaveLength(2);
  });
  it("waits for an admitted legacy write and then deletes its late records", async () => {
    const f = await fixture(); const release = await reserveAccountOperation(f.deps, f.email); const ch = await f.request();
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["in_flight_requests"] });
    await f.kv.put("nodesecret:" + f.email, "late secret"); await release.release();
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" }); expect(await f.kv.get("nodesecret:" + f.email)).toBeNull();
  });
  it("refuses incomplete provider listings and reports contact cleanup failures", async () => {
    const f = await fixture(); const ch = await f.request(); f.control.paginated = true; f.control.segmentFailure = true;
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["mesh", "contact"] });
    expect(f.users).toHaveLength(2); f.control.paginated = false; f.control.segmentFailure = false;
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
  });
  it("permits fresh signup with a new account identity while retiring every old credential", async () => {
    const f = await fixture(); const oldIdentity = (await f.service.authenticate(f.credentials.access_token)).state.account_id;
    const iat = Math.floor(f.clock.now() / 1000);
    const legacy = await signJwt(f.deps.config.jwtSecret!, { sub: f.email, iat, exp: iat + 1000, ver: 1 });
    const ch = await f.request(); expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "deleted" });
    const nextId = crypto.randomUUID(); const next = await f.service.prepareChallenge(f.email, nextId, "654321");
    await f.service.challengeSent(next.challenge_id, nextId, true);
    const fresh = await f.service.verify(next.challenge_id, nextId, "654321", "New phone", "ios");
    expect((await f.service.authenticate(fresh.access_token)).state.account_id).not.toBe(oldIdentity);
    await expect(f.service.authenticate(f.credentials.access_token)).rejects.toMatchObject({ code: "invalid_credential" });
    await expect(f.service.refresh(f.credentials.device.device_id, f.credentials.refresh_token, crypto.randomUUID())).rejects.toMatchObject({ code: "invalid_credential" });
    expect((await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: `Bearer ${legacy}` }), f.deps)).status).toBe(401);
    expect((await f.post("/hosts/register", { owner_node_id: crypto.randomUUID(), registration_id: crypto.randomUUID(), host_name: "Old Mac", protocol_version: 1 }, legacy)).status).toBe(401);
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
  });
  it("keeps a concurrent cleanup inside the deletion barrier until every call finishes", async () => {
    const f = await fixture(); const ch = await f.request(); f.control.outage = true;
    await f.deletion.confirm(ch.deletion_token, "123456"); f.control.outage = false;
    const replies = await Promise.all(Array.from({ length: 5 }, () => f.deletion.status(ch.deletion_token)));
    expect(replies.every(r => r.status === "deleted" || r.pending.length > 0)).toBe(true);
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
  });
  it("rejects expired confirmation codes and Origin before any deletion side effect", async () => {
    const f = await fixture(); const ch = await f.request(); f.clock.advanceSeconds(601);
    expect((await f.post("/account/deletion/confirm", { code: "123456" }, ch.deletion_token)).status).toBe(401);
    const r = await handleRequest(postJson("/v1/controller/account/deletion/status", {}, { Origin: "https://attacker.test", Authorization: `Bearer ${ch.deletion_token}` }), f.deps);
    expect(r.status).toBe(403); expect(f.users).toHaveLength(2);
  });

  it("preserves an independent subscription but removes Link's membership and discloses retention", async () => {
    const f = await fixture(); f.control.sharedContact = true; const ch = await f.request();
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "deleted", retained: ["security_revocation_record", "historical_service_records", "shared_contact_record"] });
    expect(f.control.contactDeleted).toBe(false);
    expect(f.calls.some(c => c.init?.method === "DELETE" && c.url.endsWith("/segments/seg_test_audience"))).toBe(true);
  });

  it("reclaims abandoned local auth and cleanup leases, fencing a resumed old writer", async () => {
    const f = await fixture(); const abandoned = await reserveAccountOperation(f.deps, f.email); const ch = await f.request();
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending" });
    await f.registry.update(await digest(f.email), state => { state!.deletion!.cleanup_lease = { id: crypto.randomUUID(), expires_at: f.clock.now() + 60_000 }; return { state: state!, result: undefined }; }, true);
    f.clock.advanceSeconds(61);
    const restarted = new AccountDeletionService(new ControllerService(new Registry(f.store), f.deps.config.jwtSecret!, f.deps.now), f.deps);
    expect(await restarted.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
    await expect(abandoned.deps.kv.put("nodesecret:" + f.email, "stale")).rejects.toMatchObject({ code: "account_operation_expired" });
    expect(await f.kv.get("nodesecret:" + f.email)).toBeNull();
  });
  it("includes late-issued admitted legacy JWTs in the final revocation cutoff", async () => {
    const f = await fixture(); const admitted = await reserveAccountOperation(f.deps, f.email); const ch = await f.request();
    await f.deletion.confirm(ch.deletion_token, "123456"); f.clock.advanceSeconds(2);
    const iat = Math.floor(f.clock.now() / 1000);
    const lateToken = await signJwt(f.deps.config.jwtSecret!, { sub: f.email, iat, exp: iat + 1000, ver: 1 });
    await admitted.release(); expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
    await f.service.prepareChallenge(f.email, crypto.randomUUID(), "654321");
    expect((await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: `Bearer ${lateToken}` }), f.deps)).status).toBe(401);
  });
  it("does not call an abandoned external create successful merely because its lease elapsed", async () => {
    const f = await fixture(); const pending = await reserveAccountOperation(f.deps, f.email);
    const original = f.deps.fetchFn; f.deps.fetchFn = async () => { throw new Error("connection lost"); };
    await expect(pending.deps.fetchFn("https://api.resend.com/contacts", { method: "POST" })).rejects.toThrow();
    f.deps.fetchFn = original; await pending.release(); const ch = await f.request(); f.clock.advanceSeconds(61);
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["legacy_write_outcome_unknown"] });
  });

  it.each([
    ["contact", "https://api.resend.com/contacts"],
    ["segment", "https://api.resend.com/contacts/person%40example.test/segments/11111111-1111-4111-8111-111111111111"],
    ["activity", "https://example.api.sanity.io/v2025-01-01/data/mutate/production"],
  ])("keeps %s CREATE server failures and malformed success pending across restart", async (_kind, url) => {
    for (const response of [jsonRes(503, {}), jsonRes(200, {}), new Response("invalid json", { status: 200 })]) {
      const f = await fixture(); const operation = await reserveAccountOperation(f.deps, f.email);
      const original = f.deps.fetchFn; f.deps.fetchFn = async () => response;
      await operation.deps.fetchFn(url, { method: "POST", body: JSON.stringify({ mutations: [{ create: { _id: "siteEvent.test" } }] }) });
      f.deps.fetchFn = original; await operation.release();
      const saved = await f.registry.read(await digest(f.email));
      expect(Object.values(saved!.legacy_operations ?? {}).map(op => op.external_pending)).toEqual([1]);
      const ch = await f.request(); f.clock.advanceSeconds(61);
      expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["legacy_write_outcome_unknown"] });
      f.clock.advanceSeconds(86400);
      const restarted = new AccountDeletionService(new ControllerService(new Registry(f.store), f.deps.config.jwtSecret!, f.deps.now), f.deps);
      expect(await restarted.status(ch.deletion_token)).toMatchObject({ status: "pending", pending: ["legacy_write_outcome_unknown"] });
    }
  });

  it.each([
    ["contact", "https://api.resend.com/contacts", { object: "contact", id: "11111111-1111-4111-8111-111111111111" }],
    ["segment", "https://api.resend.com/contacts/person%40example.test/segments/11111111-1111-4111-8111-111111111111", { id: "11111111-1111-4111-8111-111111111111" }],
    ["activity", "https://example.api.sanity.io/v2025-01-01/data/mutate/production", { transactionId: "transaction", results: [{ id: "siteEvent.test", operation: "create" }] }],
  ])("settles %s CREATE only on a valid success or definitive client rejection", async (_kind, url, body) => {
    for (const response of [jsonRes(200, body), jsonRes(409, { error: "already exists" })]) {
      const f = await fixture(); const operation = await reserveAccountOperation(f.deps, f.email);
      const original = f.deps.fetchFn; f.deps.fetchFn = async () => response;
      await operation.deps.fetchFn(url, { method: "POST", body: JSON.stringify({ mutations: [{ create: { _id: "siteEvent.test" } }] }) });
      f.deps.fetchFn = original; await operation.release();
      expect(Object.keys((await f.registry.read(await digest(f.email)))!.legacy_operations ?? {})).toEqual([]);
      const ch = await f.request(); expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "deleted" });
    }
  });

  it("recovers a crashed legacy mint before its first external request and fences its resume", async () => {
    const f = await fixture(); const abandoned = await reserveLegacyMint(f.deps, f.email); const ch = await f.request();
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["in_flight_requests"] });
    f.clock.advanceSeconds(61);
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "deleted" });
    await f.service.prepareChallenge(f.email, crypto.randomUUID(), "654321");
    await expect(abandoned!.beginExternal()).rejects.toMatchObject({ code: "legacy_mint_expired" });
    expect(f.users.map(u => u.id)).toEqual(["2"]);
  });

  it("does not complete deletion before a response-lost provider CREATE commits late", async () => {
    const f = await fixture(); const own = f.users.find(u => u.id === "1")!;
    f.users.splice(f.users.indexOf(own), 1);
    const original = f.deps.fetchFn;
    let commitLate: (() => void) | undefined;
    f.deps.fetchFn = async (url, init) => {
      if (new URL(url).pathname === "/api/v1/user" && init?.method === "POST") {
        commitLate = () => { f.users.push({ ...own, id: "99" }); };
        throw new Error("provider response lost before commit becomes visible");
      }
      return original(url, init);
    };
    const iat = Math.floor(f.clock.now() / 1000);
    const token = await signJwt(f.deps.config.jwtSecret!, { sub: f.email, iat, exp: iat + 1000, ver: 1 });
    const mint = await handleRequest(postJson("/v1/mesh/join-key", {}, { Authorization: `Bearer ${token}` }), f.deps);
    expect(mint.status).toBe(502); expect(commitLate).toBeDefined();
    const ch = await f.request();
    expect(await f.deletion.confirm(ch.deletion_token, "123456")).toMatchObject({ status: "pending", pending: ["legacy_mesh_outcome_unknown"] });
    commitLate!();
    expect(await f.deletion.status(ch.deletion_token)).toMatchObject({ status: "pending", pending: ["legacy_mesh_outcome_unknown"] });
  });

});

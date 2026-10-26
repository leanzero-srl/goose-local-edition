import type { Deps } from "../lib/deps";
import { policyIsolates, usernameForEmail } from "../lib/headscale";
import { parseHujson } from "../lib/hujson";
import { fail } from "./registry";

export interface ProviderNode {
  id: string; node_key: string; user_id: string; key_id: string | null;
  ips: string[]; online: boolean; expires_at: number | null; tagged: boolean;
}
export interface ProviderKey { id: string; key: string; user_id: string; expires_at: number }
const NODE_KEY = /^nodekey:[0-9a-f]{64}$/;
export function nodeKey(value: unknown): string {
  if (typeof value !== "string" || !NODE_KEY.test(value)) fail(400, "invalid_request", "mesh_node_key must be a canonical node key");
  return value;
}
function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(502, "provider_invalid", "Provider returned an invalid object");
  return value as Record<string, unknown>;
}
function numericId(value: unknown): string {
  if (typeof value !== "string" || !/^[1-9][0-9]*$/.test(value)) fail(502, "provider_invalid", "Provider identity is missing");
  return value;
}
function expiration(value: unknown): number | null {
  // https://github.com/juanfont/headscale/blob/v0.29.3/hscontrol/types/node.go
  // IsExpired treats nil and Go's zero timestamp as no expiry.
  if (value === undefined || value === null || value === "0001-01-01T00:00:00Z") return null;
  if (typeof value !== "string" || !Number.isFinite(Date.parse(value))) fail(502, "provider_invalid", "Provider expiry is invalid");
  return Date.parse(value);
}
function meshIp(value: unknown): string {
  if (typeof value !== "string") fail(502, "provider_invalid", "Provider address is invalid");
  const p = value.split(".");
  if (p.length === 4 && p.every(x => /^(0|[1-9][0-9]{0,2})$/.test(x) && Number(x) <= 255) && p[0] === "100" && Number(p[1]) >= 64 && Number(p[1]) <= 127) return value;
  if (/^fd7a:115c:a1e0:[0-9a-f:]+$/i.test(value)) {
    try { const u = new URL("https://[" + value + "]/"); if (u.hostname.startsWith("[")) return u.hostname.slice(1, -1); } catch { /* invalid literal */ }
  }
  fail(502, "provider_invalid", "Provider address is outside the mesh ranges");
}
export function usable(node: ProviderNode, now: number): boolean {
  return !node.tagged && (node.expires_at === null || node.expires_at > now);
}

/** Exact enrollment linkage is available in Headscale v0.29.3's REST DTOs:
 * https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/node.proto
 * https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/preauthkey.proto
 * https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/headscale.proto
 * Shared Tailscale tagged keys do not establish our per-account enrollment identity:
 * https://tailscale.com/docs/features/access-control/auth-keys
 */
// Headscale uses grpcRuntime.NewServeMux() with the default JSONPb marshaller:
// https://github.com/juanfont/headscale/blob/v0.29.3/hscontrol/app.go
// https://github.com/grpc-ecosystem/grpc-gateway/blob/v2.27.1/runtime/marshaler_registry.go
// EmitUnpopulated=true makes false/empty protobuf fields explicit on this REST API.
export class MeshProvider {
  constructor(private readonly deps: Deps) {}
  private configured(): void {
    const c = this.deps.config;
    if (c.meshConfigError || c.tsKeyExpiryInvalid) fail(503, "mesh_unconfigured", "Mesh provider configuration is invalid");
    if (c.meshProvider === "none") fail(501, "mesh_unconfigured", "Mesh provider is not configured");
    if (c.meshProvider !== "headscale") fail(501, "provider_binding_unsupported", "This provider has no configured account enrollment attestation");
    if (!c.hsApiUrl || !c.hsApiKey || !c.hsLoginServer) fail(503, "mesh_unconfigured", "Headscale configuration is incomplete");
  }
  async request(path: string, method = "GET", body?: unknown, absentOk = false): Promise<Record<string, unknown>> {
    this.configured();
    let response: Response;
    try {
      response = await this.deps.fetchFn(this.deps.config.hsApiUrl + path, { method, redirect: "error", headers: { Authorization: "Bearer " + this.deps.config.hsApiKey, "Content-Type": "application/json" }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    } catch { fail(502, "provider_unavailable", "Mesh provider is unavailable"); }
    if (absentOk && response.status === 404) return {};
    if (!response.ok) fail(502, "provider_failed", "Mesh provider rejected the operation");
    if (response.status === 204) return {};
    const value = record(await response.json().catch(() => null));
    // v0.29.3 lists are complete and unpaginated. Refuse an unimplemented paging
    // extension: a partial list cannot prove that no legacy hosts remain.
    if (response.headers.has("Link") || ["nextPageToken", "next_page_token", "next", "cursor"].some(k => value[k] !== undefined && value[k] !== null && value[k] !== "")) fail(502, "provider_pagination_unsupported", "Provider listing is incomplete");
    return value;
  }
  async assertIsolation(): Promise<void> {
    const p = await this.request("/api/v1/policy");
    const parsed = typeof p.policy === "string" ? parseHujson(p.policy) : null;
    if (!parsed?.ok || !policyIsolates(parsed.value)) fail(503, "mesh_isolation_unverified", "Provider must enforce account isolation");
  }
  async user(email: string): Promise<{ id: string; name: string }> {
    const name = await usernameForEmail(email);
    const body = await this.request("/api/v1/user?name=" + encodeURIComponent(name));
    if (!Array.isArray(body.users)) fail(502, "provider_invalid", "Provider user list is missing");
    const found = body.users.map(record).filter(u => u.name === name);
    if (found.length !== 1) fail(409, "mesh_account_unregistered", "Account must have exactly one provider user");
    return { id: numericId(found[0]!.id), name };
  }
  async nodes(email: string): Promise<{ user: { id: string; name: string }; nodes: ProviderNode[] }> {
    await this.assertIsolation();
    const user = await this.user(email);
    const body = await this.request("/api/v1/node");
    if (!Array.isArray(body.nodes)) fail(502, "provider_invalid", "Provider node list is missing");
    const nodes: ProviderNode[] = [];
    for (const raw of body.nodes) {
      const n = record(raw);
      const u = n.user === undefined || n.user === null ? null : record(n.user);
      if (u?.id !== user.id && u?.name !== user.name) continue;
      if (u?.id !== user.id || u.name !== user.name) fail(502, "provider_account_mismatch", "Provider account identity disagrees");
      if (typeof n.nodeKey !== "string" || !NODE_KEY.test(n.nodeKey) || !Array.isArray(n.ipAddresses) || typeof n.online !== "boolean") fail(502, "provider_invalid", "Provider node identity is incomplete");
      if (n.tags !== undefined && !Array.isArray(n.tags)) fail(502, "provider_invalid", "Provider tags are invalid");
      const pre = n.preAuthKey === undefined || n.preAuthKey === null ? null : record(n.preAuthKey);
      nodes.push({ id: numericId(n.id), node_key: n.nodeKey, user_id: user.id, key_id: pre ? numericId(pre.id) : null, ips: n.ipAddresses.map(meshIp), online: n.online, expires_at: expiration(n.expiry), tagged: Array.isArray(n.tags) && n.tags.length > 0 });
    }
    if (new Set(nodes.map(n => n.id)).size !== nodes.length || new Set(nodes.map(n => n.node_key)).size !== nodes.length) fail(502, "provider_ambiguous", "Provider node identities are ambiguous");
    return { user, nodes };
  }
  async assertNoUntrackedKeys(userId: string, known: Set<string>): Promise<void> {
    const body = await this.request("/api/v1/preauthkey");
    if (!Array.isArray(body.preAuthKeys)) fail(502, "provider_invalid", "Provider key list is missing");
    for (const raw of body.preAuthKeys) {
      const k = record(raw); const u = record(k.user);
      if (u.id !== userId) continue;
      const id = numericId(k.id); const expires = expiration(k.expiration);
      if (typeof k.used !== "boolean" || typeof k.reusable !== "boolean") fail(502, "provider_invalid", "Provider key usage is missing");
      if ((expires === null || expires > this.deps.now()) && (k.reusable || !k.used) && !known.has(id)) fail(409, "update_required", "Outstanding legacy mesh credentials must expire before controller enrollment");
    }
  }
  async mint(userId: string, expiresAt: number): Promise<ProviderKey> {
    const b = await this.request("/api/v1/preauthkey", "POST", { user: userId, reusable: false, ephemeral: true, expiration: new Date(expiresAt).toISOString() });
    const k = record(b.preAuthKey); const u = record(k.user);
    if (numericId(u.id) !== userId || k.reusable !== false || k.ephemeral !== true || typeof k.key !== "string" || k.key.length === 0 || (Array.isArray(k.aclTags) && k.aclTags.length > 0)) fail(502, "provider_invalid", "Provider key is not scoped to enrollment");
    const expires = expiration(k.expiration);
    if (expires === null || expires > expiresAt || expires <= this.deps.now()) fail(502, "provider_invalid", "Provider key lifetime is invalid");
    return { id: numericId(k.id), key: k.key, user_id: userId, expires_at: expires };
  }
  async expireKey(id: string): Promise<void> { await this.request("/api/v1/preauthkey/expire", "POST", { id }, true); }
  async deleteNode(id: string): Promise<void> { await this.request("/api/v1/node/" + encodeURIComponent(id), "DELETE", undefined, true); }
  get loginServer(): string { this.configured(); return this.deps.config.hsLoginServer!; }
  get keySeconds(): number { this.configured(); return this.deps.config.tsKeyExpirySeconds; }
}

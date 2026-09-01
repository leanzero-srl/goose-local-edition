import type { Deps } from "./deps";
import { safeText, truncate } from "./http";
import { parseHujson } from "./hujson";

// Headscale-backed mesh keys — the self-hosted, multi-tenant path. Each account maps to
// one Headscale user (the isolation unit); a node joins with a per-user ephemeral preauth
// key and, under the isolation policy below, sees ONLY nodes of its own account.
//
// Every request/response shape here was measured live against Headscale v0.29.3's
// gRPC-gateway REST API before this shipped:
//   GET  /api/v1/user?name={name}   -> { "users": [ { "id": "<string>", "name": ... } ] }
//   POST /api/v1/user  { "name" }   -> { "user": { "id": "<string>", ... } }
//   POST /api/v1/preauthkey  { "user": "<numeric id string>", "reusable", "ephemeral",
//        "expiration": "<RFC3339>" }  -> { "preAuthKey": { "key": "hskey-auth-...", ... } }
//        — `user` MUST be the numeric id; the username is rejected ("invalid value for
//        uint64 field user").
//   GET  /api/v1/policy             -> { "policy": "<hujson>", "updatedAt": ... }
//   PUT  /api/v1/policy  { "policy": "<hujson>" }  -> { "policy": ... }
// Auth is `Authorization: Bearer <hskey-api-...>`.

export type HeadscaleMint =
  | { ok: true; authKey: string; loginServer: string; expirySeconds: number }
  | { ok: false; status: number; detail: string };

// The per-account isolation policy: every node may reach ONLY nodes of its own account
// (Headscale user) via `autogroup:self`. One static policy for all accounts — a node's
// netmap is filtered to same-user peers, so account A never sees account B. Proven live
// (two accounts, three nodes: same-account visible, cross-account not) before shipping.
const ISOLATION_POLICY =
  '{\n  "acls": [\n    { "action": "accept", "src": ["*"], "dst": ["autogroup:self:*"] }\n  ]\n}';

/// A stable Headscale username derived from the account email — one user per account is
/// the isolation unit. sha256(email) → first 16 hex: no PII in the username, always a
/// valid Headscale name (lowercase alphanumeric + dash).
export async function usernameForEmail(email: string): Promise<string> {
  const data = new TextEncoder().encode(email.trim().toLowerCase());
  const digest = await crypto.subtle.digest("SHA-256", data);
  const hex = Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return `acct-${hex.slice(0, 16)}`;
}

interface HsResult {
  status: number;
  ok: boolean;
  json: unknown;
  text: string;
}

async function hsFetch(deps: Deps, path: string, init?: RequestInit): Promise<HsResult> {
  const { hsApiUrl, hsApiKey } = deps.config;
  const response = await deps.fetchFn(`${hsApiUrl}${path}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${hsApiKey}`,
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  const text = await safeText(response);
  let json: unknown = null;
  try {
    json = JSON.parse(text);
  } catch {
    json = null;
  }
  return { status: response.status, ok: response.ok, json, text };
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : null;
}

function isSelfOnly(dst: unknown): boolean {
  return typeof dst === "string" && (dst === "autogroup:self" || dst.startsWith("autogroup:self:"));
}

function sectionIsEmpty(section: unknown): boolean {
  return section === undefined || section === null || (Array.isArray(section) && section.length === 0);
}

/// A PARSED policy enforces per-account isolation only if EVERY reachability rule keeps a
/// node inside its own account: at least one acl, every acl an `accept` whose every dst is
/// `autogroup:self[:port]`, and no `ssh` or `grants` section (either can open a path the
/// acls do not). One extra allow-all rule beside the self rule used to pass — `.some()` —
/// and a key was minted into a server where every account could see every other.
export function policyIsolates(policy: unknown): boolean {
  const p = asRecord(policy);
  if (p === null) {
    return false;
  }
  const acls = p.acls;
  if (!Array.isArray(acls) || acls.length === 0) {
    return false;
  }
  for (const rule of acls) {
    const r = asRecord(rule);
    if (r === null || r.action !== "accept") {
      return false;
    }
    const dst = r.dst;
    if (!Array.isArray(dst) || dst.length === 0 || !dst.every(isSelfOnly)) {
      return false;
    }
  }
  return sectionIsEmpty(p.ssh) && sectionIsEmpty(p.grants);
}

/// Refuse to mint into a Headscale whose policy does not enforce per-account isolation:
/// GET the policy (HuJSON — Headscale returns it verbatim), and PUT the isolation policy
/// only when the server has NONE or the parsed one does not isolate (self-healing). A
/// policy that cannot be parsed is NEVER overwritten — an operator's intent we could not
/// read is not ours to replace — and the mint is refused. A key must NEVER be handed out
/// against an allow-all server. Any API failure here fails the mint loudly.
async function ensureIsolationPolicy(deps: Deps): Promise<{ ok: true } | { ok: false; status: number; detail: string }> {
  const got = await hsFetch(deps, "/api/v1/policy");
  if (!got.ok) {
    deps.log("headscale_policy_get_failed", { status: got.status, detail: truncate(got.text) });
    return { ok: false, status: got.status, detail: "headscale policy read failed" };
  }
  const policyText = asRecord(got.json)?.policy;
  if (typeof policyText !== "string") {
    deps.log("headscale_policy_unparseable", { error: "response carried no policy string", detail: truncate(got.text) });
    return { ok: false, status: got.status, detail: "headscale policy response had no policy string" };
  }
  let reason: string;
  if (policyText.trim().length === 0) {
    reason = "no policy set";
  } else {
    const parsed = parseHujson(policyText);
    if (!parsed.ok) {
      deps.log("headscale_policy_unparseable", { error: parsed.error, head: truncate(policyText, 120) });
      return { ok: false, status: got.status, detail: "headscale policy unparseable; refusing to overwrite it" };
    }
    if (policyIsolates(parsed.value)) {
      return { ok: true };
    }
    reason = "policy does not isolate accounts";
  }
  deps.log("headscale_policy_healing", { detail: `${reason}; setting the isolation policy` });
  const put = await hsFetch(deps, "/api/v1/policy", {
    method: "PUT",
    body: JSON.stringify({ policy: ISOLATION_POLICY }),
  });
  if (!put.ok) {
    deps.log("headscale_policy_set_failed", { status: put.status, detail: truncate(put.text) });
    return { ok: false, status: put.status, detail: "headscale isolation policy could not be set" };
  }
  return { ok: true };
}

type UserResult = { ok: true; id: string } | { ok: false; status: number; detail: string };

function findUserId(json: unknown, username: string): string | undefined {
  const users = asRecord(json)?.users;
  if (!Array.isArray(users)) {
    return undefined;
  }
  for (const entry of users) {
    const u = asRecord(entry);
    if (u?.name === username && typeof u.id === "string") {
      return u.id;
    }
  }
  return undefined;
}

/// Find the account's Headscale user by name, creating it if absent. A create that races
/// another (UNIQUE constraint) re-reads and returns the winner, so concurrent connects
/// from the same account converge on one user.
async function ensureUser(deps: Deps, username: string): Promise<UserResult> {
  const listed = await hsFetch(deps, `/api/v1/user?name=${encodeURIComponent(username)}`);
  if (!listed.ok) {
    deps.log("headscale_user_list_failed", { status: listed.status, detail: truncate(listed.text) });
    return { ok: false, status: listed.status, detail: "headscale user lookup failed" };
  }
  const existing = findUserId(listed.json, username);
  if (existing !== undefined) {
    return { ok: true, id: existing };
  }
  const created = await hsFetch(deps, "/api/v1/user", {
    method: "POST",
    body: JSON.stringify({ name: username }),
  });
  if (created.ok) {
    const id = asRecord(asRecord(created.json)?.user)?.id;
    if (typeof id === "string" && id.length > 0) {
      deps.log("headscale_user_created", { username });
      return { ok: true, id };
    }
  }
  // Create failed or returned no id — re-read (a concurrent create may have won).
  const reread = await hsFetch(deps, `/api/v1/user?name=${encodeURIComponent(username)}`);
  const raced = reread.ok ? findUserId(reread.json, username) : undefined;
  if (raced !== undefined) {
    return { ok: true, id: raced };
  }
  deps.log("headscale_user_create_failed", { status: created.status, detail: truncate(created.text) });
  return { ok: false, status: created.status, detail: "headscale user create failed" };
}

/// Mint an ephemeral, single-use preauth key for the account's isolated mesh, returning it
/// with the PUBLIC login server the node must join. Never returns a fabricated key: a 2xx
/// with no `key` field is a loud failure.
export async function mintHeadscaleJoinKey(deps: Deps, email: string): Promise<HeadscaleMint> {
  const { hsApiUrl, hsApiKey, hsLoginServer, tsKeyExpirySeconds } = deps.config;
  if (!hsApiUrl || !hsApiKey || !hsLoginServer) {
    return { ok: false, status: 0, detail: "headscale mesh not configured" };
  }

  const policy = await ensureIsolationPolicy(deps);
  if (!policy.ok) {
    return policy;
  }

  const username = await usernameForEmail(email);
  const user = await ensureUser(deps, username);
  if (!user.ok) {
    return user;
  }

  const expiration = new Date(deps.now() + tsKeyExpirySeconds * 1000).toISOString();
  const minted = await hsFetch(deps, "/api/v1/preauthkey", {
    method: "POST",
    body: JSON.stringify({ user: user.id, reusable: false, ephemeral: true, expiration }),
  });
  if (!minted.ok) {
    deps.log("headscale_key_mint_failed", { status: minted.status, detail: truncate(minted.text) });
    return { ok: false, status: minted.status, detail: truncate(minted.text) };
  }
  const key = asRecord(asRecord(minted.json)?.preAuthKey)?.key;
  if (typeof key !== "string" || key.length === 0) {
    deps.log("headscale_key_missing", { detail: "2xx response did not contain a preAuthKey.key" });
    return { ok: false, status: minted.status, detail: "headscale response did not contain a key" };
  }
  deps.log("headscale_join_key_minted", { username, expirySeconds: tsKeyExpirySeconds });
  return { ok: true, authKey: key, loginServer: hsLoginServer, expirySeconds: tsKeyExpirySeconds };
}

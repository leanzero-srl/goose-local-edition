import type { Deps } from "./deps";
import { safeText, truncate } from "./http";

export const TAILSCALE_API_BASE = "https://api.tailscale.com";

export type JoinKeyMint =
  | { ok: true; authKey: string; expirySeconds: number }
  | { ok: false; status: number; detail: string };

// Auth-key creation — verified against https://tailscale.com/api (Tailscale API v2 reference;
// the same request/response shape is published in the tailscale/tailscale repo's api.md,
// "Tailnet keys" section):
//   POST https://api.tailscale.com/api/v2/tailnet/{tailnet}/keys
//   Body: { "capabilities": { "devices": { "create": {
//            "reusable", "ephemeral", "preauthorized", "tags" } } },
//          "expirySeconds": <seconds; "Defaults to 90 days if not supplied">,
//          "description": <string> }
//   Response: { "id", "key": "tskey-...", "created", "expires", "capabilities" }
//   — "key" is the one-time secret auth key.
//   Auth: "an API access token" supplied as "the username portion of HTTP Basic
//   authentication (leave the password blank) or as an OAuth Bearer token".
// OAuth clients — verified against https://tailscale.com/kb/1215/oauth-clients:
//   token exchange: POST https://api.tailscale.com/api/v2/oauth/token with form-encoded
//   client_id / client_secret / grant_type=client_credentials → { "access_token": ... }.
//   "All auth keys created from an OAuth client must use tags" — so an OAuth-client token
//   REQUIRES a tag; a plain API access token (Basic auth) may mint UNTAGGED keys. When
//   tsNodeTag is undefined (TS_NODE_TAG explicitly empty) the `tags` field is OMITTED, for
//   personal tailnets that don't own tag:leanzero-link in their ACL (tagged mint rejected,
//   untagged accepted — measured live).

type AuthHeader = { ok: true; header: string } | { ok: false; status: number; detail: string };

async function resolveAuthHeader(deps: Deps, token: string): Promise<AuthHeader> {
  const separator = token.indexOf(":");
  if (separator === -1) {
    return { ok: true, header: `Basic ${btoa(`${token}:`)}` };
  }
  const clientId = token.slice(0, separator);
  const clientSecret = token.slice(separator + 1);
  const form = new URLSearchParams({
    grant_type: "client_credentials",
    client_id: clientId,
    client_secret: clientSecret,
  });
  try {
    const response = await deps.fetchFn(`${TAILSCALE_API_BASE}/api/v2/oauth/token`, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: form.toString(),
    });
    if (!response.ok) {
      const detail = truncate(await safeText(response));
      deps.log("tailscale_oauth_failed", { status: response.status, detail });
      return { ok: false, status: response.status, detail: "oauth token exchange failed" };
    }
    const parsed: unknown = await response.json().catch(() => null);
    const accessToken =
      typeof parsed === "object" && parsed !== null ? (parsed as Record<string, unknown>).access_token : undefined;
    if (typeof accessToken !== "string" || accessToken.length === 0) {
      deps.log("tailscale_oauth_malformed", { detail: "response did not contain access_token" });
      return { ok: false, status: response.status, detail: "oauth token exchange returned no access_token" };
    }
    return { ok: true, header: `Bearer ${accessToken}` };
  } catch (error) {
    deps.log("tailscale_oauth_error", { error: String(error) });
    return { ok: false, status: 0, detail: String(error) };
  }
}

export async function mintJoinKey(deps: Deps, email: string): Promise<JoinKeyMint> {
  const { tsApiToken, tsTailnet, tsNodeTag, tsKeyExpirySeconds } = deps.config;
  if (!tsApiToken || !tsTailnet) {
    return { ok: false, status: 0, detail: "mesh not configured" };
  }
  const auth = await resolveAuthHeader(deps, tsApiToken);
  if (!auth.ok) {
    return auth;
  }
  const body = {
    capabilities: {
      devices: {
        create: {
          reusable: false,
          ephemeral: true,
          preauthorized: true,
          ...(tsNodeTag ? { tags: [tsNodeTag] } : {}),
        },
      },
    },
    expirySeconds: tsKeyExpirySeconds,
    // Tailscale key descriptions reject '@' and '.' (measured); sanitize the email to [A-Za-z0-9_-].
    description: `leanzero-link join key for ${email.replace(/[^A-Za-z0-9_-]/g, "_")}`,
  };
  try {
    const response = await deps.fetchFn(
      `${TAILSCALE_API_BASE}/api/v2/tailnet/${encodeURIComponent(tsTailnet)}/keys`,
      {
        method: "POST",
        headers: { Authorization: auth.header, "Content-Type": "application/json" },
        body: JSON.stringify(body),
      },
    );
    if (!response.ok) {
      const detail = truncate(await safeText(response));
      deps.log("tailscale_key_mint_failed", { status: response.status, detail });
      return { ok: false, status: response.status, detail };
    }
    const parsed: unknown = await response.json().catch(() => null);
    const key = typeof parsed === "object" && parsed !== null ? (parsed as Record<string, unknown>).key : undefined;
    if (typeof key !== "string" || key.length === 0) {
      deps.log("tailscale_key_missing", { detail: "2xx response did not contain a key field" });
      return { ok: false, status: response.status, detail: "tailscale response did not contain a key" };
    }
    deps.log("join_key_minted", { email, expirySeconds: tsKeyExpirySeconds });
    return { ok: true, authKey: key, expirySeconds: tsKeyExpirySeconds };
  } catch (error) {
    deps.log("tailscale_error", { error: String(error) });
    return { ok: false, status: 0, detail: String(error) };
  }
}

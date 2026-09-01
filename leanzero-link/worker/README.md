# LeanZero Link — auth worker

A single self-hostable Cloudflare Worker that is the trusted backend for LeanZero Link's
passwordless auth and mesh onboarding. The desktop app talks ONLY to this worker for
identity: email OTP sign-in, a long-lived identity JWT, optional Resend contact sync, and
minting ephemeral Tailscale join keys for the app's embedded userspace `tailscaled`.

Everything degrades loudly, never silently: unconfigured capabilities answer `501`,
upstream failures answer `502` with the upstream status, and the audience sync reports its
own outcome in the response instead of pretending.

## Endpoints (JSON, versioned under `/v1`)

### `POST /v1/auth/request-code`

Request: `{ "email": "user@example.com" }`

Generates a 6-digit OTP, stores only its SHA-256 hash in Workers KV (10-minute expiry,
single-use, max 5 verify attempts), and emails the code via Resend.

| Status | Body |
| --- | --- |
| 200 | `{ "ok": true, "email": "<normalized email>", "expiresInSeconds": 600 }` |
| 400 | `{ "error": "invalid JSON body" \| "invalid email" }` |
| 429 | `{ "error": "rate limited", "scope": "email" \| "ip", "retryAfterSeconds": n }` + `Retry-After` header |
| 501 | `{ "error": "mail not configured on this deployment" }` |
| 502 | `{ "error": "failed to send code email", "status": <resend status> }` |

Rate limits (fixed hourly windows, KV counters): 3 requests per email per hour and
10 requests per IP (`CF-Connecting-IP`) per hour. Requesting a new code invalidates the
previous one for that email.

### `POST /v1/auth/verify`

Request: `{ "email": "user@example.com", "code": "123456" }`

Constant-time hash comparison, at most 5 attempts per code, single-use. On success the
worker mints an identity JWT (HS256 with `LINK_JWT_SECRET`; claims `sub` = email, `iat`,
`exp` = `iat` + 180 days, `ver` = 1) and upserts the email into the configured Resend
segment. An upsert failure never blocks auth — it is logged and surfaced as
`"audienceSync": "failed"`.

| Status | Body |
| --- | --- |
| 200 | `{ "token": "<jwt>", "email": "<normalized email>", "audienceSync": "synced" \| "skipped" \| "failed" }` |
| 400 | `{ "error": "invalid JSON body" \| "invalid email" \| "code must be a 6-digit string" }` |
| 401 | `{ "error": "invalid or expired code" }` (wrong code, expired, already used, or never issued) |
| 429 | `{ "error": "too many attempts; request a new code" }` |
| 500 | `{ "error": "LINK_JWT_SECRET not configured on this deployment" }` |

`audienceSync` values: `synced` (contact created, or already existed and was attached to
the segment), `skipped` (no `RESEND_AUDIENCE_ID` configured), `failed` (Resend refused or
was unreachable — details in the worker logs).

### `POST /v1/mesh/join-key`

Headers: `Authorization: Bearer <identity JWT>` (no body).

Verifies the JWT, then mints an **ephemeral, preauthorized, tagged, single-use**
Tailscale auth key scoped to the deployment's tailnet — exactly what the desktop's
embedded userspace `tailscaled` needs to join the mesh and vanish cleanly when it stops.

| Status | Body |
| --- | --- |
| 200 | `{ "authKey": "tskey-auth-…", "expirySeconds": n }` |
| 401 | `{ "error": "missing bearer token" }` or `{ "error": "invalid token", "reason": "malformed" \| "bad_signature" \| "expired" \| "bad_claims" }` |
| 501 | `{ "error": "mesh keys not configured on this deployment" }` — explicit, never a dummy key |
| 500 | `{ "error": "TS_KEY_EXPIRY_SECONDS is not a positive integer" \| "LINK_JWT_SECRET not configured on this deployment" }` |
| 502 | `{ "error": "tailscale key mint failed", "status": <upstream status; 0 = network error> }` |

The minted key always carries `reusable: false`, `ephemeral: true`, `preauthorized: true`
and `tags: [TS_NODE_TAG]`.

### `GET /v1/health`

Response: `{ "ok": <bool>, "version": "0.1.0", "capabilities": { "mail": <bool>, "audience": <bool>, "mesh": <bool> } }`

Each capability is derived from env presence — the desktop reads this to show what the
deployment supports. `ok` is `false` only when `LINK_JWT_SECRET` is missing (the worker
cannot mint or verify identity at all).

## CORS

`ALLOWED_ORIGINS` is a comma-separated origin list; default `*`. A listed origin is echoed
back with `Vary: Origin`; unlisted origins get no CORS headers. Preflight (`OPTIONS`)
answers 204 with `GET, POST, OPTIONS` and `Content-Type, Authorization`.

## Environment matrix

| Name | Kind | Required for | Default |
| --- | --- | --- | --- |
| `LINK_JWT_SECRET` | secret | everything (identity) | — (health `ok:false` without it) |
| `RESEND_API_KEY` | secret | `mail`, `audience` | — |
| `LEANZERO_MAIL_FROM` | var | `mail` (sender, e.g. `LeanZero Link <link@your-domain>`; the domain must be verified in Resend) | — |
| `RESEND_AUDIENCE_ID` | secret | `audience` (Resend **segment** id — see deviation note below) | — |
| `TS_API_TOKEN` | secret | `mesh` — either a Tailscale API access token, or an OAuth pair written as `client_id:client_secret` | — |
| `TS_TAILNET` | secret | `mesh` (tailnet name, e.g. `example.com`) | — |
| `TS_NODE_TAG` | var | — | `tag:leanzero-link` |
| `TS_KEY_EXPIRY_SECONDS` | var | — | `600` |
| `ALLOWED_ORIGINS` | var | — | `*` |
| `LINK_KV` | KV binding | OTP storage + rate limits | — (required) |

`TS_API_TOKEN` containing a `:` is treated as an OAuth client and exchanged at
`POST https://api.tailscale.com/api/v2/oauth/token` (client-credentials grant) on each
mint; otherwise it is sent as the username of HTTP Basic auth, per Tailscale's API docs.
The OAuth client needs the `auth_keys` scope, and the tag in `TS_NODE_TAG` must be one of
the tags selected on that OAuth client (Tailscale requires tags on all OAuth-minted keys).
The tag must also exist in your tailnet policy file (`tagOwners`).

## Deploy (owner runbook)

```sh
cd leanzero-link/worker
npm install

# 1. Authenticate wrangler against your Cloudflare account
npx wrangler login

# 2. Create the KV namespace and paste its id into wrangler.toml (kv_namespaces → id)
npx wrangler kv namespace create LINK_KV

# 3. Secrets (wrangler prompts for each value)
openssl rand -base64 48 | npx wrangler secret put LINK_JWT_SECRET
npx wrangler secret put RESEND_API_KEY      # from https://resend.com/api-keys
npx wrangler secret put RESEND_AUDIENCE_ID  # optional — segment id from the Resend dashboard
npx wrangler secret put TS_API_TOKEN        # optional — API key, or "client_id:client_secret"
npx wrangler secret put TS_TAILNET          # optional — e.g. "example.com"

# 4. Vars: set LEANZERO_MAIL_FROM (and optionally ALLOWED_ORIGINS, TS_NODE_TAG,
#    TS_KEY_EXPIRY_SECONDS) in wrangler.toml [vars]

# 5. Ship it
npx wrangler deploy

# 6. Smoke test
curl https://leanzero-link-auth.<your-subdomain>.workers.dev/v1/health
```

Rotating `LINK_JWT_SECRET` invalidates every issued identity token (users sign in again).

## Local development

```sh
npm install
npm run typecheck   # tsc --noEmit
npm test            # vitest — pure handlers, injected fetch/KV/clock, no network
npm run dev         # wrangler dev (local KV simulator)
```

The handlers are factored pure (`request → deps → response`, deps = KV + fetch + clock +
RNG + config), so the entire contract — OTP lifecycle, rate limits, JWT round-trip,
Resend/Tailscale request bodies — is unit-tested against the documented API shapes with
no keys and no network.

## External API contracts (doc-verified)

- Resend Send Email — <https://resend.com/docs/api-reference/emails/send-email>:
  `POST https://api.resend.com/emails`, `Authorization: Bearer`, body
  `{ from, to, subject, html, text }` → `{ id }`.
- Resend Create Contact — <https://resend.com/docs/api-reference/contacts/create-contact>:
  `POST https://api.resend.com/contacts`, body
  `{ email, unsubscribed, segments: [{ id }] }` → `{ object: "contact", id }`.
- Resend Add Contact to Segment — <https://resend.com/docs/api-reference/contacts/add-contact-to-segment>:
  `POST https://api.resend.com/contacts/{email}/segments/{segment_id}` → `{ id }`
  (used when the contact already exists, HTTP 409).
- Tailscale create auth key — <https://tailscale.com/api>:
  `POST https://api.tailscale.com/api/v2/tailnet/{tailnet}/keys`, body
  `{ capabilities: { devices: { create: { reusable, ephemeral, preauthorized, tags } } }, expirySeconds, description }`
  → `{ id, key, created, expires, capabilities }` (`key` is the one-time secret).
- Tailscale OAuth clients — <https://tailscale.com/kb/1215/oauth-clients>:
  `POST https://api.tailscale.com/api/v2/oauth/token` (form-encoded client credentials)
  → `{ access_token }`; all OAuth-minted keys must be tagged.

### Deviation note: Resend audiences → segments

The original spec named `POST /audiences/{audience_id}/contacts`. Resend has since
renamed Audiences to Segments and deprecated the audience-scoped contact endpoints
(<https://resend.com/docs/dashboard/segments/migrating-from-audiences-to-segments>).
This worker therefore calls the current, non-deprecated API: `POST /contacts` with a
`segments: [{ id }]` membership, falling back to the explicit add-to-segment call on a
409 (contact already exists). `RESEND_AUDIENCE_ID` keeps its name for spec compatibility
and holds the **segment id** — existing Resend audiences were migrated in place.

## Design notes and honest limitations

- Only the OTP's SHA-256 hash is stored, never the code; comparison is constant-time and
  JWT signature checks go through WebCrypto `verify` (also constant-time).
- KV is eventually consistent. The attempt counter and rate-limit counters are per-key
  read-modify-write, so a perfectly timed concurrent burst can exceed a limit by a small
  factor. For a low-traffic OTP mailer that is acceptable; the upgrade path if it ever
  matters is a Durable Object per email/IP.
- The logical OTP expiry (`expiresAtMs` in the record) is authoritative; the KV TTL is
  garbage collection.
- `POST /v1/mesh/join-key` performs the OAuth token exchange per request (tokens are not
  cached in KV) — one extra upstream call, zero secret-lifetime bookkeeping.

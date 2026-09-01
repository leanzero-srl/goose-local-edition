# LeanZero Link — self-hosted Node deployment

The same auth worker (`handleRequest` + all handlers/libs), served from a plain Node HTTP
server instead of Cloudflare. Nothing in the router or handlers changes — only the adapter:
the KV is backed by the filesystem (`src/lib/fs-kv.ts`) and the deps are built from
`process.env` (`src/node-server.ts`). Runs on the Mac; published to the internet with
Tailscale Funnel, no Cloudflare.

The Cloudflare Workers path (`src/index.ts`, `wrangler.toml`, `npm run dev/deploy`) is
untouched and still valid — this is a second front door onto identical logic.

## Run it

```bash
cd leanzero-link/worker
npm install
./run-node.sh
```

`run-node.sh` sources a KEY=VALUE env file (`set -a; . "$file"; set +a`) from
`${LEANZERO_LINK_ENV:-$HOME/.leanzero/link-worker.env}`, then `exec npx tsx src/node-server.ts`.
The script reads no secret values itself and the repo contains none — the env file supplies
everything. On boot it logs `node_server_listening` with the bound addr, KV dir, and the
capabilities `parseConfig` derived from the env.

The server binds `127.0.0.1:<PORT>` only. It never listens on a routable interface — Funnel
terminates TLS and forwards to loopback.

## Env file keys (`~/.leanzero/link-worker.env`)

| Key | Purpose | Missing behavior |
| --- | --- | --- |
| `LINK_JWT_SECRET` | HS256 signing secret for the identity JWT | `/v1/health` `ok:false`; `/v1/auth/verify` → 500 |
| `RESEND_API_KEY` | Resend API key for OTP email + segment upsert | mail/audience capability off; `request-code` → 501 |
| `LEANZERO_MAIL_FROM` | From address for the OTP email | mail capability off; `request-code` → 501 |
| `RESEND_AUDIENCE_ID` | Resend **Segment** id for contact upsert (optional) | audience capability off; verify still succeeds, `audienceSync:"skipped"` |
| `TS_API_TOKEN` | Tailscale API access token (from an OAuth client exchange) | mesh capability off; `mesh/join-key` → 501 |
| `TS_TAILNET` | Tailscale tailnet name, e.g. `example.ts.net` | mesh capability off; `mesh/join-key` → 501 |
| `TS_NODE_TAG` | ACL tag for minted keys (default `tag:leanzero-link`) | falls back to the default tag |
| `TS_KEY_EXPIRY_SECONDS` | Ephemeral join-key lifetime (default 600) | uses the default |
| `ALLOWED_ORIGINS` | Comma-separated CORS allow-list (default `*`) | allows all origins |
| `LINK_KV_DIR` | Filesystem KV directory (default `~/.leanzero/link-kv`) | uses the default; created `0700` if missing |
| `PORT` | Loopback port to bind (default `8791`) | uses `8791` |

No key/env is ever faked: an unconfigured capability answers `501`/`500` or reports its state,
never a dummy value.

## Filesystem KV

`createFsKvStore(dir)` stores one JSON file per key, `{ value, expiresAtMs? }`. The key is
base64url-encoded into the filename, so a key containing `../` cannot escape `dir`. Writes are
atomic (temp file + `rename`). `get` returns `null` for a key whose `expiresAtMs` has passed
(honoring Workers KV `expirationTtl` used for OTP expiry and rate-limit windows) and removes the
stale file; a corrupt/half-written file is treated as absent, logged (`fs_kv_corrupt`), and
cleaned up — never thrown.

## Publish with Tailscale Funnel

```bash
tailscale funnel "${PORT:-8791}"
```

Funnel exposes `https://<machine>.<tailnet>.ts.net/` and forwards to `127.0.0.1:<PORT>`. Point
`goosed` (and the future iOS companion) at that HTTPS URL. This is the personal/self-host
tailnet for publishing the worker — it is NOT the app's embedded, goose-owned mesh.

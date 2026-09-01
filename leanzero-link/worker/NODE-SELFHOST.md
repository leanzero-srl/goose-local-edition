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
everything. On boot it logs one `config_error` line per configuration problem `parseConfig`
found (`mesh_provider_partial_config` with the missing `HEADSCALE_*` names,
`jwt_secret_too_short`, `ts_key_expiry_invalid`), then `node_server_listening` with the
bound addr, KV dir, capabilities and `meshProvider`.

The server binds `127.0.0.1:<PORT>` only. It never listens on a routable interface — Funnel
terminates TLS and forwards to loopback.

## Env file keys (`~/.leanzero/link-worker.env`)

| Key | Purpose | Missing behavior |
| --- | --- | --- |
| `LINK_JWT_SECRET` | HS256 signing secret for the identity JWT — at least 32 bytes | `/v1/health` `ok:false`; `/v1/auth/verify` → 500 (also when present but under 32 bytes) |
| `RESEND_API_KEY` | Resend API key for OTP email + segment upsert | mail/audience capability off; `request-code` → 501 |
| `LEANZERO_MAIL_FROM` | From address for the OTP email | mail capability off; `request-code` → 501 |
| `RESEND_AUDIENCE_ID` | Resend **Segment** id for contact upsert (optional) | audience capability off; verify still succeeds, `audienceSync:"skipped"` |
| `HEADSCALE_API_URL` | Headscale API base (the preferred, per-account-isolated mesh), e.g. `http://127.0.0.1:8790` | see below |
| `HEADSCALE_API_KEY` | Headscale API key `hskey-api-…` | see below |
| `HEADSCALE_LOGIN_SERVER` | PUBLIC control URL nodes join (the Headscale Funnel root) | see below |
| `TS_API_TOKEN` | Tailscale API access token (from an OAuth client exchange) — used only when NO `HEADSCALE_*` is set | mesh capability off; `mesh/join-key` → 501 |
| `TS_TAILNET` | Tailscale tailnet name, e.g. `example.ts.net` | mesh capability off; `mesh/join-key` → 501 |
| `TS_NODE_TAG` | ACL tag for minted keys (default `tag:leanzero-link`) | falls back to the default tag |
| `TS_KEY_EXPIRY_SECONDS` | Ephemeral join-key lifetime (default 600) | uses the default |
| `ALLOWED_ORIGINS` | Comma-separated CORS allow-list (default `*`) | allows all origins |
| `LINK_KV_DIR` | Filesystem KV directory (default `~/.leanzero/link-kv`) | uses the default; created `0700` if missing |
| `PORT` | Loopback port to bind (default `8791`) | uses `8791` |

No key/env is ever faked: an unconfigured capability answers `501`/`500` or reports its state,
never a dummy value. The three `HEADSCALE_*` keys are all-or-nothing: all three → provider
`headscale`; none → `tailscale` if `TS_*` is set, else `none`; SOME → provider `none`,
`config_error{mesh_provider_partial_config, missing:[…]}` at boot, and `mesh/join-key` → 500
naming the missing keys. A partial Headscale config never falls back to Tailscale.

## Filesystem KV

`createFsKvStore(dir)` stores one JSON file per key, `{ key, value, expiresAtMs? }`, named
`sha256(key)` as 64 hex chars + `.json` — a fixed-length name from the hex alphabet, so a key
containing `../` cannot escape `dir` and a long key (a 254-char email, or anything) can never
exceed `NAME_MAX`. The plaintext key is inside the record (`jq .key`). Writes are atomic (temp
file + `rename`), and every operation on a key runs on that key's promise chain, so
`update(key, mutate)` — the atomic read-modify-write the attempt counter, rate windows and
`nodeSecret` mint are built on — cannot interleave with another operation on the same key.
`get` returns `null` for a key whose `expiresAtMs` has passed (honoring Workers KV
`expirationTtl` used for OTP expiry and rate-limit windows) and removes the stale file; a
corrupt/half-written file, or one whose embedded key is not the key looked up, is treated as
absent, logged (`fs_kv_corrupt`), and cleaned up — never thrown. `nodesecret:<email>` records
have no expiry.

Files written by earlier versions (base64url-named) are never read again and can be deleted.

## Publish with Tailscale Funnel

```bash
tailscale funnel "${PORT:-8791}"
```

Funnel exposes `https://<machine>.<tailnet>.ts.net/` and forwards to `127.0.0.1:<PORT>`. Point
`goosed` (and the future iOS companion) at that HTTPS URL. This is the personal/self-host
tailnet for publishing the worker — it is NOT the app's embedded, goose-owned mesh.

Funnel/Serve SETS `X-Forwarded-For` on every forwarded request to the connecting client's
address (tailscaled `ipn/ipnlocal/serve.go`, `Header.Set`, so a client-supplied value is
replaced; measured live: the header carried the caller's public egress address, not the
ingress). The request-code address limiter keys on it. The adapter deletes any inbound
`CF-Connecting-IP` (nothing on this path sets it). A request with no `X-Forwarded-For` —
e.g. `curl http://127.0.0.1:8791` from the machine itself — logs `client_ip_unresolved` and
is limited per email only.

Bodies over 1 MiB answer `413` (the response is written, then the rest of the body is
drained rather than the socket reset); an aborted or malformed request prelude answers `400`.

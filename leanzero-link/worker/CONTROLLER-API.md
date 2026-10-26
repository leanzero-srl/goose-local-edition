# Controller authority, protocol 1

Implemented source slice A1, 26 October 2026. This is the authority/registry contract; it does not establish a working mobile-to-owner transport, mesh enrollment binding or owner command execution. No production deployment accompanies this patch.

All routes below are on the configured **HTTPS auth-worker URL** under `/v1/controller`. Native requests use Bearer headers; Origin is refused and credentials never go in query strings. Responses use `Cache-Control: no-store`. The controller receives no account JWT or `nodeSecret`. Its signed access JWT is version 2 with a distinct audience/issuer; existing host account tokens remain version 1, and old `/v1/mesh/join-key` refuses controller tokens.

## Mobile login

`POST /auth/request-code`, body `{ "email": "owner@example.test", "enrollment_id": "<client-generated UUID>" }`, returns `{ "challenge_id": "<opaque string>", "expires_at": "<RFC3339>" }`. Keep enrollment ID across retries. A challenge handle is **not a UUID**. The account routing component is an email hash, not a secret or proof of account membership.

`POST /auth/verify`, body `{ "challenge_id": "<opaque string>", "enrollment_id": "<same UUID>", "code": "123456", "device_name": "My iPhone", "platform": "ios" }`, returns:

```json
{
  "access_token": "<opaque credential>",
  "token_type": "Bearer",
  "expires_at": "2026-10-26T17:00:00.000Z",
  "refresh_token": "<opaque credential>",
  "device": {
    "device_id": "<UUID>",
    "name": "My iPhone",
    "platform": "ios",
    "role": "controller",
    "revision": 1,
    "scopes": ["catalog.read", "history.read", "work.send", "work.steer", "work.stop", "input.answer", "resources.read", "models.read", "models.download", "models.manage", "providers.read", "providers.manage", "devices.read", "devices.manage"],
    "created_at": "2026-10-26T16:45:00.000Z",
    "revoked_at": null
  }
}
```

Platforms are `ios`, `ipados`, `desktop`. Role/scopes are server policy; a supplied role is refused. Concurrent exact verify retries return the same device and credentials; they do not enroll additional devices. Device OTP attempt/consume and account email/verify counters are authoritative registry transactions; legacy KV rate limits provide an additional outer limit.

`POST /auth/refresh`, body `{ "device_id": "<UUID>", "refresh_token": "<current token>", "request_id": "<UUID>" }`, returns the same `DeviceCredentials` shape. Persist the **old refresh token and request ID together before sending**. Retry that same pair after a lost response. A used refresh token presented with a different request ID revokes the device's refresh family and access tokens. Persist replacement credentials atomically on success. Access lifetime is 15 minutes; the refresh family expires 180 days after enrollment. Rotation increments the device revision, invalidating prior access tokens. Receipts are AEAD-encrypted at rest; refresh credentials are otherwise stored only as hashes.

`GET /devices` returns `{revision,devices:[DeviceRecord]}`. `POST /devices/{device_id}/revoke`, authenticated by an active device with `devices.manage`, takes `{request_id,expected_revision}` and returns `{device_id,revision,revoked_at}`. Revoking one device does not revoke another. Already-running owner work is not cancelled by revocation.

## Host registration and introspection

`POST /hosts/register` uses the **existing host account JWT**. Body is `{owner_node_id,registration_id,host_name,protocol_version:1,tls_spki_sha256}`. The IDs are UUIDs; `tls_spki_sha256` is lowercase 64-hex SHA-256 over the host certificate's DER SubjectPublicKeyInfo, or null before TLS is configured. Response is `{host_credential,host_revision}`. Store the opaque host credential privately. No host JWT is verified on the Rust client.

Initial retries with the same account/registration ID/name/pin can recover the encrypted initial receipt before the host has ever acquired a lease. Existing activated owners additionally require `X-Link-Host-Credential` and the same registration ID. Different registration cannot take over that owner. Registration can update display name/pin with the existing credential; the host revision increments. A registered pin is a trust binding, not proof that a production TLS listener exists: the native client must verify the actual TLS key and HTTPS endpoint, without trust-all delegates or global ATS exceptions.

`POST /hosts/lease` uses Bearer host credential and body `{owner_node_id,runtime_id,store_generation,catalog_revision,mesh_node_key}`. IDs are UUIDs, catalog revision a nonnegative safe integer, mesh key a provider node-identity string. Response is `{lease_id,lease_expires_at,host_revision}`. The host runtime ID is freshly generated in memory on each start. The lease lasts 60 seconds and must be renewed. Another live runtime ID is refused; an expired runtime can be replaced. Lower catalog revisions are refused against the authoritative high-water; generation changes require explicit owner recovery and are not silently accepted. Identical copied offline state is intrinsically undetectable until independent evidence differs; this registry does not claim otherwise.

`POST /introspect` uses Bearer host credential and body:

```json
{
  "token": "<opaque controller access token>",
  "owner_node_id": "<registered owner UUID>",
  "runtime_id": "<current runtime UUID>",
  "host_lease_id": "<current lease UUID>",
  "required_scope": "history.read"
}
```

Invalid/cross-account/revoked controller authorization returns `{active:false}`. Active response:

```json
{
  "active": true,
  "account_id": "<account UUID>",
  "device_id": "<device UUID>",
  "device_revision": 1,
  "role": "controller",
  "owner_node_id": "<owner UUID>",
  "scopes": ["history.read"],
  "token_expires_at": "2026-10-26T17:00:00.000Z",
  "lease_seconds": 30,
  "host_revision": 1,
  "store_generation": "<store UUID>",
  "host_lease_id": "<lease UUID>"
}
```

The real scope array contains the current device grants. The duration is capped by both token expiry and host lease expiry. Host/account/owner/runtime/lease are all checked. A missing TLS pin refuses with `host_trust_unconfigured`. The receiver anchors its monotonic freshness deadline before making the introspection request, sends no frame after it, and revalidates before mutation acceptance/dispatch. A1 supplies the authority decision; stream termination/queued-command behavior needs A2 owner implementation and proof.

`GET /hosts` uses a device token with `catalog.read`. At A1 it returns registration metadata with `availability:"update_required"`, `registration_state:"awaiting_mesh_binding"`, empty endpoints/capabilities (or `offline` once the lease expires). It intentionally does not pretend that a submitted mesh key is provider-verified. Provider-confirmed endpoint/role binding and controller mesh enrollment remain the next source slice. The agreed endpoint shape for that slice is `{url:"https://<verified mesh IP>:<configured port>",tls_spki_sha256:"<hex>"}`; no caller-supplied IP is authoritative.

## Storage and operational gates

Node's actual startup wires `SqliteDeviceRegistry` at `<LINK_KV_DIR>/controller-registry.sqlite`, schema version 1, WAL and `BEGIN IMMEDIATE`, private DB file. It uses the measured `node:sqlite` **DatabaseSync** export (Node 26.5.0 here; supported Node 24 also has this export). The newer rolling Node documentation names a different alias; do not code against an unmeasured runtime. Read/schema corruption is fatal/unavailable, never empty success.

Cloudflare's actual entrypoint exports `DeviceRegistryDurableObject` and injects `DurableDeviceRegistry` using `LINK_DEVICE_REGISTRY`; wrangler migration `controller-registry-v1` creates its SQLite class. Every read/compare-and-swap uses the same per-account object. Missing binding yields typed 503; the adapter itself is implemented and tested with workerd, not a mock KV substitute. Node/Cloudflare registry data is not automatically replicated; moving deployments requires an explicit verified migration, not starting a blank authority over existing devices.

The conformance suite runs both adapters, concurrent OTP/refresh/CAS, wrong account/owner/runtime/scope, legacy replay, individual revocation, expiry, rollback, corruption, and actual worker-router persistence across a local workerd restart. Miniflare 5's shipped constructor requires `convertV4MiniflareOptions`; its README examples still show the old direct options. Test dependencies are pinned to the versions already resolved by Wrangler. These local results do not replace production rollout, native TLS/mesh proof, independent security review, or owner-side stream/run tests.

Errors are `{error:{code,message,retryable}}`; 400 malformed request, 401 invalid credential/code, 403 origin/scope/owner/lease mismatch, 404 missing object/route, 409 stale/reused identity/revision, 429 rate limit, 503 missing/unavailable authority. Provider delivery failure is 502. Logs omit credential values, request payloads and underlying driver errors.

# Controller authority, protocol 1

Implemented source slices A1 and A3, 26 October 2026. This documents registry authority and provider-verified enrollment. Installed mobile transport and owner execution remain separate integration proofs. Independent security review is pending; no production deployment accompanies this patch.

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

### Desktop controller identity (26 October 2026)

`POST /hosts/controller` uses the existing account JWT as Bearer **and** the activated host's `X-Link-Host-Credential`. Body is exactly `{enrollment_id:UUID,device_name:string}`. The authority verifies both credentials belong to the same account, validates the host credential and current runtime lease, and returns the `DeviceCredentials` shape above with platform `desktop`. An enrollment ID is persisted before sending; concurrent/repeated identical requests recover the same encrypted receipt. A changed owner/name is a conflict, and a consumed enrollment cannot recreate a revoked or refreshed device. The desktop's persistent disabled state prevents background reenrollment after revocation or logout.

Desktop credentials, including those enrolled by `/auth/verify` with platform `desktop`, grant exactly `catalog.read`, `history.read`, `work.send`, `work.steer`, `work.stop`, and `input.answer`. They cannot manage providers/models/devices. No mesh node is enrolled by this exchange; the desktop reuses the compute host's owned userspace mesh for transport. Account JWT, host credential and `nodeSecret` never enter a renderer response or an owner request.

`POST /auth/logout`, Bearer controller access token, body `{request_id:UUID}`, returns `{revoked:true}` and revokes only that authenticated device. Identical retries recover the revocation result. No `devices.manage` scope is needed to revoke oneself. Existing other controllers and the compute host remain authorized; accepted owner work continues.

The Rust desktop facade exposes `_goose/unstable/controller/{status,requestCode,verify,logout,hosts,catalog,sync,openWork,history,workChanges,command,receipt,pendingCommands}` on authenticated local ACP. Work methods accept immutable `WorkRef` and store generation, never an endpoint, token or requested scope. `workChanges` calls the finite signed-cursor owner replay endpoint; no work history is fetched until selected. One metadata pump/cache lives in the backend process across windows. `pendingCommands{work}` returns `{commands:[{command,receipt,device_id}]}` from a private durable outbox; it never resubmits automatically. Retrying `command{command}` preserves the original command UUID and exact typed contents.

`GET /desktop/lifetime` on the local backend requires its secret in `X-Secret-Key` or Bearer and refuses Origin, even when the other local routes were configured without required authentication. It returns `{link_host_enabled:boolean,active_run_ids:string[],detached_runs:true}` from persisted Link intent and the shared owner-run registry. It does not depend on a renderer connection. Recipe preparation tasks that have not registered an owner run need their separate lifecycle integration.

`POST /hosts/register` uses the **existing host account JWT**. Body is `{owner_node_id,registration_id,host_name,protocol_version:1,tls_spki_sha256,controller_port}`. The IDs are UUIDs; `tls_spki_sha256` is lowercase 64-hex SHA-256 over the host certificate's DER SubjectPublicKeyInfo, or null before TLS is configured. Response is `{host_credential,host_revision}`. Store the opaque host credential privately. No host JWT is verified on the Rust client.

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

Provider-confirmed host discovery, mesh enrollment and peer admission are specified in the A3 section below.

## Storage and operational gates

Node's actual startup wires `SqliteDeviceRegistry` at `<LINK_KV_DIR>/controller-registry.sqlite`, schema version 1, WAL and `BEGIN IMMEDIATE`, private DB file. It uses the measured `node:sqlite` **DatabaseSync** export (Node 26.5.0 here; supported Node 24 also has this export). The newer rolling Node documentation names a different alias; do not code against an unmeasured runtime. Read/schema corruption is fatal/unavailable, never empty success.

Cloudflare's actual entrypoint exports `DeviceRegistryDurableObject` and injects `DurableDeviceRegistry` using `LINK_DEVICE_REGISTRY`; wrangler migration `controller-registry-v1` creates its SQLite class. Every read/compare-and-swap uses the same per-account object. Missing binding yields typed 503; the adapter itself is implemented and tested with workerd, not a mock KV substitute. Node/Cloudflare registry data is not automatically replicated; moving deployments requires an explicit verified migration, not starting a blank authority over existing devices.

The conformance suite runs both adapters, concurrent OTP/refresh/CAS, wrong account/owner/runtime/scope, legacy replay, individual revocation, expiry, rollback, corruption, and actual worker-router persistence across a local workerd restart. Miniflare 5's shipped constructor requires `convertV4MiniflareOptions`; its README examples still show the old direct options. Test dependencies are pinned to the versions already resolved by Wrangler. These local results do not replace production rollout, native TLS/mesh proof, independent security review, or owner-side stream/run tests.

Errors are `{error:{code,message,retryable}}`; 400 malformed request, 401 invalid credential/code, 403 origin/scope/owner/lease mismatch, 404 missing object/route, 409 stale/reused identity/revision, 429 rate limit, 503 missing/unavailable authority. Provider delivery failure is 502. Logs omit credential values, request payloads and underlying driver errors.

## Provider-verified mesh enrollment (A3)

Supported authority is Headscale v0.29.3, using its official [node](https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/node.proto), [pre-auth key](https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/preauthkey.proto) and [REST route](https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/headscale.proto) definitions. Hostnames and submitted addresses supply no identity proof. A binding records provider node ID, exact `nodekey:<64 lowercase hex>`, account user ID and the exact single-use key ID issued for that enrollment. The enrollment binding cannot switch keys; a provider rekey remains incompatible until explicit reenrollment. Node keys themselves can rotate; they are not physical-device identifiers.

The controller provider adapter checks the existing account-isolation policy; it never rewrites provider policy. The account must already have its provider user, established by host onboarding. Tagged nodes are not eligible for this user-isolated path. The provider response's untagged user, exact issued key ID and current node key must agree. Empty or incomplete provider DTOs are errors. v0.29.3 lists are unpaginated; a response advertising pagination is rejected with `provider_pagination_unsupported`, preventing a partial list from authorizing enrollment. Only assigned 100.64.0.0/10 and fd7a:115c:a1e0::/48 addresses can form endpoints.

`POST /mesh/join-key` uses a current controller token, body `{enrollment_id:UUID}`; `POST /hosts/mesh/join-key` uses a host credential with the same body. Role comes from the route and credential; a supplied role is rejected. The host must already have a valid registered port and SPKI pin. Both return:

```json
{"enrollment_id":"<UUID>","auth_key":"<single-use credential>","login_server":"https://<headscale>","expiry_seconds":600}
```

The duration follows configured TS_KEY_EXPIRY_SECONDS; 600 is the default. No account JWT or nodeSecret is returned. Persist the enrollment ID before requesting. The account CAS reserves the external mint once; exact retries recover the encrypted response. A concurrent or interrupted mint returns `enrollment_pending` rather than creating another key. A confirmed identity can explicitly request a new enrollment ID after its provider node disappeared or expired; a still-live provider node cannot be silently replaced. A pending enrollment with an unknown provider outcome must expire before requesting a new ID.

Join using the app-owned mesh instance. Existing `tailscale up --reset` does not by itself prove that a new key was consumed. Real reenrollment still needs installed-client proof; no daemon was started by these tests.

`POST /mesh/confirm` and `POST /hosts/mesh/confirm` take `{enrollment_id,mesh_node_key}` with their respective credential. The former returns DeviceRecord; the latter returns `{owner_node_id,mesh_node_key,host_revision}`. Confirmation is repeatable and refuses forged keys, another account's node, another subject's enrollment, expired enrollment, tagged/expired nodes, or changing a confirmed identity. Unconfirmed nodes never gain an admitted role.

Before issuing any controller key, all unexpired account provider nodes must already have a confirmed enrollment. Every host must have a registered pin/port, no suspended or pending registration, and a durable `activated_binding:{node_id:string,node_key:string,user_id:string,key_id:string}` receipt matching its current provider binding. The receipt is committed only by a provider-attested host lease transaction, within the registry CAS that rechecks the enrollment ID, host role, subject and all four binding fields. If the provider or lease expires during CAS, the expired response is refused; a committed receipt records issuance, not delivery of a usable lease. A previously activated host may be offline with an expired or withdrawn lease without blocking controller enrollment. Old rows without the receipt, confirmed-but-never-activated hosts and fresh enrollments require a successful lease; a generation or old current lease never substitutes. The receipt changes no discovery, peer-admission, introspection or command authority: those still require current leases. It does not attest that an offline binary has not subsequently been downgraded. Unknown nodes, legacy hosts and outstanding untracked provider join keys produce `409 update_required`. Legacy key mint requests and controller enrollment reservations serialize in the same account registry: in-flight legacy mints and their credential lifetime block controller issuance; issued/pending controller enrollments with unexpired keys, and confirmed controller enrollments that still have a provider foothold (a node carrying their binding or key, online or not, or a key that can still admit a node), block new legacy join keys; so does ANY controller enrollment, whatever its status, whose node is still listed by the provider (WR-7 r2). A confirmed controller whose node has left the provider and whose key can no longer admit is dormant and does not block (WR-7, 31 October 2026): it can only return through a fresh controller enrollment, which the checks above refuse while legacy nodes or legacy credential lifetimes exist, so legacy nodes and controller nodes are still never on the mesh together. Any provider read failure keeps the strict rule. This worker gate cannot upgrade or suppress an old binary itself. Rollout still requires fleet upgrade and direct mixed-version verification before admitting phones.

Hosted Tailscale returns `501 provider_binding_unsupported` for these new mesh/discovery routes. The configured shared tagged key path has no implemented per-account enrollment attestation; it is not treated as equivalent to Headscale. No provider returns `501 mesh_unconfigured`. Legacy account authentication/join-key stays available to accounts without controller enrollment and still rejects controller tokens.

### Host endpoints and authoritative peer admission

Host registration accepts `controller_port:1..65535|null`; missing port is an explicit old/incomplete registration. Host lease and introspection now require a current provider-confirmed host binding. The lease's submitted key must equal the attested key. Host leases and authorization leases are also capped by provider node expiry.

`GET /hosts` uses a controller token with catalog.read. It re-reads the provider and registry, returning `{revision,hosts:[HostDescriptor]}`. Verified, leased, online hosts expose only `{url:"https://<provider IP>:<registered port>",tls_spki_sha256:"<registered hex>"}` endpoints; IPv6 literals use brackets. Provider IP moves update the next response. Incompatible records carry update_required, empty endpoints/capabilities and registration_state awaiting_mesh_binding; activated hosts with unreachable endpoints, expired leases or gracefully withdrawn leases are offline with registration_state verified. The activation receipt determines compatibility only; online endpoints and capabilities still require a current lease for the same node key. Provider outage is an error, never stale successful endpoints. The reported online state reflects the provider and lease; it is not an end-to-end TLS reachability probe.

`GET /hosts/peers` uses the host credential and returns:

```json
{"protocol_version":1,"revision":42,"lease_seconds":30,"peers":[
  {"mesh_node_key":"nodekey:<64 hex>","role":"host","owner_node_id":"<UUID>"},
  {"mesh_node_key":"nodekey:<64 hex>","role":"controller","owner_node_id":null}
]}
```

The receiving host must have its own current verified binding and lease. Only current provider-attested, confirmed registry enrollments appear. Missing/unknown/controller peers must never be polled or counted as compute hosts. The registry revision fences registry changes; provider address/liveness changes can occur without incrementing that revision, so refresh each snapshot even if its revision is unchanged. Anchor freshness before the HTTP request. lease_seconds is at most 30 and capped by the receiver/peer host leases, provider expiries and device refresh-family expiries; zero remaining freshness returns an error. Outage or deadline expiry must clear admission.

### Revocation cleanup and generation recovery

Device revoke retains its existing response and adds `mesh_cleanup:"complete"|"pending"`. Registry authorization is removed before calling the provider. Cleanup expires the issued single-use keys, enumerates nodes by the exact account/key binding (including joined-but-unconfirmed nodes), then deletes those nodes. A second controller keeps its own enrollment. Provider failure leaves an explicit pending receipt. `POST /devices/{device_id}/mesh/cleanup`, authenticated with devices.manage, body `{}`, retries cleanup and returns `{device_id,mesh_cleanup}`. Refresh replay also invokes cleanup. Refresh cannot restore a revoked device. Mesh cleanup does not cancel already-running owner work.

Only a cryptographically recognized refresh replay that commits revocation schedules refresh-triggered cleanup. Public account/device IDs, invalid secrets and malformed requests do not authorize provider calls. Completed cleanup remains complete on repeated refresh failures and explicit retry; a mint finishing after revocation independently marks its newly learned key pending.

`POST /hosts/recover-generation` requires Bearer host credential and body:
`{token:<controller token with devices.manage>,owner_node_id,request_id,expected_generation,expected_host_revision,expected_catalog_revision,new_generation}`.
The expected catalog revision is the authoritative high-water. Both credentials must belong to the same account; every expected value must match. Success invalidates the host lease, rotates generation, resets that generation's high-water to zero and increments host revision, returning `{host_revision,store_generation}`. Exact retries recover the same result. Changed request IDs/contents, stale high-water, old-generation leases and old lease IDs are refused. The client must perform this explicit recovery flow; it cannot silently reset registration on 409.

### A3 verification and remaining rollout proof

Local typecheck, the full worker suite (178 tests), and Wrangler dry-run are the release-source checks. The new 18 tests execute both actual registry adapters against a loopback HTTP mock implementing the official Headscale REST shapes, including provider expiry, cross-account/role/forged-key negatives, IP moves, incomplete pagination, policy failure, mixed legacy nodes, outstanding/in-flight credentials, simultaneous retries, revoke during mint, retryable cleanup, sibling controllers, and explicit generation recovery. These are wire-level provider mocks, not a real Headscale daemon or installed phone.

Independent security review is pending. No production provider query, credential read, daemon start, policy mutation or deployment was performed. Real Headscale enrollment/rekey behavior, mobile VPN enrollment, mixed-version fleet visibility, endpoint TLS reachability and manager admission wiring remain rollout gates.
# Host lifecycle integration (2026-10-26)

Host-only requests use the registered `h1` credential, Bearer header, no `Origin`, and `Cache-Control: no-store`. `GET /v1/controller/hosts/bootstrap` returns exactly `{account_id:UUID,owner_node_id:UUID}`. It returns no JWT or node secret. The host retains the existing `/v1/auth/verify` response's `nodeSecret` in a private account-bound local file. Controllers never receive it.

`POST /v1/controller/hosts/recover-node-secret` accepts exactly `{}` with the current v1 account JWT as Bearer and the matching `h1` credential in `X-Link-Host-Credential`. Response is exactly `{account_id:UUID,owner_node_id:UUID,email:string,node_secret:string}`, where `node_secret` is the existing lowercase 64-hex account secret. This route neither creates nor rotates secrets and makes no provider calls. Missing/corrupt stored secrets return `409 node_secret_missing` / `409 node_secret_corrupt`; account deletion/revocation, suspension, expired JWTs and v2 controller tokens refuse recovery. The registry CAS rechecks concurrent authority changes, and JWT expiry is checked after the KV read and CAS completion.

A saved-login host with an absent local secret first registers and bootstraps using its own account JWT and preserved owner/store/TLS state. It validates recovered account, owner and email, then saves the secret privately under the current connect generation and intent lock before starting the mesh. A concurrent logout cannot restore the secret; existing corrupt, insecure or foreign local secret files fail without recovery. Reconnect uses the saved secret and existing verified host enrollment. This migration needs no fresh OTP when both credentials remain authorized; expired/revoked account login still requires sign-in. No live provider migration proof is implied by the fake-mesh tests.

`GET /v1/controller/hosts/mesh/context` returns `{login_server:string,enrollment:null|{enrollment_id:UUID,status:"pending"|"issued"|"confirmed",mesh_node_key:string|null}}`. The provider's complete current node listing determines whether a confirmed enrollment still exists. Provider outage is an error; a provider identity change is `409 mesh_binding_changed`. A lost confirmation ACK retries the same enrollment against the actual node key before consuming a new key.

Host registration additionally accepts `runtime_id:UUID`; a different runtime cannot change an actively leased owner's endpoint. Host lease requests accept `capabilities:ControllerScope[]` (omission means none). Discovery reports only this current host list. Local host policy still intersects those scopes before requests and queued dispatch.

`POST /v1/controller/hosts/withdraw {runtime_id:UUID}` returns `{withdrawn:true}` and clears the current lease for a graceful disconnect. It preserves the host credential and enrollment for reconnect. `POST /v1/controller/hosts/release {runtime_id:UUID}` returns `{released:true}` and invalidates the host credential, lease, and host enrollment admission for logout. Both reject a conflicting live runtime. Release is idempotent with the released credential. Same-account registration may reactivate the same immutable owner/registration IDs; generation and catalog high-water remain fenced. A lost reactivation response is retryable before the new runtime acquires its first lease.

Cloudflare provider requests use `redirect:"manual"` and reject every non-2xx response without following it. The real workerd fixture verifies redirects produce zero destination requests; `redirect:"error"` is unsupported by Workers.

## Account deletion (2026-10-26)

Apple requires in-app deletion initiation when an app creates accounts. Mobile email enrollment creates a controller account in `ControllerService.prepareChallenge`; logout/device revoke therefore do not satisfy deletion. Current primary reference: https://developer.apple.com/help/app-review/guideline-reference/5-1-1-account-deletion . The extra email code is purpose-bound account confirmation, which Apple permits.

All three routes reject Origin, use `Cache-Control: no-store`, and accept bearer headers only. Never put the deletion token in a URL or diagnostics.

| POST route | Authorization and JSON | Success JSON |
| --- | --- | --- |
| `/v1/controller/account/deletion/request-code` | Current controller bearer with `devices.manage`; `{ "request_id": "<UUID>" }` | `{ "request_id": "<UUID>", "deletion_token": "d1.<account-key>.<request-id>.<secret>", "expires_at": "<RFC3339>" }` |
| `/v1/controller/account/deletion/confirm` | Deletion token bearer; `{ "code": "123456" }` | `DeletionStatus` below |
| `/v1/controller/account/deletion/status` | Deletion token bearer; `{}` | `DeletionStatus`; also retries unfinished cleanup |

`DeletionStatus = { request_id: string, status: "pending" | "deleted", pending: string[], retained: string[], completed_at: string | null }`. Pending and deleted both return HTTP 200. The code expires after ten minutes and has five attempts. The purpose token has no expiry after confirmation: the client saves it in Keychain before sending confirmation and retains it until deletion is confirmed. It cannot authenticate any ordinary controller, host, refresh or mesh route. Request-code retries with the same request ID recover the same token without another email.

An invalid/expired code returns 401 `invalid_code` and leaves the ordinary session intact. Status before accepted confirmation returns 409 `deletion_not_confirmed`; return to code entry without clearing the session. A lost confirmation response is unknown: preserve both credentials, call status, and clear ordinary credentials/cache only after pending or deleted. A confirmed deletion immediately revokes all controllers and hosts. Mac files, local sessions and model weights are outside this account service and are untouched.

Cleanup deletes account-linked Headscale keys, nodes and user, account node secret/OTP/email rate keys, and Link-source Sanity activity records. Headscale snapshots must be complete and account identities must agree; keys expire before nodes/user are removed. The shared Resend contact loses only its Link segment. The worker never deletes a whole Resend contact, because another service can add a newsletter subscription between a membership read and deletion. IDs fence deletion from newly created remote identities. Cleanup and old local-auth writes have renewable transport leases; an expired writer cannot renew or perform a KV mutation. Provider requests have a shorter transport timeout than their lease. These bounds do not apply to model work.

Pending values include `mesh`, `contact`, `activity`, `account_records`, `in_flight_cleanup`, `cleanup_interrupted`, `in_flight_requests`, `legacy_mesh_outcome_unknown`, and `legacy_write_outcome_unknown`. Provider failures retry safely. Ordinary abandoned cleanup/local-auth leases can be reclaimed after their lease expires. An external CREATE whose response is lost is different: a timeout cannot prove it will never commit. The existing legacy Headscale mint reservation, and unknown external Resend/Sanity create outcomes, remain explicitly pending until an operator reconciles the provider operation. Never zero a reservation while its original writer might resume. No automatic background retry/schedule has been added; status performs the next attempt.

After deletion completes, fresh email enrollment creates a new account UUID and preserves the legacy JWT issuance cutoff plus the previous deletion receipt. Old controller access/refresh/enrollment receipts remain invalid. Legacy JWTs issued at or before the final cutoff cannot register hosts, create desktop controllers or mint mesh keys. A same-second new legacy verify returns 409 `account_signin_retry` before consuming its OTP; retry after one second. Completion advances the cutoff after admitted requests finish, including any token they minted after the initial confirmation.

`retained` is explicit and does not invent a retention duration:

- `security_revocation_record`: hashed account lookup, token-revocation cutoff, and a minimal deletion receipt remain to reject old credentials and answer recovery requests.
- `shared_contact_record`: the global Resend contact remains; this worker removed Link's segment only. Independent subscriptions are never silently deleted.
- `historical_service_records`: deletion does not claim to purge historical application logs, sent-email copies, provider logs or Sanity transaction history. The repository's `ops/rotate-link-log.sh` defaults archive cleanup to 30 days; this source fact is not proof that a deployment runs that job or uses that override. No retention period for the other systems has been verified here.

The internal [account deletion incident runbook](ACCOUNT-DELETION-RUNBOOK.md) covers pending states, investigation, and the evidence required before any incident-specific repair. It provides no force-complete shortcut. Release limitation: the owner must settle and publish actual retention/data-ownership policy for the shared contact and historical records. Ambiguous external creates remain visible pending incidents; automatic reconciliation is not implemented. These are not a claim of legal necessity or App Store approval. No live account was deleted and no production route was deployed by this change. The iOS disclosure must match these retained fields; do not describe logout, local cache removal, or pending cleanup as completed deletion of all personal data.

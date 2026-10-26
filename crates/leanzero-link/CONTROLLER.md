# Controller transport, 2026-10-26

The additive Rust protocol is in `src/controller_wire.rs`. This source slice implements
the owner router, opaque device introspection client, monotonic authorization leases,
immutable owner identity, pinned TLS listener, mesh public-key preservation, and an
authoritative role filter. `GooseAcpAgent::build_link_manager` now installs host lifecycle
and the actual shared SessionManager/OwnerRunRegistry adapter. A compiled fixture is not
proof that an installed Mac or phone can control work.

## Owner adapter contract

`controller::router(ControllerContext { receiver, authority, backend })` returns an Axum
router. `receiver: ReceiverBinding` holds `owner_node_id`, `runtime_id`,
`store_generation`, `host_lease_id` (UUIDs) and `host_revision:u64`. `authority` is
`Arc<dyn ControllerAuthority>`; production uses `WorkerAuthority::new(worker,
host_credential)`. `backend: Arc<dyn ControllerBackend>` supplies durable catalog,
history, selected-work streams, command acceptance/receipts and resource samples.

The adapter must authenticate/version opaque cursors, bind every object to the owner and
generation, enforce account/device receipt visibility, filter transcript bytes to the
existing user-visible message contract, and commit acceptance before returning HTTP 202.
Queued commands retain `DispatchAuthorization`; under the owner's run-control lock call
`dispatch` immediately before side effects. This checks the worker again and refuses a
changed or revoked principal. The adapter must also recheck local switches and the exact
work revision/generation/run under that lock. Nothing delegates arbitrary ACP, jobs or
inference methods from this router.

Routes use `Authorization: Bearer <opaque device token>` only. Every Origin header is
403 before authentication. The separate legacy `/v1/swarm` router retains its own node
token; the controller router exposes no legacy paths.

| Method | Owner path | Scope | Return |
| --- | --- | --- | --- |
| GET | `/v1/controller/capabilities` | `catalog.read` | `ControllerCapabilities` |
| GET | `/v1/controller/catalog?page_token=...` | `catalog.read` | `CatalogPage` |
| GET | `/v1/controller/catalog/changes?cursor=...` | `catalog.read` | `CatalogChanges` |
| GET/WS | `/v1/controller/catalog/stream?cursor=...` | `catalog.read` | `CatalogChange` frames |
| GET | `/v1/controller/work/{kind}/{uuid}?store_generation=...` | `history.read` | `WorkSnapshot` |
| GET | `/v1/controller/work/{kind}/{uuid}/history?store_generation=...&page_token=...` | `history.read` | `HistoryPage` |
| GET/WS | `/v1/controller/work/{kind}/{uuid}/stream?cursor=...` | `history.read` | `WorkFrame` |
| POST | `/v1/controller/work/{kind}/{uuid}/commands` | action-specific | HTTP 202 `CommandReceipt` |
| GET | `/v1/controller/commands/{uuid}` | `catalog.read` | device-visible `CommandReceipt` |
| GET | `/v1/controller/resources` | `resources.read` | `ResourceSnapshot` |

Exact shapes are the Rust serde declarations. `WorkSnapshot={entry,pending_inputs,
history_watermark,work_cursor,live_messages}`. Signed `resume_cursor` values accompany catalog pages/changes/frames; history pages return the fixed work cursor. Journal sequence values increase but need not be contiguous. `HistoryItem={message_id,revision,message}` where message retains
the serialized visible Goose message. `WorkCommand` flattens the typed enum into
`action` and `payload`; resource content contains an uploaded `resource_id`, never a
phone path. `ResourceSnapshot={load:NodeLoad,sampled_at:RFC3339}`. Failures, including
malformed path/query extractor rejections, use `{error:{code,message,retryable}}`.
Responses carry `Cache-Control: no-store`.

## Authorization freshness

The monotonic lease starts before introspection begins. Its duration is the lesser of
the worker's seconds, remaining token lifetime and the 30-second transport policy. A
slow introspection consumes its own lease. Every frame checks freshness and host scope;
backpressured sends cannot outlive the authorization deadline. Refresh starts at half
the granted interval and blocks frame delivery while it runs. Failure closes the
connection; no last-known allowance is reused. WS 4401 means expired credentials, 4403
denied/revoked/wrong binding, 4413 authority or freshness unavailable. Running work is
not cancelled by a viewer disconnect or revoke.

The host renews its separate runtime lease and catalog high-water, and replaces the
authorized router with the current binding. Authority loss closes access and expires
new peer admission while accepted owner work stays alive. Access tokens are never locally decoded or verified.
Worker requests require HTTPS and refuse redirects to avoid replaying token-containing
JSON at a redirect destination.

## TLS and identity

`OwnerIdentity::load_or_create` produces private, atomically published random owner and
registration UUIDs. The catalog database owns its separate store generation. Account
logout must clear credentials without changing either durable identity.

`ControllerTlsIdentity::load_or_create` persists a private key for that owner. The leaf
certificate SAN is `owner-<owner UUID>.link`; the authenticated worker binds the
lowercase 64-character SHA256 of SubjectPublicKeyInfo DER to the same owner. Mobile
discovery uses `MeshEndpoint={url,tls_spki_sha256}`. The URL's mesh IP is a locator; the
client validates the expected owner SAN, leaf validity/EKU and the worker-authorized
SPKI. There is no trust-all or ATS exception. `start_controller_tls` serves the injected
router on the configured address (default controller port 41227; tests use 0).

`controller_host::ControllerHostFactory` supplies the actual store directory, catalog
watermark and backend. Production uses a loopback TLS listener reached through the owned
userspace mesh; local write scopes also require the current remote-execution switch.
The account-bound node secret comes only from the existing verified host sign-in and is
persisted privately beside identity.json. Missing or corrupt secret refuses new connect
before daemon startup. Host bootstrap returns account/owner UUIDs, never that secret.

Provider-confirmed enrollment resumes its exact existing node identity. New enrollment
persists its request UUID before minting, consumes the scoped key through explicit
`--force-reauth`, and verifies the returned provider node binding. Lost acknowledgements
are checked against the existing enrollment before spending another key. Disconnect
awaits listener shutdown and withdraws its own runtime lease; logout suspends the host
credential, removes local sign-in secrets, and retains the session store's account
boundary. Exact worker routes are in `leanzero-link/worker/CONTROLLER-API.md`.

## Roles and mixed versions

`MeshStatus.self_node_key` is `Self.PublicKey`; `MeshPeer.mesh_node_key` preserves the
`Peer` map key from Tailscale status. Names and addresses are not role authority.
`RegisteredPeers={protocol_version:1,lease_seconds:u32,revision:u64,peers:
[{mesh_node_key,role,owner_node_id}]}`. `PeerAdmission::registered(snapshot,request_started)`
accepts only worker-verified host keys before its monotonic deadline; the worker client
anchors that instant before HTTP. `PeerRegistry::set_peer_admission` updates new-work
admission. New targets are filtered before poll/stream tasks exist. Existing caches are
retained on expiry, marked stale with unavailable permissions/capacity. Existing accepted
work is not cancelled. New polls/reconnects wait for authorization; `compute_peer_nodes`
excludes stale roles at every placement look, and manager execute/model/distributed/job/
inference requests recheck `require_compute_peer` before resolving the destination. Accepted distributed ranks retain an opaque process-local continuation capability, issued only after a successful rank start and bound to manager, connection generation, run/rank and exact peer identity. Typed polls/stops can continue through lease expiry; authoritative revocation, peer replacement/removal, terminal status or reconnect permanently invalidates the capability. Raw operations and new work remain freshly gated.

Core integration switches compute discovery in `mlx_distributed_link`, `mlx_placement`,
and `mlx_replica` to `compute_peer_nodes`. Two cadence-only calls in `mlx_remote_single`
use `peer_liveness_interval`, not dispatch authorization, so registry expiry cannot
masquerade as a failed in-flight request. The caches still report real mesh liveness.

Automatic approval review rejected an earlier proposed timer that would abort all peer
tasks and clear mirrored sessions on registry expiry. That approach was not applied.
The implemented alternative retains those objects and fences new work instead. No
approval workaround, daemon termination, or persistent session deletion occurred.

Production Goose injects host lifecycle and authenticated `/v1/controller/hosts/peers`
admission before publishing mesh peers. Library embedders without that factory retain
legacy compatibility; a failed host bootstrap never falls back to it. Do not deploy
controller mesh enrollment until the worker binding and manager rollout are proved. `NodeState.controller` is
optional advertisement only: absent means
update_required and zero controller capabilities. It cannot authenticate a role.

## Verification boundary

Run `cargo fmt -p leanzero-link -- --check`, `cargo build -p leanzero-link`,
`cargo test -p leanzero-link` and `cargo clippy -p leanzero-link --all-targets -- -D warnings`.
The new `tests/controller.rs` uses loopback HTTP/WebSocket fixtures, no mesh daemon.
TLS/identity module tests prove their real listener and persistence paths. Never run
ignored daemon tests without before/after personal Tailscale identity evidence.

Installed-host, provider-authority, native certificate rejection and actual owner-run
disconnect proofs remain required integration gates. Results are reported separately
by the orchestrator; this document is not a passing receipt.

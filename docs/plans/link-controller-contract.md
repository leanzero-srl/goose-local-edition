---
title: Link controller implementation contract
date: 2026-10-26
status: ready-for-first-source-cut
source_revision: e79a6f60eccecabc3a8dd8b3e4e2651568034572
implementation: not-yet-implemented
---

# Link controller implementation contract

This cuts [the approved architecture](link-remote-control-plan.md) into independently owned implementation slices. It was checked against the current worktree, the evidence map, and both `/tmp/goose-*-controller-audit.md` source audits. Nothing in this document claims that the new routes, registry, persistent catalog or owner-run control already work. The first source slice is **A: device authorization and role registry**; B can proceed concurrently after agreeing the DTOs below. C depends on both.

## A. Device authorization and role registry

Keep existing account-host login, nodeSecret derivation and `/v1/swarm/*` authentication working. Add a separate `/v1/controller/*` router. Never broaden `require_token` to accept a controller token: that would grant raw execute, inference, distributed and shell-job access. Controller read access is authorization too; no existing raw session stream or job log is exposed simply because the token is valid.

### Credentials and registry

The worker remains the only JWT verifier. Controller login returns a device access token and rotating refresh credential, **neither the legacy account JWT nor nodeSecret**. Access claims are signed using the worker's existing HS256/WebCrypto mechanism with a separate token-version/type and required issuer/audience. Preserve `verifyJwt`'s existing `ver == 1` requirement: device tokens use `ver:2`, `token_use:"controller"`, `aud:"leanzero-link/controller"`, plus `iss`, opaque `sub` (account ID), `device_id`, `device_revision`, `jti`, `iat`, `exp`. The worker verifies the signature and current registry on every introspection. A phone cannot exchange this token at the legacy join-key endpoint.

The new owner router treats the access token as opaque. It presents it to the worker over HTTPS with the host's separate registered credential. The worker binds account, registered receiver owner ID, requested scope, device role, device revision and revocation state. The response, not request JSON, creates `AuthenticatedDevice`. Target selection in the URL must equal the receiving registered owner; no hostname, display name, caller `origin_node` or submitted `device_id` provides authority. This follows the protected-resource/authority split in [RFC 7662](https://www.rfc-editor.org/rfc/rfc7662), without claiming that the current passwordless service is already a complete OAuth server.

Registry schema v1 has account records, device records, refresh-token hashes/families, host bindings/leases, enrollment challenges and idempotency receipts. An account transaction atomically consumes a challenge, creates/renews/revokes a device, increments revision, and records its receipt. Role and granted scopes come from server policy; a request cannot promote a controller to a host. Refresh replay revokes that refresh family. No cleartext bearer/OTP is logged or stored. Credential delivery retries use an explicit enrollment request ID and recoverable enrollment receipt; they must not silently enroll a second device after a lost response.

Current deployment documentation names the self-hosted Node adapter: `NODE-SELFHOST.md`, `src/node-server.ts` and `createFsKvStore`, loopback port 8791 behind Funnel. No running production process was queried during this audit. The Cloudflare adapter is also supported and needs a real implementation:

- **Node:** add `SqliteDeviceRegistry` using `node:sqlite`, a dedicated `controller-registry.sqlite` under `LINK_KV_DIR`, versioned schema and `BEGIN IMMEDIATE` transactions. Use private directory/file permissions and a supported runtime API; the local read-only runtime check is Node v26.5.0. SQLite supplies cross-process exclusion; do not rely only on a JavaScript promise chain. Corrupt/unreadable registry is an error, never a fresh empty account. The [Node SQLite API](https://nodejs.org/api/sqlite.html) is the primary implementation reference.
- **Cloudflare:** add a SQLite Durable Object keyed by opaque account ID, `LINK_DEVICE_REGISTRY` binding and a named `new_sqlite_classes` migration. All authoritative reads and writes go through the object, including introspection. [Durable Object storage](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/) supplies transactional, strongly consistent storage. [Workers KV](https://developers.cloudflare.com/kv/concepts/how-kv-works/) is eventually consistent and lacks the atomic transaction needed here; the existing `src/index.ts` explicitly implements `update` as read-then-write. Merely serializing writes while introspection still reads KV does not satisfy revocation.

Both adapters ship behind the same `DeviceRegistryStore` operations and the same concurrency contract tests. Missing required deployment binding is an explicit unavailable/configuration error, but an unimplemented adapter returning 501 is not completion. Legacy OTP/rate-limit KV is kept; the new device OTP challenge and consume live in the registry. Existing `claimAttempt` spends an attempt atomically but compares/deletes later, allowing concurrent correct attempts to reach success; do not reuse that gap for device enrollment.

### Worker endpoints (new; snake_case JSON)

| Method and path | Authentication and request | Response |
| --- | --- | --- |
| `POST /v1/controller/auth/request-code` | Public rate-limited `{email, enrollment_id:UUID}` | `{challenge_id:UUID, expires_at:RFC3339}`; ordinary OTP delivery |
| `POST /v1/controller/auth/verify` | `{challenge_id, enrollment_id, code, device_name, platform:"ios"\|"ipados"\|"desktop"}` | `DeviceCredentials` below; role fixed to controller |
| `POST /v1/controller/auth/refresh` | `{device_id, refresh_token, request_id:UUID}` | Replacement `DeviceCredentials`; atomic one-use rotation |
| `POST /v1/controller/mesh/join-key` | Device Bearer; `{enrollment_id}` | `{auth_key, login_server?, expiry_seconds, enrollment_id}`; **no nodeSecret** |
| `POST /v1/controller/mesh/confirm` | Device Bearer; `{enrollment_id, mesh_node_key}` | `DeviceRecord`; worker verifies the provider record belongs to the issued enrollment/account, then binds it |
| `GET /v1/controller/devices` | Device Bearer + `devices.read` | `{revision:u64, devices:[DeviceRecord]}` |
| `POST /v1/controller/devices/{device_id}/revoke` | Device Bearer + `devices.manage`; `{request_id, expected_revision}` | `{device_id, revision, revoked_at}` |
| `POST /v1/controller/hosts/register` | Existing host account JWT, verified only by worker; `{owner_node_id, registration_id, host_name, protocol_version:1}` plus existing host credential on renewal | `{host_credential, host_revision}` initially; duplicate owner registration without the existing binding is conflict |
| `POST /v1/controller/hosts/lease` | Host credential; `{runtime_id, owner_node_id, store_generation, catalog_revision, mesh_node_key}` | `{lease_id, lease_expires_at, host_revision}`; duplicate live runtime/identity conflicts |
| `GET /v1/controller/hosts` | Device Bearer + `catalog.read` | `{revision, hosts:[HostDescriptor]}`; only verified registered hosts expose controller capabilities |
| `POST /v1/controller/introspect` | Host credential; `{token, owner_node_id, required_scope}` | `{active:false}` or `AuthorizationLease` below |

`DeviceCredentials = {access_token:string, token_type:"Bearer", expires_at:RFC3339, refresh_token:string, device:DeviceRecord}`. `DeviceRecord = {device_id:UUID, name:string, platform:string, role:"controller"|"host", revision:u64, scopes:[ControllerScope], created_at:RFC3339, revoked_at:RFC3339|null}`. No host credential is returned from controller endpoints. Explicit device administration grants never imply host enrollment credentials or access to the legacy node secret. Secure refresh handling and lost-response recovery must be tested before enabling this endpoint in an installed client.

`AuthorizationLease = {active:true, account_id:UUID, device_id:UUID, device_revision:u64, role:"controller", owner_node_id:UUID, scopes:[ControllerScope], token_expires_at:RFC3339, lease_seconds:u32}`. A host retains its registration credential in its existing protected identity/secret path; account logout clears credentials without silently reassigning durable host/work identity. Proposed initial authorization lease policy is at most 30 seconds, refresh at 15 seconds; these are authorization transport limits, never model/run limits. Measure the actual revocation bound before claiming it.

Scopes are exact strings: `catalog.read`, `history.read`, `work.send`, `work.steer`, `work.stop`, `input.answer`, `resources.read`, `models.read`, `models.download`, `models.manage`, `providers.read`, `providers.manage`, `devices.read`, `devices.manage`. The receiving host's corresponding local switch remains a separate AND condition. Absent/unknown scopes are denied. No arbitrary `mlx`/`distributed`/job operation becomes available by wildcard.

### Freshness, streams and old peers

The new native client sends only `Authorization: Bearer`; legacy query-token compatibility stays on `/v1/swarm`. Every controller route rejects `Origin` before authentication. A subscription carries an `AuthenticatedDevice` plus a monotonic authorization deadline anchored **before** the introspection request begins, capped by token expiration. Refresh starts before expiry. Each frame checks that deadline; no event queue drains after it expires. Revocation closes with WS 4403, expired token with 4401, and unavailable freshness with 4413. Close first and then refresh/reconnect; no stale-allow fallback. New mutations re-introspect at acceptance and immediately before dispatch. Revocation stops pending commands from dispatching; work already running continues until an independently authorized stop. Permission requests remain pending when a controller disappears.

Add `ControllerProtocol {version:1, owner_node_id:UUID, device_role:"host", capabilities:[ControllerScope]}` as an optional additive `NodeState.controller` field. Missing means `update_required` in the controller product, with **zero controller capabilities**, never implicit support. Keep existing `NodeAllows` semantics for legacy Mac-to-Mac behavior. A registry-verified controller peer is excluded before `PeerRegistry::set_peers` creates polling/stream tasks. Unverified peer role is not inferred from `NodeState` or hostname. The mesh parser currently discards the `Peer` map's node-key identity; preserve provider node identity in `MeshPeer` and bind it to the authoritative registry. Verify the exact provider identity/enrollment linkage against official Headscale/Tailscale APIs before implementation; a client-asserted node key is not proof.

Use one role/admission predicate for the controller view, job placement, inference, distributed participants and resource totals. Existing inference/distributed consumers include `mlx_remote_single.rs`, `mlx_distributed_link.rs`, `mlx_placement.rs`, `mlx_replica.rs`, plus `LinkManager::place_job_with`; these need narrow integration edits when the role registry lands. Controller peers never become Offline worker rows or engine-probe targets in updated hosts. Old binaries cannot learn a new role by receiving an extra JSON field: rollout updates fleet discovery first, and the registry marks unupgraded hosts incompatible for controller access. Do not claim old binaries themselves now suppress controller probes. Before controller mesh enrollment, demonstrate the mixed-version discovery/ACL behavior; if legacy nodes can see new controllers, either complete the host upgrade or implement/prove provider visibility isolation.

## B. Durable owner identity and catalog

`stable_node_id()` in `acp/server/link.rs` is a sanitized hostname with an optional six-hex mesh suffix. `manager.rs::node_suffix` writes that suffix only at connect. Neither is an immutable host identity. Add a separate `owner_identity.rs` store: CSPRNG UUID created atomically before the state source is constructed, private file, no time/PID/name fallback, unreadable/corrupt is an error. Keep mesh hostname and legacy `node_id` as locators during compatibility. Verified alias records map old names to the new owner; never merge different hosts because names match.

Host registration fences duplicate active runtimes. The store also has `store_generation:UUID`; an explicit restore/replacement rotates generation and invalidates old cursors, commands and pending-input identifiers. Compare the registered catalog high-water to local state at host-lease acquisition and reject rollback. A byte-for-byte clone of all credentials/state cannot be identified from filenames alone: live lease fencing and the authority's external high-water are required. Do not claim arbitrary offline restore detection until that path is exercised. Controller mutations remain unavailable on an unfenced/unregistered store.

Canonical wire types belong in `crates/leanzero-link/src/controller_wire.rs` and have shared Rust/Swift golden-schema tests:

```text
WorkKind = "chat" | "agent_work" | "benchmark" | "shell_job"
WorkRef = {owner_node_id:UUID, kind:WorkKind, work_id:UUID}
CatalogCursor = {owner_node_id:UUID, store_generation:UUID, revision:u64}
WorkState = "idle" | "running" | "waiting_input" | "paused" | "finished" |
            "failed" | "interrupted" | "deleted"
CatalogEntry = {work:WorkRef, store_generation:UUID, revision:u64, title:string,
  working_dir:string|null, state:WorkState, current_run_id:UUID|null,
  capabilities:[WorkAction], parent:WorkRef|null, related:[WorkRef],
  updated_at:RFC3339, last_event:EventPreview|null, archived:bool}
HostDescriptor = {owner_node_id:UUID, name:string, device_role:"host",
  protocol_version:u32, availability:"online"|"offline"|"update_required",
  last_seen_at:RFC3339|null, endpoints:[MeshEndpoint], capabilities:[ControllerScope]}
```

Legacy session IDs, run directories, benchmark slots and job IDs are private adapter locators. A prompt job links to its chat and does not create a second independent work item. Immutable IDs remain through title/cwd/archive changes. Copy/import creates new work identity even when exported JSON contains old UUIDs. Delete writes a tombstone; recreation with the reused daily local session ID gets a fresh UUID. Remote GET never activates an Agent or rewrites cwd/provider.

### Actual storage migration

`session/session_manager.rs` owns `<data_dir>/sessions/sessions.db`. Current schema is **15**. `pool()` chooses `create_schema` or `run_migrations`; `run_migrations` runs `BEGIN IMMEDIATE`, reads `schema_version`, applies every numeric `apply_migration` arm, updates the version and commits. Add **16** and call the same new `work_catalog::create_objects` from fresh schema. Existing rows receive stable UUIDs in the migration transaction. Test simultaneous first opens and migration from v15; changing only the new-database schema is insufficient.

Recommended new objects in the same DB: `controller_store` (generation/revision state), `work_identity` (UUID, kind, locator, creation/tombstone), `catalog_versions` (immutable entry versions/tombstones, global revision), `work_events` (per-work durable revisions), `controller_commands` (device-bound dedupe/receipt/dispatch state), `pending_inputs` (one-use owner/run/request identity). Only active identity has a unique `(kind, local_locator)`; tombstones retain old locators. Preserve the existing `sessions.id`, messages and import/export formats.

Install durable SQLite identity/journal triggers or equivalent transaction helpers that cover **every writer**, including other Goose processes. New session insert and UUID allocation are one commit. Audit `create_session`, `import_legacy_session`, `update_session`, `add_message`, `replace_conversation_inner`, `update_extension_data`, `update_message_metadata`, `update_tool_request_meta`, `delete_session`, both truncate methods, copy/import and archive updates. `truncate_conversation` currently deletes outside an explicit transaction; it must join the identity/event transaction. SQL triggers avoid missing a direct SQL writer or an older process. No custom connection-only SQLite function may be required by a trigger that older Goose processes can execute.

Catalog versions store the entry state at their revision, so a paginated snapshot at watermark W remains consistent while later writes occur. The first response captures W; later pages select last version per work at/before W. Retain versions until active snapshot tokens expire; expired retention returns `snapshot_required`, never a mixture of pages from different watermarks. Notify the in-process broadcaster after commit; one shared journal watcher detects writes from other processes without a full table scan per viewer. Runtime live state enters this same journal, separate from persisted transcript bytes. Durable message identity/revision is the reconciliation source; partial output is replaceable, not a second message append.

### Owner API (new; all device-authenticated)

| Method and path | Request | Response |
| --- | --- | --- |
| `GET /v1/controller/capabilities` | None | `{protocol_version:1, host:HostDescriptor, store_generation, allowed_scopes}` |
| `GET /v1/controller/catalog` | `?page_token=<opaque>` after initial request | `{entries:[CatalogEntry], watermark:CatalogCursor, next_page_token:string|null, resume_cursor:string}` |
| `GET /v1/controller/catalog/changes` | `?cursor=<opaque>` | `{changes:[CatalogChange], cursor:CatalogCursor, has_more:bool, resume_cursor:string}` |
| `GET /v1/controller/catalog/stream` | `?cursor=<opaque>`; Bearer header | WS durable catalog changes only; no unselected transcript |
| `GET /v1/controller/work/{kind}/{work_id}` | `?store_generation=<UUID>` | `WorkSnapshot` with work identity, revision, run, pending inputs and history watermark and signed `work_cursor` |
| `GET /v1/controller/work/{kind}/{work_id}/history` | `?store_generation=<UUID>&page_token=<opaque>` | `{work, store_generation, revision, items:[HistoryItem], next_page_token, work_cursor:string}` |
| `GET /v1/controller/work/{kind}/{work_id}/changes` | `?store_generation=<UUID>&cursor=<opaque>` | `{frames:[WorkFrame], resume_cursor:string, has_more:bool}`; finite selected-work replay, including an empty page |
| `GET /v1/controller/work/{kind}/{work_id}/stream` | `?cursor=<opaque>` | `WorkFrame` below; selected work only |
| `POST /v1/controller/work/{kind}/{work_id}/commands` | `WorkCommand` below | HTTP 202 `CommandReceipt` after durable acceptance; retry returns same receipt |
| `GET /v1/controller/commands/{command_id}` | None | `CommandReceipt`; authenticated account/device policy applies |
| `GET /v1/controller/resources` | None | Existing `NodeLoad` sample + sampled_at; unknown capacity stays unknown |

Owner ID comes from the verified receiving host and is checked against every cursor/body WorkRef. Opaque cursors are versioned encodings authenticated by the host; they include query/filter/snapshot identity, not just an integer. Catalog change is `{cursor, type:"upsert"|"tombstone", work, entry:CatalogEntry|null, resume_cursor:string}`. Work stream frame is `{work, store_generation, stream_epoch:UUID, sequence:u64, work_revision:u64, run_id:UUID|null, type, payload, resume_cursor:string}`. Event variants include committed message/upsert, partial replacement, history replacement, pending-input change, command receipt, run state and error. `HistoryItem` preserves the existing serialized Goose message content/metadata and stable message ID with a revision; do not expose credentials, provider config, or private engine reasoning beyond the existing user-visible transcript contract. Partial replacement carries `{message_id, message_revision, offset_bytes, content}` and is applied only to the matching run/message revision.

The existing `PubSub` RAM ring and `SessionDelta {session_id,...}` are not this protocol. Its sequence resets and its peer mirror is keyed only by session ID. Change that legacy mirror map to `(origin_node_id,session_id)` in its own compatibility patch, but do not invent durable continuity for it. New controller events retain owner identity and generation at every hop. Snapshot/history plus post-watermark replay closes the subscribe race. Reject a future cursor, changed generation or expired history; never clamp it.

## C. Shared run control and lifetime

```text
WorkAction = "send" | "steer" | "stop" | "answer_input" | "note" |
             "pause" | "resume" | "tick"
WorkCommand = {command_id:UUID, work:WorkRef, expected_store_generation:UUID,
  expected_revision:u64, expected_run_id:UUID|null, action:WorkAction, payload:ActionPayload}
CommandReceipt = {command_id:UUID, work:WorkRef, authenticated_device_id:UUID,
  revision:u64, state:"accepted"|"waiting"|"dispatching"|"delivered"|
  "rejected"|"outcome_unknown", run_id:UUID|null, message_id:string|null,
  input_id:UUID|null, checkpoint_id:string|null, reason:string|null, updated_at:RFC3339}
```

Payloads are typed: send/steer `{content:[ControllerContent]}` with text and explicitly uploaded owner resource IDs (never phone filesystem paths); stop `{}`; answer `{input_id, option_id, value?}` against the exact current permitted choices; note `{text}`; pause/resume/tick `{}`. Unsupported kinds/actions return 422 `unsupported_action`; no generic ACP dispatch or shell passthrough. Busy send is an explicit next-turn queue. Steer/stop require the exact non-null active run. Idle send requires null run and matching idle revision. Queue readiness uses the recorded prior-run completion transition, then reserves a fresh run under the same lock; later transcript appends do not accidentally invalidate a legitimate queued next turn. Object/generation replacement, device revoke, deletion or competing control transitions still invalidate it.

Pending inputs have a required typed `payload`: permission `{kind:"permission", request_id, tool_name, arguments}` contains the original tool request identity and exact argument object; form `{kind:"form", schema}` contains the original JSON Schema. The common `prompt` and `options[{option_id,label,accepts_value}]` remain. A controller must display the operation and arguments before enabling permission decisions; unknown payloads and unsupported form schemas fail closed. Form Submit sends its JSON object encoded in `value`, with required fields, enums and all original constraints validated by the owner before consumption. Invalid values leave the input open. A permission answer is Delivered only when its original provider/router reports actual delivery. A cancelled run or dropped receiver cannot acknowledge an answer; no observer disconnect chooses Cancel.

Idle `ask_user` questions retain their canonical item identity and expose `{kind:"question", item_id, why, recommended_answer, origin_run_id:UUID|null}`. Their common `run_id` is null because the original turn ended; active permission/form inputs retain the current run UUID. The controller submits the current idle revision and null expected run, with exact `choice_<index>`, free-text `answer`, or `dismiss` option. The owner resolves the original question once; an answer starts a fresh original-owner continuation with the existing `needsYouAnswers` mark, while dismissal starts no model. Other open questions remain open. Finite work changes use the same event conversion and signed cursor bindings as the selected stream; consumers persist `resume_cursor` even for an empty page, and fetch a new snapshot when the owner reports a gap.

The owner atomically checks identity/generation/control revision/run, binds the authenticated device, records canonical typed-payload digest and receipt, then acknowledges. Uniqueness is `(account_id, device_id, command_id)`; the same ID with different canonical contents is 409 `command_id_conflict`. Check again under the owner control lock at dispatch, together with fresh authorization. A worker crash after `dispatching` must reconcile with persisted run/message/checkpoint ID; absent proof becomes `outcome_unknown`, never an automatic duplicate prompt. A consumed answer is immutable; the competing answer returns 409 `input_already_answered` with current safe state. Delivery means the actual Agent consumed/accepted that message or the real adapter reported its checkpoint, not just a file write.

Implement `OwnerRunRegistry` once per backend, keyed by `WorkRef`, holding the same Agent, run ID, cancellation token, pending input and detached task. Local ACP and Link call this registry. `GooseAcpAgent::on_prompt` currently owns the reply future, calls `agent.reply`, taps it, processes events, permissions, loop ticks, cross-notes, usage, turn outcomes and reviewers; preserve those paths while moving task lifetime. `RunRegistration::drop` currently settles stopped output and clears the run when the viewing connection disappears. Sharing only its HashMap would still drop the future. Outbound notification failures must detach an observer rather than turn a running prompt into an error. Read-only load must not call `activate_acp_session`.

Local ACP reconnect takes an immutable committed-history boundary before the owner run begins, followed by the exact notifications emitted by that run. Loading installs a buffering observer while holding the same run lock used for start/finish; replay drains that buffer and switches to live delivery under the notification lock. Never deduplicate this boundary by persisted message IDs: the agent can move previously streamed thinking into newly identified tool messages. Run state, tool updates, input-adjacent notes and supported custom progress share the owner notifier. A run finishing or a new run starting during replay cannot replace that observer's buffer.

`start_active_run` currently registers in a connection map and that connection's `AgentManager`; Link binds whichever managers first constructed its state source. Replace this with the one owner registry and a narrow injected `WorkController` trait on the Link side. `on_steer` checks the run, finds the connection's Agent, checks again, then calls `Agent::steer`; the replacement must make exact-run check and enqueue one serialized operation. `on_cancel` currently permits a session-only stop; preserve local UI compatibility while remote commands require run identity. `handle_tool_permission_request` currently cancels confirmation on client-request error; move the original request and answer channel into the registry, making every UI connection an observer. Apply the same ownership to `needs_you` elicitation/questions and recipe input.

`GooseServeLeaseRegistry` currently stops local goosed after the last attached/pending window releases it. Add explicit application Link-host and active-run holders to the existing single-backend registry, so enabled hosting survives zero windows. `acquireLocal`, `releaseUnattached`, `releaseWindow`, `liveLeases`, `cleanupAll` and `stopAllAndWait` must share the holder rules. Quit still drains/stops the one owned backend and reports failures. No second mesh daemon or always-on launch service is introduced by this change.

Agent Work/benchmark adapters follow this shared door, preserving their real inbox/control semantics and checkpoint receipts. Swarm notes need command-UUID filenames and human-assisted provenance. Shell jobs expose actual log/cancel only; fix `jobs.rs::read_log` to seek/range rather than whole-file reread. These adapters cannot advertise chat steer because fixture routing returned success.

## Errors and implementation ownership

New errors are `{error:{code:string,message:string,current?:SafeCurrentState,retryable:bool}}`: 400 invalid request; 401 invalid/expired credential; 403 wrong account/owner/role/scope/host policy/revoked; 404 work missing; 409 stale run/revision/generation or idempotency/input conflict; 410 snapshot required; 422 unsupported action; 503 authority/store/owner unavailable. An absent implementation cannot return successful empty data. Work snapshot/version conflicts must never trigger automatic send/stop fallback in a client.

| Lane | Owns | First completion boundary |
| --- | --- | --- |
| A Link authorization | `leanzero-link/worker/**`; new Link `controller_wire.rs`, `controller_auth.rs`, `controller.rs`, `owner_identity.rs`; narrow `worker_client.rs`, `control.rs`, `wire.rs`, `state.rs`, `mesh.rs`, `manager.rs`, `lib.rs` and tests | Both registry adapters, controller enroll/refresh/revoke/introspection; new namespace denies by default; role-aware discovery; no mobile secret disclosure; bounded open-stream revoke |
| B Core catalog | `crates/goose/src/session/{work_catalog.rs,session_manager.rs,mod.rs}` and tests | Migration 16, UUID/tombstones/transactional journal, history snapshot and paginated watermark replay |
| C Run host | `crates/goose/src/work_control/**`, `acp/server.rs`, `acp/server/{link_serve.rs,needs_you.rs}`, narrow `agents/agent_manager.rs`/`lib.rs`; coordinated injection in `acp/server/link.rs` | Detached owner task, local/remote one-door commands, pending inputs, durable receipts; actual observer disconnect proof |
| Desktop lifetime/UI | `ui/desktop/src/gooseServeLeaseRegistry.ts`, coordinated `main.ts`, session/ACP UI files | Hosting with zero windows; remote read doesn't activate local agent; owner/control UI |
| Mobile | `ui/ios/**` and fixtures derived from A | Device credential storage, compatible-host discovery, catalog/read/commands and real mesh recovery |

`acp/server/link.rs` is a single-owner integration file: schedule A's immutable owner/injection patch before C's run wiring. A owns no session store or UI edits; B owns no protocol duplicate; nobody parallel-edits `server.rs`, `main.ts`, root manifests or campaign ledgers. Existing API shapes remain intact until upgraded clients negotiate controller protocol 1.

First A gate is real adapter concurrency plus negative controls: same OTP once, refresh replay, wrong account/owner/role, controller token to legacy join-key, forged device JSON, old NodeState without controller capability, controller excluded from every compute consumer, stream revoked while another controller survives, registry outage closes by deadline, queued command author revoked. First B gate is old/fresh/concurrent migration, same legacy ID on two stores, delete/recreate identity, rename, restore/generation fencing, snapshot changes between pages, transcript replacement/metadata/tombstone and impossible cursors. C requires actual two-window plus native-controller exercise of the same run, owner viewer close, permission race, stale stop and retry-after-lost-ACK. Isolated fixtures support these gates; they never substitute for installed real-path proof.

No source, build, daemon, deployment, production query or secret read was performed for this contract. Confidence is high in the source cut and store migration; provider mesh enrollment binding, mixed-version visibility and detached ACP lifetime require their named implementation proofs.

### Read adapter token policy (2026-10-26)

Catalog page/change/frame responses carry `resume_cursor`; selected snapshots/history carry `work_cursor`. Readable watermark structs are for display and fences, never accepted as unsigned authority. Tokens use a durable private random key with BLAKE3 keyed authentication, binding owner, account, device identity, purpose, generation, work and snapshot position. Pagination inherits the original token expiry; replay renews only after reading committed journal data. Expired tokens require a new snapshot. The store currently retains all journal versions; activating pruning requires a retained watermark floor enforced before reads. History `revision` is the global snapshot watermark, which can exceed an entry revision when another work changed. Compare subsequent history pages to their first history watermark, never assume equality with the catalog entry revision.

An open-work snapshot also carries `live_messages:[HistoryItem]` for in-progress assistant messages at its watermark. Durable partial events retain deltas rather than a full accumulated transcript per token. `partial_replacement` carries `{message_id,message_revision,offset_bytes,content}` with `content` a user-visible Goose message chunk; `offset_bytes` counts visible text bytes accumulated for that message. `partial_discarded` carries `{message_ids:[string]}`. A committed `message_upsert` for the same ID replaces the partial. Snapshot assembly reconstructs current partials once and excludes messages committed or discarded by its watermark. A `message_upsert` payload is `{item:HistoryItem,previous_message_id:string|null}`; the optional previous ID removes an older identity after a stored message-ID edit. `message_deleted` carries `{message_id}`. `WorkFrame.sequence` uses the durable global journal revision, so it increases but can skip revisions belonging to other work or filtered private events. The durable stream epoch is the store generation.

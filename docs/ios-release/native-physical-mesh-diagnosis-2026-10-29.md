# Physical phone mesh diagnosis — 2026-10-29 09:55 local

Read-only investigation. No UI, restart, install, enrollment mutation, credential copy or archive performed.

## Confirmed

Root observed physical phone setup source15a3625e41 after owner OTP: My iPhone/Controller, catalogs from Gabee55, original laptop557 and workhorse165; Workmule Offline; isolated laptop Mesh setup needed. Fleet remains busy for >3 minutes. Opening existing Cloud controller test reports “Connect this device’s embedded mesh before opening a Mac.” Phone briefly locked between enrollment and opening the chat.

That exact text exists ONLY at EmbeddedMesh.swift:145, under canImport(TailscaleKit), when node==nil. It is not the no-framework branch, peer readiness timeout, TLS failure or authority denial. Actual setup IPA linkage was verified by root in /tmp/goose-phone-setup-verification.json. EmbeddedMesh.swift and GooseRemoteApp.swift are unchanged between installed15a and currentHEAD.

Safe authority read at07:52:59UTC: device333b2a27-9958-4195-8341-2256696e8287, name My iPhone, platformios, rolecontroller, revision1, created07:48:53.698UTC, revoked_at=null,14scopes. Read at07:54:07UTC: mesh enrollmented48f07d-04da-4a89-999c-d287db99d22c has statusconfirmed, rolecontroller, same subject, cleanup_pending=false. No token, auth key, mesh binding key or sealed receipt was printed. The registry was opened SQLite mode=ro/query_only. No need for reenrollment is established.

## Lifecycle trace and bounded inference

ControllerStore.sceneBackground cancels syncTask, stops streams and awaits mesh.suspend(). suspend changes its lifecycle, closes cached transports, sets node=nil BEFORE awaiting current.close(). TailscaleNode.close is an actor method calling synchronous tailscale_close; Go invokes tsnet.Server.Close. sceneActive waits captured syncTask.value before invoking manualSync. manualSync refuses while syncTask exists. Thus a canceled sync that does not finish prevents all resume reconnect attempts.

Separately, manualSync sets syncRunning=true and connection.loading BEFORE awaiting mesh.prepare. prepare always begins await suspend(), then sets up/starts its new node, up() and authority mesh confirmation. syncResults from the last pass are NOT reset until prepare and /hosts succeed. Therefore settled catalog rows plus a busy fleet do not prove the present sync reached any owner. They fit a prepare blocked while old node closes, with node already nil. They also fit a canceled earlier sync that has not completed. Neither exact blocked await is proven from source alone.

Connection(for:) fails immediately for nil node; selected-work loading does not itself prepare a missing mesh node. Settings connected identity is authority identity, not mesh readiness. Remove-local and device-edit controls are disabled by syncRunning, explaining grey controls without revoked access.

Transport HTTP and websocket paths have explicit cancellation handlers calling NWConnection.cancel, and owner peer readiness checks task cancellation every poll. No obvious unlimited owner-catalog retry was found. Existing ControllerTests calls to sceneBackground are teardown; they do not prove prompt resume when prior synchronization/mesh shutdown is pending. Native source was not changed.

## Log limitation and next proving step

Supported /usr/bin/log collect --device-udid <exactphone> --start 2026-10-29T09:48 --size10m refused exit77 because physical collection requires root. sudo -n refused because password is required. No interactive prompt was issued. There is no physical native log proving which await is stuck, so a precise close-deadlock diagnosis would be overclaiming.

Next useful proof: root-controlled Console/device diagnostic capture or exact-source lifecycle instrumented build, with no credentials logged, distinguish scene active/background, sync identity, prepare/suspend start/end and node-up completion. Add controlled latch tests where background cancels active sync, old operation completes late, foreground reconnects, and stale operation cannot clear replacement state. Any repair should retain explicit node lifecycle ownership and fail-closed mesh-only transport; do not bypass the mesh guard or force re-enrollment.

Final archive remains held pending root acceptance and this physical failure investigation.

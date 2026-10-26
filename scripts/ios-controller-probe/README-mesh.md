# Disposable simulator mesh probe

This is a separate diagnostic bundle, `net.leanzero.goose.mesh-probe`, for iPhone and iPad simulators on iOS 18.1+. Its executable architecture is arm64. It does not implement production Link enrollment, the companion application, or an App Store release.

It imports the pinned TailscaleKit framework, starts a fresh ephemeral node against an explicitly validated loopback test control server, and runs HTTPS and WSS through that node's authenticated SOCKS proxy. URLSession uses ordinary server trust. There is no ATS exception, custom trust override, production credential read, or owner cancellation endpoint.

## Build

After checking available resources, the coordinator builds the app from the already-built framework; this script never rebuilds TailscaleKit:

```sh
bash scripts/ios-controller-probe/build-mesh.sh \
  /tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator/TailscaleKit.framework \
  /tmp/GooseMeshProbe.app
```

The output path must not already exist. The helper uses the selected simulator SDK explicitly, includes iPhone and iPad device families, embeds the framework, and ad-hoc signs the bundle. The receipt records the SHA-256 hashes of the framework binary and probe Swift source. Install only into disposable simulators with the fixture's disposable CA installed as a trusted root. The coordinator owns simulator creation, trust setup, install, configuration injection, launch, evidence capture and cleanup.

## Injected runtime configuration

Before launching, write `Documents/mesh-config.json` in the installed app container. The fixture's `runtime-config.json` is translated into this shape for each controller. None of this configuration belongs in version control or release artifacts.

```json
{
  "schemaVersion": 1,
  "fixture": true,
  "runID": "<fixture 64-character lowercase hex run ID>",
  "controllerID": "fixture-phone",
  "controlURL": "http://127.0.0.1:<fixture control port>",
  "authKey": "goose-ios-disposable-<64-character lowercase hex random key>",
  "healthURL": "https://<fixture mesh IPv4>:8443/health",
  "eventsURL": "wss://<fixture mesh IPv4>:8443/events",
  "bearerToken": "<fixture phone token>",
  "expectedNonce": "<fixture random nonce>",
  "expectedOwnerID": "fixture-owner",
  "expectedEpoch": "<fixture epoch>"
}
```

Use `fixture-ipad` and its separate token for iPad. Control is constrained to literal `127.0.0.1`, HTTP and an explicit port; the fixture key must match the disposable prefix and random hex format. Run ID, nonce and epoch must each be 64 lowercase hexadecimal characters. Peer endpoints must share a literal IPv4 authority inside `100.64.0.0/10`, explicit port, HTTPS/WSS schemes and the expected endpoint paths. User information, queries and fragments are rejected. Redirects are rejected without changing TLS validation.

## Evidence and boundaries

The UI displays pending, running, passed, failed, skipped and unproven checks. `Documents/mesh-receipt.json` is atomically rewritten after each transition, including observed HTTP status, response identity, nonce, event sequence, assigned node addresses, URL error domain/code, and cleanup results. It never serializes bearer tokens, mesh auth keys, proxy credentials or local API keys. A completed pass requires every check to have actually passed; an unavailable prerequisite skips its dependants. Successful command recovery cannot turn an unproven acknowledgement loss into a full pass.

Checks require the health response and WSS connected envelope to match this run, owner, epoch, controller and nonce; the first event sequence must be positive. A wrong token must receive HTTP 401 without the nonce from the same health endpoint. An independent ephemeral URLSession with no configured SOCKS proxy must fail to obtain that nonce. HTTP request/resource and WebSocket-envelope deadlines are transport test limits only. A direct-session failure establishes only that this route did not retrieve this fixture nonce on this simulator during the probe.

The node readiness deadline explicitly closes and discards the controller node to interrupt the pinned wrapper's detached `up()` operation. Cancelling the Swift task alone is insufficient. Cancellation cannot count as a successful direct-route negative control. Launch with `SIMCTL_CHILD_TS_NO_LOGS_NO_SUPPORT=true` in addition to the probe's explicit log-discard sink, as required by the pinned upstream transport's test setup.

Sockets and sessions are invalidated; the controller node is explicitly closed and discarded, and its unique state directory is removed. `down()` is never used because the pinned wrapper incorrectly calls `tailscale_up`. The fixture owner is left running for coordinator readback and shutdown. Inspect both the app receipt and fixture request ledger: the app alone does not prove that requests arrived from a mesh peer.

This proves only the measured disposable simulator transport. It does not prove production OTP enrollment, durable controller credentials, production authorization, real-device suspension/cellular recovery, App Store acceptance, or remote tool execution.

## Optional command and replay checks

Add both `siblingControllerID` and `siblingBearerToken` to the injected configuration to enable these checks. For phone these are the fixture's iPad device ID/token; for iPad they are the phone device ID/token. These are disposable fixture credentials only. The receipt explicitly records `commandTestingEnabled`; absent sibling fields mean these tests did not run.

Before its first command POST, the probe atomically writes and independently reads back `Documents/mesh-command-<controllerID>.json`. This journal contains the fixture run ID, controller identity, randomly generated command UUID, fixed harmless test payload and replay cursor. It contains no token. A later launch with the same fixture run reuses this identity; a different fixture run creates a new command identity. The journal remains after node cleanup so that command recovery across app relaunch can be exercised.

The first request asks the fixture to drop the acknowledgement after accepting a harmless synthetic command. The probe must actually observe a transport error; receiving HTTP headers fails this particular check, including if URLSession silently retried it, while the independent status/retry checks still continue. The observed error is persisted. An owner status lookup must then recover that exact accepted command with one delivery, and an explicit same-ID retry must return `duplicate: true` and the identical command. Changed payload under the same ID must return 409. The sibling token must receive POST 409 and status GET 404 without seeing the command ID; a final owner lookup must still report the original command and one delivery.

These sibling requests test token ownership rules through this controller's mesh node. They do not claim simultaneous contention between devices. The coordinator separately runs the actual iPhone and iPad nodes and checks the fixture's peer/request/delivery evidence.

After disconnecting before the command, the probe reconnects WSS with the durable `after` cursor. It requires the missed `command_accepted` event to carry the exact command, identity and nonce, followed by an increasing `connected` event. A reused journal always triggers an authoritative status lookup before any command mutation. If the owner already accepted it, a separate recovery check passes and the probe continues status, retry, ownership and replay checks with the same identity. A missing acknowledgement observation remains unproven; any stored error appears only as explicitly labelled prior-launch evidence and cannot pass the current-launch acknowledgement check. If the owner reports 404, the probe continues with the same saved command ID. Node state is fresh on every launch; this proves command reconciliation and stream replay against the disposable owner, not real-device suspension recovery.

## Deliberate death before acknowledgement journaling

The optional fixture-only `crashCheckpoint` value `after-command-transport-before-journal` reproduces death after the first request's transport result but before its interruption is written to the command journal. It requires the sibling fields. The probe writes an unfinished checkpoint receipt, then exits only its own simulator process with exit code 86. There is no graceful node cleanup on this first leg. The coordinator must verify actual owner acceptance from the fixture delivery log; reaching this checkpoint alone does not establish that the request reached the owner.

1. Start with a fresh command journal on the chosen disposable simulator. If reusing a simulator from a completed command test, preserve its evidence and remove only its `Documents/mesh-command-<controllerID>.json` while the app is stopped. Add the exact `crashCheckpoint` field to its injected configuration.
2. Launch. Require an unfinished receipt whose `lost-ack` check is `unproven` with the requested checkpoint, plus a command journal with no `interruption` field. Confirm the fixture delivery log records exactly one delivery for that journal's command ID. Save the unfinished receipt before relaunch overwrites it.
3. Relaunch with the same fixture, configuration and untouched command journal. The status-before-resubmission path must recover that command and bypass the deliberate exit. Require `command-recovery`, `command-status`, `command-retry`, `body-conflict`, `token-ownership` and `replay` to pass. The acknowledgement check remains `unproven` and the overall receipt remains `passed: false` because this launch did not observe its loss.
4. Compare the fixture's delivery records before and after: the same command must still have one delivery. Verify current-launch node close/state removal and preserve both receipts. Finally discard the disposable simulator/container to remove state left by the intentionally abrupt first exit.

This scenario passes command recovery only when the app and independent owner evidence agree. Its deliberately incomplete acknowledgement evidence must never be presented as a fully passing transport/command probe.

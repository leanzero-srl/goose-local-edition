# Disposable iOS mesh fixture

This Go test runs an actual tsnet owner, an in-memory Tailscale test control
server, and local DERP/STUN. It serves HTTPS and WebSocket traffic on the owner's
mesh listener. It does not use production Link, personal Tailscale credentials,
existing node state, a real agent, or model inference.

`TestIOSMeshFixture` uses a real `testing.T`, `integration.RunDERPAndSTUN`, and
`testcontrol.Server`. A non-nil DERP map is mandatory: upstream explicitly uses
production DERP when the map is nil. Every advertised relay/STUN address is
checked for loopback before the owner starts. Control and DERP bind loopback.
Upstream's STUN helper binds an ephemeral UDP wildcard socket but advertises
only loopback; this harness does not claim all its sockets bind loopback.

Dependencies are pinned to Go 1.27.1, `tailscale.com v1.104.1`, and
`github.com/coder/websocket v1.8.15`. The other dependency versions/checksums were
copied from `tailscale/libtailscale` commit
`d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`. No dependency version was upgraded.

## Build, test, start, stop

The coordinating operator owns machine headroom checks and build placement.
These commands assume this directory is the working directory. Compilation may
need network access to fetch a pinned dependency that is not in the local cache.
Resource estimates are unmeasured: budget 2–4 GB RAM for the two-worker compile,
1–3 GB cache growth, and 200–600 MB for the running fixture. Measure the actual
process before combining it with another heavy task.

```sh
env GOTOOLCHAIN=go1.27.1 GOMAXPROCS=2 \
  go test -p 2 -c -o /tmp/goose-ios-mesh-fixture.test .
/tmp/goose-ios-mesh-fixture.test -test.run '^TestFixture' -test.v
env -u TS_AUTHKEY -u TS_AUTH_KEY -u TS_CLIENT_SECRET -u TS_CLIENT_ID \
  -u TS_ID_TOKEN -u TS_AUDIENCE -u TS_CONTROL_URL \
  GOOSE_IOS_FIXTURE_OUT=/tmp/goose-ios-mesh-run-unique \
  /tmp/goose-ios-mesh-fixture.test \
  -test.run '^TestIOSMeshFixture$' -test.v -test.timeout=0
```

`GOOSE_IOS_FIXTURE_OUT` must name a **new**, disposable directory whose parent
exists. Existing directories are refused. No variable means the long-running
fixture is skipped, so ordinary contract tests terminate. Keep the fixture
process attached to a supervised job; it starts no service or daemon.

Startup prints only the config and stop-file paths. Read the config in the
operator script, not into shared logs: it contains disposable bearer credentials.
Create the path in `stopFilePath` to stop. SIGINT/SIGTERM also run teardown. Go
cleanup closes the HTTPS owner, control, relay, and STUN servers and removes the
owner's temporary state. It deletes `runtime-config.json`; `stopped.json` is
written after those cleanups, alongside token-free evidence. Forced SIGKILL
cannot run cleanup; explicitly remove that abandoned run's config/directory.

The Go fixture disables log uploads with `TS_NO_LOGS_NO_SUPPORT`; tsnet also
skips its logger under the real Go test runner (`tsnet.go:startLogger`). Embedded
simulator clients must independently disable their library's telemetry before
starting. This is a local enrollment/control/relay fixture, not a measured
assertion of zero external packets from every dependency.

## Config and client mapping

`runtime-config.json` has these keys:

```text
schemaVersion: 1
fixture: true
runID: random 64-character lowercase hex
controlURL: http://127.0.0.1:<ephemeral-port>
fixtureAuthKey: goose-ios-disposable-<random 64-character lowercase hex>
peerHTTPSURL: https://<owner mesh IPv4>:<ephemeral-port>
peerWSSURL: wss://<owner mesh IPv4>:<ephemeral-port>/events
nonce, epoch: independent random 64-character lowercase hex
ownerID: fixture-owner
caCertificatePath: absolute path to public ca.pem
stopFilePath: absolute path to stop
controllers.phone: { deviceID: fixture-phone, token: random hex }
controllers.ipad: { deviceID: fixture-ipad, token: random hex }
```

To prepare the Swift probe's `Documents/mesh-config.json`, map `authKey` from
`fixtureAuthKey`, `controllerID` from the selected controller's `deviceID`,
`bearerToken` from its `token`, `healthURL` from `peerHTTPSURL + /health`,
`eventsURL` from `peerWSSURL`, and `expectedNonce`/`expectedOwnerID`/
`expectedEpoch` from `nonce`/`ownerID`/`epoch`. Copy `fixture`, `schemaVersion`,
`runID`, and `controlURL`. The optional sibling controller ID/token are solely
for negative ownership tests. Do not put these credentials in a product build.

Each run generates a disposable CA and an IP-SAN server leaf. CA/leaf private
keys stay in process memory. Install only `ca.pem` in a dedicated disposable
simulator using the selected Xcode's verified `simctl keychain add-root-cert`
command. Use ordinary TLS validation, with no trust-all delegate or ATS bypass.
Afterward remove the disposable simulator and test app/state; a copied config is
the operator's cleanup responsibility. Certificates expire after 24 hours.

## Wire contract

Every peer request requires `Authorization: Bearer <selected controller token>`.
Wrong tokens return 401, including before a WebSocket upgrade. The server derives
the device ID from the token. A request body cannot supply identity.

`GET /health` returns status 200 and
`{type:"health",runID,ownerID,epoch,nonce,deviceID}`.

`GET /events?after=N` upgrades to WSS. Envelopes are
`{type,runID,ownerID,epoch,sequence,payload:{nonce,deviceID,command?}}`.
Sequence starts at 1 and is per device. Each connection adds a `connected`
event; a reconnection first replays events after the requested cursor, then that
new connection event. Command acceptance produces `command_accepted`. Cursors
ahead of the stream return 409; invalid cursors return 400. Events remain in
memory for this run, without retention gaps. The next run has a new epoch.

`POST /commands` accepts only this diagnostic action:

```json
{"commandID":"random-UUID","kind":"fixture-command","payload":{"text":"fixture probe"},"dropAcknowledgement":true}
```

The ID accepts ASCII letters, digits and hyphens (1–128 characters); the text is
nonempty and at most 1024 UTF-8 bytes. Unknown fields, arbitrary tools/kinds, or
extra JSON values return 400. The body is limited to 4096 bytes.

Acceptance calls a harmless diagnostic sink that increments an independent
per-command invocation count and writes a `fixture-delivery` audit entry. The
receipt uses that counter (never a literal success count). Acceptance retains
`{commandID,kind,state:"accepted",deviceID,deliveryCount:1,acceptedAt}`. This
means that the fixture received the string, not that an agent obeyed it. Normal
first acceptance returns 202. With `dropAcknowledgement:true`, the first
acceptance instead closes TCP before sending any HTTP response, and records
`simulatedAcknowledgementLoss:true`. This flag exists only in the fixture.

A retry with the same ID, token-bound device, kind, and text returns 200 with
the same receipt and `duplicate:true`, regardless of the drop flag. A different
device or changed body with the same ID returns 409. `GET /commands/<id>`
returns its own device's receipt, or 404 for unknown/other-device IDs. Response
envelopes are `{type:"command",runID,ownerID,epoch,nonce,command,duplicate}`.
Lookup uses `duplicate:false`; it does not create a delivery.

## Evidence and limitations

`requests.jsonl` records request method/path, token-valid Boolean, mesh
`RemoteAddr`, and the device derived from the token. Command records add the ID,
delivery count, duplicate Boolean, and simulated interruption Boolean. It never
records tokens, auth keys, request headers, or payload text.
`command-receipts.json` contains the latest receipts without payload text.
Evidence writes fail loudly; lost network acknowledgements remain recoverable.
Count the `fixture-delivery` entries as independent evidence that duplicate
requests never invoked the diagnostic sink twice.

The contract tests cover wrong credentials, real HTTP lost acknowledgement,
same-device retry, cross-device conflict/status isolation, payload conflicts,
identity spoof rejection, both controller identities, WSS cursor replay, and CA
trust/IP-SAN validation. They use host loopback and do not prove simulator mesh
transport. The long-lived fixture's clean exit does not assert a simulator pass.

The dependency allocates synthetic mesh IPs in `100.64.0.0/10` without a public
subnet selection knob. Do not inspect or change personal Tailscale routes to
make the proof pass. The **same URL without the embedded SOCKS proxy must fail
to obtain this run's nonce**, while the proxied HTTPS/WSS requests must return
it and produce server records with mesh source addresses. A failed request alone
does not establish why routing failed. If direct access obtains the nonce, the
isolation proof fails and must be investigated before claiming success.

Command IDs/events are retained only for the current fixture lifetime. This
does not test production persistence, enrollment/OTP, real Link tokens or
revocation, simultaneous controller scheduling, background/cellular behavior,
real-device operation, a restart recovery protocol, or App Store readiness.

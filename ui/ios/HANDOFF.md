# Native controller handoff — 2026-10-26

## Owned source and status

This lane owns only `ui/ios/**`. Everything here is native SwiftUI/Foundation, iOS 18.1+, universal iPhone/iPad. The Xcode project has no package-manager dependency: `scripts/generate-project.py` is deterministic Python 3 standard library, and the generated project/scheme are committed. Fixtures are only in the test target; product startup has none.

Source architecture and release gaps are in `README.md`; measured results are in `VERIFICATION.md`. This is a foundation, not a release-ready app.

## Build and tests

Full Xcode is on laptop. Use node-dispatch, never raw heavy commands. DerivedData is `/tmp/goose-remote-derived`; built app `/tmp/goose-remote-derived/Build/Products/Debug-iphonesimulator/GooseRemote.app`, bundle `net.leanzero.goose-remote`. Shared scheme `GooseRemote`.

```
~/Projects/machine-sync/bin/node-run --host laptop --need-mem 4 --wait 60 --timeout 1200 --cwd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose -- 'ui/ios/scripts/build.sh --test --output /tmp/goose-remote-derived --destination "platform=iOS Simulator,id=B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9"'
```

Dedicated iPhone: B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9. iPad: F7078B46-CCF8-4779-9070-0BAFE33ADFB7. Both iOS 27.0. Parent owns simulator visual evidence. The parent's separate mesh-fixture simulators are not this lane's to alter.

First unsigned build passed mocks but actual startup Keychain failed. Fixed by explicit app/Keychain entitlements plus ad-hoc simulator signing in build script; a true Keychain add/read/update/delete test now passes. Xcode simulator entitlements are in the simulated-entitlement executable section, even though `codesign -d` prints an empty signature dictionary. Do not remove signing to make tests easier. Parent reinstalled the signed build and proved startup with no alert, white primary text, persistent form labels, and the disabled email action. No real OTP was sent.

Signed iPhone interim 13 tests passed; iPad 20 tests passed. Final suite adds exact-run/choice input answer test (21). Read final receipt in VERIFICATION.md after this lane completes. An initial final dispatch was refused because laptop load reached 5.06/core; queued retry uses the real admission gate without bypassing it.

## Mesh insertion points

`Services/OwnerHTTPControllerService.swift` owns HTTP routing and DTO checks. Inject `OwnerConnectionProvider.connection(for: HostDescriptor)` returning `OwnerConnection(endpoint: MeshEndpoint, transport: HTTPTransport)`. The provider MUST perform userspace-mesh dialing plus TLS authentication against the worker-bound SHA256(SPKI DER) and immutable owner UUID. It must never return plain unpinned URLSession for a self-signed owner.

`Domain/ControllerModels.swift`: MeshEndpoint `{url,tls_spki_sha256}`; HostDescriptor has immutable owner_node_id, protocol_version, role, availability, endpoints, capabilities. Optional registration_state/tls_spki_sha256 decode worker A1's interim unbound-host response. `controllerAvailable` requires host role, v1, online, nonempty endpoints.

Canonical owner DTO source is `crates/leanzero-link/src/controller_wire.rs`. WorkSnapshot is `{entry,pending_inputs,history_watermark}`; PendingInput `{input_id,run_id,prompt,options:[{option_id,label,accepts_value}]}`; HistoryPage `{work,store_generation,revision,items,next_page_token}`; HistoryItem `{message_id,revision,message}`. WorkCommand is flattened action/payload, with owner/work/generation/revision/run fencing. The service preserves catalog watermark across pages, rejects repeated tokens and mixed owners/generations, routes exactly one owner, and rejects a provider endpoint not present in that host's authenticated endpoint list.

`OwnerHTTPControllerService` is implemented for catalog, snapshot/history paging, resources, commands, receipt lookup. `TranscriptRenderer` exposes text, preserves stable message IDs/revisions, omits non-user-visible messages/private thinking, and labels unsupported attachments/tool detail. Full native attachment/tool-result rendering remains open. CPU is load-per-core, NOT percent; memory absent values remain unknown.

`App/ControllerStore` receives the OwnerControllerService in its initializer. `GooseRemoteApp` currently initializes default `UnavailableOwnerControllerService`; replace that composition with the verified mesh service only after its real proof. No owner endpoint or private mesh port is inferred by the app. No connection is configured by linking a framework alone.

`build.sh --framework-path /absolute/TailscaleKit.framework` adds an explicit framework search/link and embeds it; no framework is vendored. It must match the target platform/architecture. The parent has a pinned simulator framework under `/tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator/TailscaleKit.framework`; device framework/license/provenance remain release requirements.

## Auth and persistence

`ControllerAPI` implements worker OTP request-code/verify, refresh, hosts/devices/revoke over HTTPS; rejects redirects; bearer only in header, no Origin. Worker authority is explicitly entered today. Parent will supply a verified release default endpoint; ordinary users should not need to enter a server URL (keep custom authority advanced). Do not infer a production address or read existing credentials.

Keychain service is `net.leanzero.goose-remote.controller`, non-synchronizing, `WhenUnlockedThisDeviceOnly`. Device envelope saves authority + credentials + pending refresh request UUID atomically. After a lost refresh ACK, retry EXACT old token plus SAME request ID, including after relaunch; changing the ID revokes the refresh family server-side. SavedEnrollment retains original enrollment UUID/challenge; code is never persisted. Device-revoke retries retain request UUID and original expected device revision.

Local drafts and command recovery live in protected Application Support/GooseRemote/controller-state.json, excluded from backups. Missing file starts empty; corrupt file is an explicit failure disabling sends. Every command is saved before request; failure keeps its immutable ID/payload for receipt lookup. No blind retry, no assumed delivery. Successful receipt is validated against command/work. User can remove local access (warns server revoke is separate); revoking current device clears local access after actual server receipt. Pending remote jobs are never canceled by screen/app disappearance.

## Open integration/release work

Mesh auth/connection recovery and real network path; catalog/work streams/replay; actual host registry/run adapters; installed owner execution and exact input race proof; release default worker URL; remote model/provider/node management (explicitly unavailable currently); rich visible-message rendering; account deletion; icon; policy/privacy/support/review artifacts; physical device/signing/IPv6/cellular/lifecycle/accessibility proof. App Store submission was not authorized/performed by this lane.

Final gate receipt: `laptop-20261026-165814-c9cc` passed all 21 tests on iPhone, including real Keychain and final input-answer routing. Parent has confirmed signed startup, contrast/form labels, and iPad split view. The native lane is ready to release its agent slot; no production network flow is claimed proven.

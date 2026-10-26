# Development and verification

## Inspect the environment

Use read-only discovery before choosing commands or changing tools:

```sh
xcode-select -p
xcodebuild -version
xcodebuild -showsdks
xcrun simctl list devices available
```

Inspect the actual project/workspace and shared schemes with `xcodebuild -list -json -project <project>` or `-workspace <workspace>`. Use the selected project's supported schemes, SDKs and destination IDs; do not paste a simulator UUID from another machine. An installed SDK, the App Store upload SDK minimum, and the deployment target are three different things.

A simulator SDK is not a simulator runtime. If no usable devices exist, inspect `xcrun simctl list runtimes -j` and each runtime's `supportedDeviceTypes` before creating a device. A newer runtime can support only a newly introduced model; it may not cover iPad or older iPhones. Check free disk space before installation, use `xcodebuild -help` for the installed Xcode's `-downloadPlatform iOS`, `-buildVersion` and `-architectureVariant` options, and select a runtime that supports the requested device family. Create/boot the actual compatible types, then wait for `xcrun simctl bootstatus <udid> -b` and verify them. Do not infer a broken simulator or missing devices from sandbox XPC/CoreSimulator permission failures; repeat the narrowly scoped discovery with the required execution permission.

On Xcode 27, simulator windows live in [Device Hub](https://developer.apple.com/documentation/xcode/pairing-your-devices-with-your-mac), opened through Xcode > Open Developer Tool > Device Hub. Discover the installed UI app rather than assuming `Simulator.app` still exists. On the measured Xcode 27.1 installation it is `Xcode.app/Contents/Applications/DeviceHub.app`; `simctl` remains the command-line control surface. If an auto-created device fails with a stale runtime path after installation, first verify the mounted runtime and matching type, then try a fresh dedicated test device; do not delete the user's existing devices as a repair shortcut.

Do not switch the user's global Xcode selection unnecessarily. For a specific build, `DEVELOPER_DIR` can select an installed Xcode. Report missing SDKs, provisioning, device trust, or account access precisely. Do not imply a simulator build proves signing or physical-device installation.

## App structure

Keep the app target thin. Separate domain models/state transitions, transport/auth, local cache, and feature views. Prefer supported platform APIs and existing project dependencies. Add dependencies for demonstrated needs; pin versions and inspect licenses, privacy manifests, and binary provenance.

Use explicit state for loading, stale/offline, reconnecting, needs-authentication, awaiting-permission, partial success, unsupported version, and terminal failure. An empty state requires a successful authoritative response; a timeout or decode failure is not an empty list.

For streams, define stable IDs, ordering, replay cursor, restart epoch, snapshot handoff, deletion tombstones, and retention recovery. Subscribe/snapshot atomically or retain events across the snapshot watermark. Persist command IDs before sending and allow status lookup after connection loss. Do not advertise exactly-once remote side effects from request deduplication alone.

Keep network and persistence work off the main actor where appropriate. Use structured concurrency with clear task ownership. A task owned by a screen can end when the screen disappears; persistent work must have another owner. Native background scheduling is discretionary. A remote job continues on the server when the app suspends; recover its state on foreground.

## Design for iPhone and iPad separately

Use the product's identity, language, hierarchy and meaningful colors. Adapt interaction patterns to the platform rather than shrinking the desktop. A narrow iPhone generally uses stacked navigation; a wide iPad can use a sidebar/list/detail split. Collapse gracefully for narrow resizable windows. Test portrait, landscape, keyboard-visible layouts, safe areas, and multiwindow behavior if supported.

Use Dynamic Type and semantic accessibility labels, traits and actions. Verify VoiceOver reading/focus order and useful announcements without reading every streamed token. Support hardware keyboard focus/navigation and pointer use on iPad. Preserve text selection, copy, scroll position, composer drafts, and the user's place when new events arrive. Do not auto-scroll someone who is reading earlier history.

Follow Apple's [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/) and [accessibility guidance](https://developer.apple.com/design/human-interface-guidelines/accessibility). Touch targets, contrast, motion and text-size guidance are design criteria; do not present an arbitrary screenshot comparison as App Store certification.

Native controls are preferred where they fit. Customize tint, typography, spacing and assets consistently, while retaining platform semantics. Avoid unnecessary permissions, modal prompts, custom text editors, or reimplemented keyboard handling. Use a branded font only with redistribution rights and Dynamic Type support.

## Network and security

Apply [App Transport Security](https://developer.apple.com/documentation/security/preventing-insecure-network-connections) and valid server trust. A private encrypted overlay does not automatically settle URLSession's ATS policy, backend authentication, or review classification. Prove the real transport on a physical device; document any narrow exception and why it is necessary. Never install a trust-all URLSession delegate or enable unrestricted arbitrary loads as a shortcut.

Use [Keychain Services](https://developer.apple.com/documentation/security/keychain-services) with accessibility appropriate to the feature, normally device-bound credentials for device identity. Plan token refresh/revocation, locked-device behavior, logout and account switching. Do not assume deleting an app reliably removes all credentials. Avoid caching provider/API secrets unless the feature needs them; remote configuration should return masked status, not secret values.

Request local-network, camera, notifications or other permissions in context only when the chosen feature uses them. A local-network privacy description is not a substitute for verifying whether the actual network API/route triggers permission. Camera pairing should also have a manual path when feasible. An app-local networking library is not automatically a system VPN; if the product uses Network Extension or markets a VPN, review its entitlement and policy requirements separately.

## Build and test evidence

Typical commands, resolved to actual project values:

```sh
xcodebuild -project <project> -scheme <shared-scheme> -destination 'platform=iOS Simulator,id=<available-udid>' -derivedDataPath <build-dir> build
xcodebuild -project <project> -scheme <shared-scheme> -destination 'platform=iOS Simulator,id=<available-udid>' -resultBundlePath <new-result.xcresult> test
xcodebuild -project <project> -scheme <release-scheme> -destination 'generic/platform=iOS' -archivePath <archive.xcarchive> archive
```

Use `-workspace` instead when required. Preserve the actual exit code if piping logs. Inspect `.xcresult` failures, not only a last-line summary. Use the repository's existing formatting/lint rules rather than installing tools gratuitously.

Coverage should follow risk: domain ordering/idempotency tests; transport/auth integration; actual user journeys in UI tests; migration tests; physical-device network and lifecycle tests. For a controller, include Wi-Fi/cellular change, locked screen, long suspension beyond replay retention, killed/restarted host, duplicate send from two controllers, revoked device, and mixed client/server versions. Measure bytes, CPU, memory, battery/energy and UI latency under representative data; do not claim negligible overhead without a baseline and measurement.

Use [background-execution documentation](https://developer.apple.com/documentation/backgroundtasks) to choose legitimate mechanisms. Do not use audio, location or VoIP background modes as socket keepalive tricks. Background notifications and scheduled refresh are not guaranteed delivery. Persistent remote execution belongs on the remote host.

Evidence record: commit + app build, selected Xcode/SDK, deployment target, device/OS, backend versions, scenario and exact user action, observed fields, expected outcome, artifacts, and remaining gaps. Keep bulky recordings/build products in the project's designated working storage; credentials and customer content stay out of evidence.


## Simulator credentials and recovery proof (2026-10-26)

A simulator build with `CODE_SIGNING_ALLOWED=NO` can launch normally while real Keychain access fails. Build with supported simulator ad-hoc signing and the app identifier/keychain access-group entitlements, then run an actual device-bound Keychain read, create, update and delete against an isolated test service. A mocked credential store cannot prove entitlement correctness. On Xcode 27 the simulator may carry effective entitlements in its generated `*-Simulated.xcent` and Mach-O `__entitlements` section while `codesign -d` displays an empty signature entitlement dictionary; inspect the build products and actual Keychain result together. This is simulator qualification, not iOS distribution signing. Never fall back to plaintext or suppress the storage error.

For remote-command recovery, persist the command identity before sending and exercise process death after owner acceptance but before the controller journals the acknowledgement. Relaunch must query authoritative status before a new mutation, retain the same identity and prove one actual owner-side delivery. Keep missing acknowledgement evidence explicitly unproven even when reconciliation succeeds. Verify server request/delivery records independently of the UI receipt; a constant delivery-count response is not evidence. Test fresh stream subscriptions separately from explicit cursor replay, including a second connection to an already-used stream.


## Private-network certificate proof (2026-10-26)

Test custom trust on the actual non-loopback network address. A URLSession HTTPS/WebSocket test on localhost can pass with a pinned self-signed leaf while the same certificate is rejected by ATS on a private mesh address before the custom trust delegate can extend trust. Prove the combined routing, TLS and authentication stack; separate passing pieces do not establish the composition. The measured failure was ATS system-trust rejection (`-9802`) on the mesh IP, despite successful localhost positive/negative certificate controls.

Do not quietly broaden ATS to make this test pass. Investigate supported secure transport APIs and preserve explicit verification of the authenticated authority's SPKI pin, expected owner name, validity and server-authentication usage. Apple's [NWParameters privacy-context proxy configuration](https://developer.apple.com/documentation/network/nwparameters/privacycontext/proxyconfigurations) supports SOCKSv5; the installed SDK exposes this from iOS 17. [NWProtocolWebSocket](https://developer.apple.com/documentation/network/nwprotocolwebsocket) supplies framing and request headers, so do not hand-build WebSocket framing unnecessarily. Choosing these APIs is not proof: exercise actual mesh transport, wrong pin/name/expiry, stream close codes, reconnect and cancellation on supported OS versions. Record any failed path honestly while an alternative is still being implemented.


## Saved-network identity and retry proof (2026-10-26)

A transport fixture that requires its enrollment key on every restart cannot prove production saved-node recovery. Add an isolated instance of the real control server with disposable storage, one-use enrollment keys, loopback control endpoints and private relay infrastructure. Restart using only saved device state, assert the same public node identity, and exercise actual pinned HTTP and WebSocket traffic. A backend state of Running, a populated peer list or a successful discovery ping alone does not prove TCP traffic works.

Distinguish connection establishment from a sent application request. A transient proxy connection failure may be retried within the transport deadline before an HTTP command body is sent, retaining the same owner pin and route and respecting cancellation. Never retry a sent mutation merely because its acknowledgement is missing. Prove both directions with actual owner counters: first POST after saved-node restart is applied once; deliberately drop the acknowledgement after applying another POST and verify the transport does not send it again. Reconciliation belongs to the durable command identity protocol.

For Network.framework WebSockets, an immediate close can race the socket terminal error. Stop the connection-start state handler once ready; do not let it cancel an established socket before the reader drains a close frame. When a received close frame carries the authenticated owner's authorization code, preserve it even if the same receive reports a terminal transport error. Stress immediate close codes through a real TLS/WebSocket fixture; a pure error-code mapping test will not expose the race.


## Embedded networking privacy and build identity (2026-10-26)

Inspect the embedded networking SDK at its pinned source revision. A nil host-language logger or discarded console output does not prove that its background telemetry is disabled. Cross-language environment changes may miss a runtime's cached environment. Disable diagnostic collection/upload inside the embedded runtime before constructing its first node, then prove the actual constructor with a local capture server: a positive control must upload, the protected build must not, including pre-existing buffered logs. Do not send test telemetry to the vendor.

Require a rebuilt binary after source/privacy changes. A build target that depends only on a configuration file can leave a stale archive despite a successful make invocation. Force the archive build when needed; record the upstream revision and hashes of the final framework, privacy policy source and bundled manifest. Verify those inputs before embedding. Keep the framework's required-reason API declarations based on actual linked calls, separate from the app's collected-data declarations.

## Theme proof on the actual native screen (2026-10-26)

Test visible navigation titles as well as accessibility text. Forced SwiftUI navigation-bar background modifiers can cover a large title even while its accessibility label exists. Validate every theme on the actual iPhone and iPad layouts; preserve native navigation behavior before adding more styling overrides. Prove preference persistence by selecting a theme, terminating and relaunching the app, and verify System tracks an OS appearance change. Respect Reduce Motion and stop decorative animations when the scene is inactive.


## Consent, refusal and explicit navigation entry (2026-10-26)

For streaming remote work, permission binds the reviewed semantic payload, account/device, owner/work, store generation and run. Preserve those exact values across the human review interval; a rolling transcript revision is concurrency metadata, not the reviewed instruction. If only that revision advances, refresh it immediately before first dispatch after revalidating the same target/run/current input. A changed recipient configuration or run needs a new review. Verify this with a live frame during consent, a changed-run negative control and a changed composer draft; static snapshots missed the defect.

Distinguish a typed refusal proven to occur before server acceptance from an unknown/lost acknowledgement. A verified preaccept refusal may clear only its exact receiptless local outbox entry, atomically, and return to a new explicit user action. Do not synthesize an authoritative receipt or retry automatically. Keep uncertain delivery on the original command/status lookup; a later 404 alone never proves nonexecution. Trace the server's idempotency lookup, rejection and receipt INSERT order before defining the narrow refusal classifier. A local deletion failure must retain the unresolved row.

An explicit Connect action must open the sign-in root even if the Settings tab previously held a pushed details page. Own the navigation path and reset it on that explicit entry; changing tabs alone preserves the old page. Prove the real sequence (Settings detail -> another tab -> Connect -> sign-in), not only a routing variable. Ordinary tab switching may retain its path.


## Observed sharing destinations (2026-10-26)

A session disclosure is an observation, not an exhaustive list of recipients. Distinguish active runtime data from saved configuration and unavailable metadata; a provider driver label does not establish every downstream service or legal recipient. Routers and tools can reach additional destinations. Project only known labels and HTTP(S) scheme/host/port, never credentials, paths, query values, environment variables or command arguments. Missing destinations never mean local-only execution. Preserve honest unknown disclosure for older owners and unfamiliar provenance. Without an enforced configuration binding through actual dispatch, do not claim the snapshot guarantees future recipient stability. Prove the real response-to-sheet path and inspect the rendered qualification alongside the action being approved.

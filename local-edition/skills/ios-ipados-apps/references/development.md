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

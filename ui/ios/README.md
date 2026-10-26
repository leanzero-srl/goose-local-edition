# Goose Swarm Remote

Native universal iPhone/iPad controller, iOS 18.1+. SwiftUI, system typography, SF Symbols, and semantic Any/Dark colors matched to Goose Studio's `ui/desktop/src/styles/main.css`. Execution and files remain on the owning Mac. No compute, inference, or shell-job executor exists in this target.

## Build

Xcode and Python 3's standard library are the only source-build prerequisites. The checked-in `GooseRemote.xcodeproj` is generated deterministically by `scripts/generate-project.py`; run the generator after adding source files. No XcodeGen, CocoaPods, Swift package fetch, or hidden install step is needed.

Use the project `GooseRemote.xcodeproj` and shared `GooseRemote` scheme in Xcode. For a simulator build, choose an actual available destination from `xcrun simctl list devices available`:

```sh
ui/ios/scripts/build.sh --output /tmp/goose-remote-derived
ui/ios/scripts/build.sh --test --output /tmp/goose-remote-derived \
  --destination 'platform=iOS Simulator,id=<available-udid>'
```

On the owner's fleet these commands run through `node-run --host laptop --need-mem 4 -- <command>` after `node-probe`, because the laptop has full Xcode. Build products stay in the explicit output directory, outside git. Ad-hoc simulator signing and the checked-in app/Keychain entitlements are essential: an unsigned build compiled and passed in-memory tests but failed actual Keychain access at startup. The test suite now exercises real Keychain add/read/update/delete. Xcode stores simulator entitlements in its simulated-entitlement executable section; `codesign -d` alone can show an empty signature dictionary while the simulator entitlements are present.

An optional pinned mesh framework can be provided explicitly:

```sh
ui/ios/scripts/build.sh --output /tmp/goose-remote-derived \
  --framework-path /absolute/compatible/TailscaleKit.framework
```

This links and embeds the supplied binary. Production composition then enrolls its independent userspace node through the controller account authority. Choose a framework for the actual target architecture/platform. Never commit binaries or copy a simulator framework into a device archive. The pinned revision and local test commands are in `HANDOFF.md`. A physical/App Store build requires a valid Apple signing team/profile and device-compatible framework, configured in Xcode. The helper script is for simulator iteration, not distribution signing.

## Layers and security boundary

- `Domain/ControllerModels.swift`: controller v1 identities, canonical DTOs, action/run/revision checks, and explicit unknown resource data. Wire keys use snake_case; legacy visible message contents retain their existing camelCase contract.
- `Services/ControllerAPI.swift`: real HTTPS worker request-code, verify, rotating refresh, host/device discovery, and revocation. Redirects are refused. No legacy account token or node secret is requested.
- `Services/KeychainStore.swift`: `WhenUnlockedThisDeviceOnly`, nonsynchronizing credentials. Enrollment IDs and rotating refresh request IDs are saved before dispatch. A lost refresh response retries the same old token and request ID after app relaunch, then replaces both atomically on success.
- `Services/EmbeddedMesh.swift`, `NativeOwnerTransport.swift`, `OwnerTLS.swift`: independent userspace mesh node, canonical worker join/confirm flow, protected enrollment/state, explicit SOCKS transport, Network.framework TLS/WebSocket, mandatory worker-bound SPKI and immutable owner SAN/validity/EKU checks. Account-authority ATS remains unchanged; owner connections never fall back to unverified endpoints.
- `Services/OwnerHTTPControllerService.swift`, `OwnerReplay.swift`: catalog/history paging, signed change cursors, selected-work WebSocket replay, byte-offset partial answers, resource sampling, commands, and receipt lookup. Owner/generation/watermark and pagination progress are checked. Global revisions need not be contiguous. Live deletions/renames cannot be resurrected by older history pages. Unsupported visible content is labeled; private thinking and non-user-visible messages are omitted.
- `Services/OwnerControllerService.swift`: transport seam and protected local draft/command/catalog cache. Only the selected transcript is cached. Local data is excluded from backup. Commands are recorded before a request, and unknown delivery is looked up rather than blindly resubmitted.
- `App/ControllerStore.swift`: app-owned sync/refresh lifetime, per-owner outcomes, selected-work loading, draft persistence, receipts, input answers, resources, and device access. Closing a view does not cancel a remote run. A duplicate sync tap joins the app's current sync task.
- `Features/`: native tab navigation, iPad split view, per-kind session identity, explicit owner and sample age, persistent form labels, accessible semantic controls, and offline/unavailable/draft states.

The production entry point injects the real embedded mesh and owner service. Without its framework or configuration it reports an explicit error. Worker authentication targets an explicitly entered HTTPS authority. A release-configured default authority is still required for ordinary users; no production endpoint was guessed or contacted by this lane. Test resources are isolated from product startup. Combined native TLS/mesh proof requires `build.sh --test --mesh-fixture /absolute/compiled-fixture` and the matching framework; without the fixture that test explicitly skips.

## What remains before release

The current app is **not App Store ready**. Combined native mesh/pinned HTTPS/WSS has simulator proof, but real worker enrollment, resumed Headscale traffic after discarding the one-use key (identity restoration passed, the first dial failed), installed-Mac adapter journeys, and physical-device networking/lifecycle remain unproved. Active permission requests show complete arguments and explicit choices. Flat primitive JSON-schema forms support required fields, enums and bounds; unsupported schemas fail closed. Idle question/recipe preflight DTO integration is still pending. Message attachments/tool-result detail need native rendering; unsupported content currently says to view it on the Mac. Remote model downloads/provider configuration/node enrollment need real management APIs and qualification. Account deletion support, an App Store app icon, product privacy/support URLs, localization, distribution signing, privacy declaration review including embedded Go dependencies, accessibility/device qualification, review account/host, metadata, and release support material are also required before submission.

The privacy manifest's empty collected-data list is unreviewed foundation data, not a claim that no data is collected. Worker email/device/mesh records and embedded-library diagnostics need a complete declaration review. No server deploy, paid request, customer access, App Store submission, or production OTP was performed by this native lane.

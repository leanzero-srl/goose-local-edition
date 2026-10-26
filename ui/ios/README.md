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

This links and embeds the supplied binary; it does **not** configure a mesh connection. Choose a framework for the actual target architecture/platform. Never commit binaries or copy a simulator framework into a device archive. Parent transport work owns the framework provenance, license, pin validation, userspace mesh, and connection recovery. A physical/App Store build requires a valid Apple signing team/profile and device-compatible framework, configured in Xcode. The helper script is for simulator iteration, not distribution signing.

## Layers and security boundary

- `Domain/ControllerModels.swift`: controller v1 identities, canonical DTOs, action/run/revision checks, and explicit unknown resource data. Wire keys use snake_case; legacy visible message contents retain their existing camelCase contract.
- `Services/ControllerAPI.swift`: real HTTPS worker request-code, verify, rotating refresh, host/device discovery, and revocation. Redirects are refused. No legacy account token or node secret is requested.
- `Services/KeychainStore.swift`: `WhenUnlockedThisDeviceOnly`, nonsynchronizing credentials. Enrollment IDs and rotating refresh request IDs are saved before dispatch. A lost refresh response retries the same old token and request ID after app relaunch, then replaces both atomically on success.
- `Services/OwnerHTTPControllerService.swift`: actual catalog pagination, selected snapshot/history paging, resource sampling, commands, and receipt lookup. Requires an injected `OwnerConnectionProvider` that verifies the worker-bound TLS SPKI pin and immutable owner identity over mesh. It checks owner/generation/watermark and pagination progress, and never guesses endpoints. Unsupported visible content is labeled; private thinking and non-user-visible messages are omitted.
- `Services/OwnerControllerService.swift`: transport seam and protected local draft/command store. Local data is excluded from backup. Commands are recorded before a request, and unknown delivery is looked up rather than blindly resubmitted.
- `App/ControllerStore.swift`: app-owned sync/refresh lifetime, per-owner outcomes, selected-work loading, draft persistence, receipts, input answers, resources, and device access. Closing a view does not cancel a remote run. A duplicate sync tap joins the app's current sync task.
- `Features/`: native tab navigation, iPad split view, per-kind session identity, explicit owner and sample age, persistent form labels, accessible semantic controls, and offline/unavailable/draft states.

The production entry point currently injects `UnavailableOwnerControllerService`: it does not claim an operational mesh connection. Worker authentication can target an explicitly entered HTTPS authority. A release-configured default authority is still required for ordinary users; no production endpoint was guessed or contacted by this lane.

## What remains before release

The current app is a tested foundation, **not App Store ready**. The production mesh connection provider, catalog/work streams and replay, installed-Mac adapter journeys, and physical-device networking/lifecycle proof remain outstanding. Owner reads and commands have typed real HTTP implementations and tests, but those tests are not a production connection receipt. Message attachments/tool-result detail need native rendering; unsupported content currently says to view it on the Mac. Remote model downloads/provider configuration/node enrollment are explicitly unavailable and need real management APIs and qualification. Account deletion support, an App Store app icon, product privacy/support URLs, localization, distribution signing, privacy declaration review including embedded Go dependencies, accessibility/device qualification, review account/host, metadata, and release support material are also required before submission.

The versioned manifest declares the app source's current absence of tracking/analytics SDKs. It is not a substitute for reviewing the finished embedded framework and server data handling. No server deploy, paid request, customer access, App Store submission, or production OTP was performed for this app foundation.

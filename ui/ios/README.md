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

The production entry point injects the real embedded mesh and owner service. Without its framework or configuration it reports an explicit error. Worker authentication targets an explicitly entered HTTPS authority. The test iPad uses the deployed Link authority explicitly; the first-release default authority remains a release configuration decision. Test resources are isolated from product startup. Combined native TLS/mesh proof requires `build.sh --test --mesh-fixture /absolute/compiled-fixture` and the matching framework; without the fixture that test explicitly skips.

## Current qualification — 27 October 2026

The app is **not App Store ready**. The current source includes account deletion and recovery, model/provider management, typed pending-input forms, per-action sharing disclosure, theme support, an app icon, and policy/support links. The signed distribution package and its exact source/framework receipts are recorded in [release preparation](../../docs/ios-release/README.md). The app privacy manifest declares six collected-data categories; it is no longer the foundation's empty declaration. Those declarations still require reconciliation with deployed behavior and final App Store privacy answers.

The enrolled iPad retained its authentication and settings through an in-place update. Its actual Link connection is waiting for the account's legacy Mac hosts to finish their approved upgrades. Desktop-to-desktop history and command delivery have real two-Mac proof; phone-to-owner delivery is still unproved. Passing isolated mesh, replay, consent and management tests does not replace that journey. See the [current delivery ledger](../../docs/plans/link-controller-progress.md) and [host upgrade record](../../docs/plans/link-controller-host-upgrade.md).

Remaining release qualification includes the native connected-owner journeys (sessions, pending input, model/provider operations and resources), keyless reconnection and interruption recovery against the deployed service, physical-device networking/lifecycle/accessibility, supported OS coverage, final privacy/export declarations, reachable mobile-specific policy/support content, dedicated reviewer access, and real connected screenshots. Apple organization conversion is pending. No App Store record, upload or submission has been made. Historical lane handoffs describe their dated checkpoints; use the release record and [verification receipts](VERIFICATION.md) for subsequent evidence.

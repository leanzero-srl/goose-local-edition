# iPhone and iPad simulator feasibility proof

Measured 2026-10-26. Scope: simulator setup, dependency compilation, framework loading and unstarted C-handle lifecycle. This is not the Goose controller product or proof of Link transport.

## Result

Both dedicated devices booted and ran the [diagnostic](../../scripts/ios-controller-probe/README.md):

| Device | Runtime | Fresh runtime receipt |
| --- | --- | --- |
| Goose Controller iPhone, iPhone 17 | iOS 27.0 (24A434), arm64 | [iPhone](link-ios-simulator-proof/iphone-receipt.json), 13:39:18 UTC |
| Goose Controller iPad, iPad Pro 11-inch (M5) | iPadOS 27.0 (24A434), arm64 | [iPad](link-ios-simulator-proof/ipad-receipt.json), 13:39:19 UTC |

Each observed handle 2752513, successful close 0, duplicate-close rejection EBADF 9, fresh distinct handle 2752514, successful fresh close 0. The duplicate-close assertion is a negative control. Receipts record device family, OS, input framework hash and `linkNetworkingTested: false`. Device Hub's actual screens were inspected on both devices, including every measured value, runtime and saved-receipt state; the iPhone receipt section was checked by scrolling. The screens explicitly say “Link networking not tested.”

No controller was enrolled, no Link secret or provider credential was read, and no real session was started or nudged. No iOS signing account was required for these ad hoc simulator builds. Device provisioning/TestFlight signing remain separate work.

## Build provenance

Goose branch `codex/link-controller-plan`, starting source `e79a6f60eccecabc3a8dd8b3e4e2651568034572`; probe source is committed alongside this receipt. Xcode 27.1 (27A9275), iOS Simulator SDK 27.1, native Swift compiler, iOS 18.1 deployment minimum. The binary input SHA-256 is in each receipt; it identifies the separately compiled framework before the app helper ad hoc signs its embedded copy.

Public dependency: [tailscale/libtailscale at d1dc45d](https://github.com/tailscale/libtailscale/tree/d1dc45d039edb7759ecbee6ba2aea0c4bca884b8). Exact HEAD verified by `git ls-remote` and checkout HEAD. Go 1.27.1 was selected for this build without replacing the installed Go default. The dependency's locked module is tailscale.com v1.104.1. Neither dependency code nor Go caches are vendored into Goose.

Commands from the dependency checkout, each completed with exit 0:

```sh
GOTOOLCHAIN=go1.27.1 make libtailscale_ios_sim_arm64.a
cp libtailscale_ios_sim_arm64.a libtailscale_ios_sim.a
xcodebuild build -project swift/TailscaleKit.xcodeproj \
  -scheme 'TailscaleKit (Simulator)' -configuration Release \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /tmp/goose-libtailscale-ios-build \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO
```

The final probe build uses `xcrun --sdk iphonesimulator swiftc`, embeds the framework, and verifies ad hoc signatures. It completed without compiler warnings. The script's no-input/bad-input controls, Swift syntax parse and shell syntax check passed; an independent source review returned PASS for the limited proof, including no enrollment and truthful hash scope. Actual install/launch and receipts, rather than the build alone, establish execution on both simulator families.

## Setup findings retained for reuse

The machine had Xcode/SDKs but no simulator runtimes. Apple’s arm64 iOS 27.1 download was 7.85 GB and supported only iPhone Duo. Its boot succeeded, but it did not supply iPad support. The additional iOS 27.0 runtime was 8.05 GB and supplies the phone/tablet profiles used above. Free disk was checked before downloads; both runtimes remain installed. The initial Duo simulator is shut down.

After installation, auto-created standard devices reported `runtime path not found` despite runtime availability. Fresh dedicated devices using the verified runtime/type IDs booted successfully; the previous devices were not deleted. Dedicated IDs on this Mac are `B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9` (phone) and `F7078B46-CCF8-4779-9070-0BAFE33ADFB7` (tablet). Discover IDs again on another machine.

Xcode 27 uses [Device Hub](https://developer.apple.com/documentation/xcode/pairing-your-devices-with-your-mac) for simulated-device windows. This installation's UI is `/Applications/Xcode.app/Contents/Applications/DeviceHub.app`, bundle `com.apple.dt.Devices`; assuming `Simulator.app` exists led to an incorrect launch attempt. `simctl` still controls the runtimes. Restricted CoreSimulator IPC errors were resolved by scoped execution permission; they were not evidence of a broken installation.

## What this changes in the plan

The pinned framework compiles and loads on phone/tablet simulators. It declares iOS 18.1, rather than 18.0. Independent source review confirmed its `down()` calls `tailscale_up`, it caches its loopback address, and cancelling the Swift task does not by itself prove detached enrollment stopped. The [architecture](link-remote-control-plan.md#ios-and-ipados-transport) now reflects these constraints. Node recreation is a recovery candidate, not a measured fix.

Still unproved: HTTPS/WebSocket through the embedded proxy; isolated mesh authentication/negative controls; actual Link enrollment, discovery and scoped device access; remote session control and multi-controller idempotency; performance; physical-device suspension, cellular changes, battery and permission behavior. Upstream's `tstestcontrol` can support an isolated future mesh proof with its own DERP map; that would still not prove the production Link contract. No inference-pool eligibility or new session-sharing capability has been shipped.

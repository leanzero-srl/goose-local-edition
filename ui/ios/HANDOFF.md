# Native controller handoff — 2026-10-26

This lane owns `ui/ios/**` only. Production composition now uses `EmbeddedMeshConnectionProvider` + `OwnerHTTPControllerService`; it never substitutes demo data. iOS 18.1+, universal SwiftUI. Source projects are generated deterministically by `scripts/generate-project.py`. Execution stays on the original Mac.

## Native transport checkpoint

`EmbeddedMesh.swift` implements canonical controller `/mesh/join-key` then `/mesh/confirm`, Keychain enrollment recovery and protected independent native node state. Authority/device/enrollment IDs are bound before dispatch. Only confirmed mesh addresses and the worker's registered endpoint/pin are accepted. Credentials use headers, never URLs. Missing framework/configuration is explicit failure.

`NativeOwnerTransport.swift` uses Network.framework TLS, its built-in WebSocket protocol, and the embedded SOCKS proxy with failover disabled. HTTP uses CFHTTPMessage plus bounded framing validation. `OwnerTLS.swift` validates SHA256 of the original SPKI DER, exact immutable `owner-UUID.link` SAN, current validity, explicit serverAuth EKU, and Apple SSL policy with only that verified leaf anchored. A changed leaf with the same SPKI is allowed; another key/owner/expired/future/client-only certificate is rejected.

ATS is unchanged. URLSession custom trust passed localhost but failed on the actual mesh IP before its verifier. A proposed CIDR ATS exception was rejected by automatic approval review as an overly broad trust-boundary weakening. It was not applied. The safer Network.framework path passed the combined embedded-mesh certificate test.

Network.framework bypasses SOCKS for loopback even with failover false. Production input validation therefore only permits assigned Tailscale IPv4/IPv6 ranges when using mesh. The actual mesh fixture proves broken proxy and direct no-proxy paths cannot retrieve the owner nonce.

`OwnerReplay.swift` accepts global noncontiguous revisions, signed opaque cursors, byte-offset partials and committed replacements. History has its own fixed global watermark, not the catalog entry revision. Live deletion/rename tombstones survive late history pages. Streams cache metadata for all owners and one selected transcript, with explicit saved/stale display. Pending-input reload uses an independent task and fences selection identity. Close 4401 refreshes device access, 4403 removes revoked owner's cached content, 4413 triggers reconnect. Closing a screen/app never cancels remote execution.

Manual sync is an app-owned task across all hosts with durable cursors/pending host IDs, partial outcomes, and no remote sync job. Commands and drafts retain the protected write-before-send recovery; delivery uncertainty is resolved by exact receipt lookup.

## Framework and build

Pinned libtailscale commit: `d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`.
Simulator framework: `/tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator/TailscaleKit.framework`.
Device framework: `/tmp/goose-libtailscale-device-build/Build/Products/Release-iphoneos/TailscaleKit.framework` (parent built unsigned, not installed).

Do not call this revision's `down()`; it incorrectly calls `up()`. Close/discard the node. Cached loopback state means reconstruct the node after proxy death; reconstructing URLSession alone is insufficient. Canceling the Swift `up()` waiter does not cancel detached Go work; close the node to unblock it. Preserve/protect its state directory.

Dedicated simulator IDs: iPhone `B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9`; iPad `F7078B46-CCF8-4779-9070-0BAFE33ADFB7` (both iOS 27.0). Parent owns visual proof; do not bypass a locked Mac.

Build the disposable Go fixture once through node-dispatch from `ui/ios/tests/mesh-fixture`: `go test -c -o /tmp/goose-native-pinned-mesh.test .`. Use Go 1.27.1 as required by the pinned module. It starts only local testcontrol/DERP and an isolated tsnet owner; it does not read real Link credentials.

```
~/Projects/machine-sync/bin/node-run --host laptop --need-mem 4 --wait 60 --timeout 1200 --cwd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose -- 'ui/ios/scripts/build.sh --test --output /tmp/goose-remote-derived --framework-path /tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator/TailscaleKit.framework --mesh-fixture /tmp/goose-native-pinned-mesh.test --destination "platform=iOS Simulator,id=B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9"'
```

`--mesh-fixture` is required for combined proof; without it the embedded mesh test explicitly skips. Fixtures are test-bundle-only. `build.sh` starts/stops local servers and removes disposable runtime credentials from the test source resource directory. Test certificates use a harmless committed private key and production's 90-day leaf lifetime; `scripts/refresh-test-certificates.py` regenerates them with Python cryptography. Never copy fixtures into app resources.

Final iPhone gate `laptop-20261026-181338-2035`: 40 tests, 0 failures, no skips, including actual mesh and equal-timestamp history order. iPad `laptop-20261026-180818-e61b`: 39 tests, 0 failures, no skips before that final ordering regression. Independent review `/tmp/goose-controller-runtime-native-review.md` found self-canceling pending-input reload and stale-history resurrection; both repaired with passing regressions. No TLS/framing bypass was confirmed. Typed-input/runtime receipt defects are owned by the next integration lane.

## Explicit remaining qualification

The local testcontrol fixture requires an auth key on every registration, including a persisted node. Reconstruction proves stable PublicKey with the disposable fixture key. Production deliberately discards the one-use key after confirmation; the real Headscale result below supersedes the earlier identity-restoration gap but leaves resumed traffic unproven. Do not call this fixture a real worker-enrollment or physical-device proof.

Active permission/form payloads now match Rust commit `8b1e52fd8`. `PendingInputPresentation.swift` supports a flat object form with primitive string/number/integer/boolean properties, required fields, enums, numeric bounds and Unicode code-point string lengths. Unsupported keywords/nested schemas fail closed; generic text cannot bypass the command-builder validator. The view displays full permission request ID/tool/arguments before choices, without truncating. Signed/unsigned 64-bit JSON integers retain exact values. Tests cover required booleans without implicit false, enum/required failures, invalid JSON, exact numeric bounds and unsupported schemas. Root independently read the presentation/view and found no defect in the supported path; visible connected-owner UI still needs proof.

Typed-input iPhone job `laptop-20261026-182116-83e6`: 47 tests, 0 failures, no skips with testcontrol mesh. Real Headscale job `laptop-20261026-182200-b3d0`: 47 tests, 1 failure. First native node passed pinned HTTP/WSS. Reconstruction used no auth key, retained exactly the same PublicKey and reached Running, but its first SOCKS dial to owner timed out. Identity restoration passed; resumed traffic did not. Root now owns `tests/mesh-fixture/*` and this investigation. Swift test's optional `resumeWithoutKey` flag defaults false for testcontrol; true uses the key only on iteration 0. Run real fixture with `GOOSE_NATIVE_HEADSCALE_BINARY=/tmp/goose-headscale-proof/headscale` and `--mesh-fixture /tmp/goose-headscale-native-mesh.test`. Official local Headscale is v0.29.3, SHA256 `fb55b6d3d1ef3b850fe02837299ac1853f383a06f561d7f180d36b9ce406904b`.

The next owner-input patch will introduce idle questions/recipe preflight and optional current run IDs; its exact DTO is not applied here. A fresh native lane must integrate that frozen contract, preserve the source-run/current-run distinction, then add tests. Remote model/provider/node management and richer visible tool/attachment rendering remain separate work.

Release default authority, live OTP/worker enrollment, installed owner journeys, real revoked-device recovery, physical device/cellular/IPv6/lifecycle/accessibility, account deletion, app icon, distribution signing and review access remain open. PrivacyInfo's empty collected-data list is unreviewed foundation data, not a no-collection claim: worker email/device/mesh records and embedded-library diagnostics require review. Parent owns release docs. No production credentials, deploy, Apple signing or submission was performed by this lane.

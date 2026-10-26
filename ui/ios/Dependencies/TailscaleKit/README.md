# Embedded mesh privacy and build receipt

2026-10-26. Source is `tailscale/libtailscale` revision `d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`, using `tailscale.com v1.104.1` and Go 1.27.1. This folder contains our narrow build-source policy and its proof, not a vendored framework. Build products remain in the local working directories; no binary is committed.

## Build

Run through node-dispatch, one native build at a time, using the existing pinned checkout and cache:

```sh
~/Projects/machine-sync/bin/node-run --host laptop --need-mem 6 --wait 60 --timeout 1800 --cwd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose -- 'ui/ios/scripts/build-mesh-framework.sh /tmp/goose-libtailscale-ios-d1dc45d /tmp/goose-libtailscale-ios-build simulator'
~/Projects/machine-sync/bin/node-run --host laptop --need-mem 6 --wait 60 --timeout 1800 --cwd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose -- 'ui/ios/scripts/build-mesh-framework.sh /tmp/goose-libtailscale-ios-d1dc45d /tmp/goose-libtailscale-device-build device'
```

The script verifies the source pin and dependency version, adds `disableDiagnosticUploads()` as the first action in the C-exported `TsnetNewServer`, copies the reviewed helper/test, runs the focused proof, forces a fresh Go c-archive link, builds the selected arm64 framework and places the SDK manifest inside its bundle. Do not rely on the upstream Makefile's already-existing archive target: it has no source prerequisites and can reuse the pre-patch archive.

`goose-mesh-build.json` binds the input binary, policy source and privacy manifest hashes. The app's embedding script verifies that receipt before copying/signing the framework. This catches stale or accidentally modified build inputs; it is not an independent authenticity signature. App signing changes the embedded binary, so the receipt identifies the pre-signing build input, not the final signed bytes. Simulator build success does not prove device provisioning, a signed archive, or App Store acceptance.

## Why logger nil was insufficient

The Swift wrapper's `TailscaleNode.init` (`swift/TailscaleKit/TailscaleNode.swift:62-92`) does not call `tailscale_set_logfd` when its logger argument is nil. More importantly, the pinned `tsnet/tsnet.go:863-870` sends logs to `s.logtail` before its optional local `Logf`. `startLogger:1091-1127` creates file buffering and a logger whose default receiver is `log.tailscale.com`; `tsnet.go:241` explicitly documents automatic uploads. A custom Headscale control URL does not change this log receiver. `UserLogf` also defaults to local `log.Printf` (`tsnet.go:1143-1151`).

The helper uses two supported Go controls before any server is allocated: `envknob.SetNoLogsNoSupport()` sets the environment seen by Go, and `logtail.Disable()` closes the process-wide upload path. The first is observed by `logpolicy.TransportOptions.New:895`, which substitutes its no-network transport. The second is checked before upload (`logtail.go:532`) and before buffering (`logtail.go:768`), including a previously buffered queue. Setting the C environment in Swift after the Go runtime starts is not equivalent to setting the Go-visible knob.

The proof executes the actual constructor in a fresh protected subprocess. A separate control subprocess omits that call. Both use the real pinned `logtail.Logger` with a capturing HTTP transport and seeded older buffered data; no external endpoint is contacted. Measured control: 1 upload attempt, 3 new buffer writes. Protected: 0 upload attempts, 0 new buffer writes. The debug startup banner is explicitly enabled in both children, so initialization traffic is covered too. This proves the logtail boundary, not a general assertion that the networking library makes no connections.

## Required-reason API census

The device framework's actual undefined-symbol table contains `_stat`, `_fstat`, `_fstatat`, `_lstat`, and `_mach_absolute_time`. It has no matched direct imports for UserDefaults, active keyboard enumeration, or the listed disk-capacity calls in this census. Source routing establishes these reasons:

- `NSPrivacyAccessedAPICategoryFileTimestamp`, `C617.1`: configured tsnet state/log files are under the app's protected Application Support mesh directory (`GooseRemote/Services/EmbeddedMesh.swift:96-103`). `tsnet.go:857` checks that directory. `logtail/filch/filch.go:449-478` obtains metadata and orders the two app-owned buffer files. The Go `os` implementation reaches the Darwin stat-family calls. Linux-only host package checks such as `hostinfo.DisabledEtcAptSource` do not run on iOS.
- `NSPrivacyAccessedAPICategorySystemBootTime`, `35F9.1`: the embedded Go 1.27.1 runtime's `runtime/sys_darwin_arm64.s:145-147` calls `mach_absolute_time` for monotonic elapsed time and timers. Native `ContinuousClock` call sites are `NativeOwnerTransport` deadlines and `MeshPeerReadiness`; those calls use Apple's Swift runtime rather than a separate raw boot-time field. No boot-time payload is added by this app.

Apple's [required-reason API categories](https://developer.apple.com/documentation/bundleresources/app-privacy-configuration/nsprivacyaccessedapitypes/nsprivacyaccessedapitype) list the above calls and reasons. `C617.1` covers file metadata in app containers; `35F9.1` covers elapsed time and timers, with limits on transmitting derived information. Apple's [declaration instructions](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api) require a dynamic library's own bundle to carry its API declarations. Consequently the framework manifest is copied into `TailscaleKit.framework`, not only the app bundle. These are API-purpose declarations, not a blanket “no data collected” claim.

The app's new theme preference is a separate `@AppStorage`/UserDefaults use, owned by the app manifest (`CA92.1`). The Link authority still receives account email, controller/device IDs and registration metadata. Mesh control/relay traffic is required for connectivity; disabling logtail does not remove that traffic. The app-level collected-data declaration and public retention policy require the full account/backend inventory. Historical uploaded diagnostics, if any, are not deleted by this forward-looking change.

## Build and verification receipts

Simulator job `laptop-20261026-203444-7081`: focused Go proof passed; framework build succeeded; SHA-256 `e35b1884b0a43fdc5a4d53191dfc52f32918c2eb815c35ace8eefef97b977a77`.

Device job `laptop-20261026-203528-18f0`: same proof passed; framework build succeeded; SHA-256 `61a611339f7cb75599f7f58dcef27b18a72e878c21a2caae2931789ae7e8880a`.

Manifest SHA-256 `a9bc274530bbc22e684cb8bb6b2bc319f575699e9cedacd7abd205c3dce4e6a3`; helper SHA-256 `841ca217e8ad76253dc1cfbfeb8c04439fb50c862bc91f2358096f489d74b98b`. The embedding verifier passed both real outputs; negative controls rejected a missing receipt and a changed binary. Both outputs are unsigned working SDK builds. Final app embedding, signing and updated native UI tests belong to the final app build gate.

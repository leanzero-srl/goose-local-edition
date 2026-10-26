# Goose Link simulator probe

This is a diagnostic, not the controller app. It proves that an arm64 iOS
Simulator app can load the separately built TailscaleKit framework and call its
C handle lifecycle APIs. It does not prove Link networking, enrollment,
authentication, device signing, background behavior, or App Store readiness.

The app calls only `tailscale_new()` and `tailscale_close()`. It never creates a
Swift `TailscaleNode`, which would start networking. Each process launch measures
a positive first handle, successful close, duplicate close returning
`Darwin.EBADF`, a different positive fresh handle, and successful fresh close.
The UI shows every observed value and expectation. It saves the same measurements
with runtime OS, device family, date, and input framework binary SHA-256 to
`Documents/probe-receipt.json`. A failed check stays failed; a receipt write error
is visible separately. A receipt from an earlier launch is not evidence of a new
run: check `measuredAt` after each launch.

Build with Xcode selected and a completed arm64 simulator framework. The initial
dependency source was `tailscale/libtailscale` at
`d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`; the helper does not download, build, or
attest that source. Its binary hash lets the operator tie a receipt to the exact
input framework they built and verified.

```sh
bash scripts/ios-controller-probe/build.sh \
  /tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator/TailscaleKit.framework \
  /tmp/GooseLinkProbe.app
```

The output path must not exist. The helper uses `xcrun swiftc` with the selected
simulator SDK, an iOS 18.1 minimum, an embedded framework, and ad hoc simulator
signatures. It generates the plist for both iPhone and iPad; there is no Xcode
project, provisioning profile, real-device target, or signing account. Build
success is not a runtime pass.

Choose an available simulator UUID from `xcrun simctl list devices available`,
boot it if needed, and use that same UUID for every command below. Repeat on an
iPhone and an iPad simulator. These commands do not substitute a UUID or create
new simulator devices for you.

```sh
xcrun simctl install "$SIMULATOR_UDID" /tmp/GooseLinkProbe.app
xcrun simctl launch "$SIMULATOR_UDID" net.leanzero.goose.link-probe
container=$(xcrun simctl get_app_container "$SIMULATOR_UDID" net.leanzero.goose.link-probe data)
cat "$container/Documents/probe-receipt.json"
```

Inspect the visible results and the fresh JSON receipt on each simulator. A
missing receipt is a failed or incomplete probe, never a pass. The app deliberately
keeps “Link networking not tested” visible regardless of handle results.

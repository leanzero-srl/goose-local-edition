#!/bin/bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    printf 'Usage: %s /absolute/path/TailscaleKit.framework /absolute/path/GooseLinkProbe.app\n' "$0" >&2
    exit 64
fi

framework=$1
app=$2
if [[ "$framework" != /* || "$app" != /* || "$app" != *.app ]]; then
    printf 'Both paths must be absolute, and the output must end in .app.\n' >&2
    exit 64
fi
if [[ ! -f "$framework/TailscaleKit" || ! -f "$framework/Headers/TailscaleKit.h" ]]; then
    printf 'The input must be a built TailscaleKit.framework with its public header.\n' >&2
    exit 66
fi
if [[ -e "$app" ]]; then
    printf 'Output already exists; choose a new .app path: %s\n' "$app" >&2
    exit 73
fi

source_dir=$(cd -- "$(dirname -- "$0")" && pwd)
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path)
framework_sha256=$(shasum -a 256 "$framework/TailscaleKit")
framework_sha256=${framework_sha256%% *}
module_cache=$(mktemp -d /tmp/goose-ios-probe-modules.XXXXXX)
trap 'rm -rf "$module_cache"' EXIT
mkdir -p "$app/Frameworks"
/usr/bin/ditto "$framework" "$app/Frameworks/TailscaleKit.framework"

cat > "$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleIdentifier</key><string>net.leanzero.goose.link-probe</string>
    <key>CFBundleName</key><string>GooseLinkProbe</string>
    <key>CFBundleDisplayName</key><string>Goose Link Probe</string>
    <key>CFBundleExecutable</key><string>GooseLinkProbe</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.0.1</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleSupportedPlatforms</key><array><string>iPhoneSimulator</string></array>
    <key>MinimumOSVersion</key><string>18.1</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>UILaunchScreen</key><dict/>
    <key>UISupportedInterfaceOrientations</key><array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>ProbeFrameworkSHA256</key><string>$framework_sha256</string>
</dict></plist>
PLIST

xcrun --sdk iphonesimulator swiftc -parse-as-library -sdk "$sdk" \
    -target arm64-apple-ios18.1-simulator \
    -module-cache-path "$module_cache" \
    -F "$(dirname -- "$framework")" -framework TailscaleKit \
    -Xlinker -rpath -Xlinker '@executable_path/Frameworks' \
    "$source_dir/GooseLinkProbe.swift" -o "$app/GooseLinkProbe"
/usr/bin/plutil -lint "$app/Info.plist"
/usr/bin/codesign --force --sign - "$app/Frameworks/TailscaleKit.framework"
/usr/bin/codesign --force --sign - "$app"
/usr/bin/codesign --verify --deep --strict "$app"
printf 'Built simulator diagnostic: %s\n' "$app"

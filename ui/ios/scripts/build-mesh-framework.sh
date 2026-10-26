#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
source_dir="${1:?usage: build-mesh-framework.sh SOURCE_DIR DERIVED_DATA simulator|device}"
output="${2:?missing absolute DerivedData directory}"
platform="${3:?missing simulator or device}"
if [[ "$source_dir" != /* || "$output" != /* ]]; then echo 'Absolute source and build paths are required' >&2; exit 2; fi
case "$platform" in
  simulator) wrapper=clangwrap-ios-sim-arm.sh; archive=libtailscale_ios_sim_arm64.a; sdk=iphonesimulator; scheme='TailscaleKit (Simulator)'; destination='generic/platform=iOS Simulator';;
  device) wrapper=clangwrap-ios.sh; archive=libtailscale_ios.a; sdk=iphoneos; scheme='TailscaleKit (iOS)'; destination='generic/platform=iOS';;
  *) echo 'Expected simulator or device' >&2; exit 2;;
esac
python3 "$root/scripts/prepare-mesh-privacy.py" "$source_dir"
(
  cd "$source_dir"
  go test -run '^TestGooseDiagnosticUploadPolicy$' -count=1 -v .
  CGO_ENABLED=1 GOOS=ios GOARCH=arm64 CC="$source_dir/swift/script/$wrapper" \
    go build -ldflags=-w -tags ios -o "$archive" -buildmode=c-archive
  if [[ "$platform" == simulator ]]; then cp "$archive" libtailscale_ios_sim.a; fi
)
xcodebuild -project "$source_dir/swift/TailscaleKit.xcodeproj" -scheme "$scheme" -configuration Release \
  -sdk "$sdk" -destination "$destination" -derivedDataPath "$output" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  IPHONEOS_DEPLOYMENT_TARGET=18.1 CODE_SIGNING_ALLOWED=NO build
framework="$output/Build/Products/Release-$sdk/TailscaleKit.framework"
cp "$root/Dependencies/TailscaleKit/PrivacyInfo.xcprivacy" "$framework/PrivacyInfo.xcprivacy"
python3 - "$framework" "$source_dir" <<'PY'
import hashlib, json, subprocess, sys
from pathlib import Path
framework, source = map(Path, sys.argv[1:])
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
receipt = {"source_revision": subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip(),
           "binary_sha256": digest(framework / "TailscaleKit"), "privacy_sha256": digest(framework / "PrivacyInfo.xcprivacy"),
           "policy_sha256": digest(source / "goose_privacy.go"), "diagnostic_upload_policy": "disabled-before-node-construction"}
(framework / "goose-mesh-build.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps(receipt))
PY

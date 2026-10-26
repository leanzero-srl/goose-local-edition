#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
mode=build
output=""
destination="generic/platform=iOS Simulator"
framework=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --test) mode=test; shift;;
    --output) output="$2"; shift 2;;
    --destination) destination="$2"; shift 2;;
    --framework-path) framework="$2"; shift 2;;
    *) echo "Unknown option: $1" >&2; exit 2;;
  esac
done
if [[ -z "$output" || "$output" != /* ]]; then echo '--output requires an explicit absolute DerivedData path' >&2; exit 2; fi
args=(CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES)
if [[ -n "$framework" ]]; then
  if [[ "$framework" != /* || ! -f "$framework/TailscaleKit" ]]; then echo '--framework-path requires an absolute TailscaleKit.framework directory' >&2; exit 2; fi
  args+=("GOOSE_TAILSCALE_FRAMEWORK_DIR=$(dirname "$framework")" 'GOOSE_MESH_LDFLAGS=-framework TailscaleKit')
fi
python3 "$root/scripts/generate-project.py"
exec xcodebuild -project "$root/GooseRemote.xcodeproj" -scheme GooseRemote -configuration Debug -destination "$destination" -derivedDataPath "$output" "${args[@]}" "$mode"

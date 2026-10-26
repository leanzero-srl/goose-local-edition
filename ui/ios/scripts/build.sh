#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
mode=build
output=""
destination="generic/platform=iOS Simulator"
framework=""
mesh_fixture=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --test) mode=test; shift;;
    --output) output="$2"; shift 2;;
    --destination) destination="$2"; shift 2;;
    --framework-path) framework="$2"; shift 2;;
    --mesh-fixture) mesh_fixture="$2"; shift 2;;
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
if [[ "$mode" == test ]]; then
  args+=(-collect-test-diagnostics never)
  mkdir -p "$output"
  python3 "$root/scripts/tls-fixture.py" > "$output/tls-fixture.log" 2>&1 &
  fixture_pid=$!
  mesh_pid=""
  cleanup() {
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
    if [[ -n "$mesh_pid" ]]; then kill "$mesh_pid" 2>/dev/null || true; wait "$mesh_pid" 2>/dev/null || true; fi
    rm -f "$root/GooseRemoteTests/Fixtures/mesh-runtime.json"
  }
  trap cleanup EXIT
  for attempt in {1..50}; do
    if grep -q READY "$output/tls-fixture.log"; then break; fi
    if ! kill -0 "$fixture_pid" 2>/dev/null; then cat "$output/tls-fixture.log" >&2; exit 1; fi
    sleep 0.1
  done
  if ! grep -q READY "$output/tls-fixture.log"; then echo 'TLS fixture did not start' >&2; exit 1; fi
  if [[ -n "$mesh_fixture" ]]; then
    if [[ "$mesh_fixture" != /* || ! -x "$mesh_fixture" ]]; then echo '--mesh-fixture requires a compiled absolute fixture path' >&2; exit 2; fi
    mesh_run="$output/mesh-run-$(uuidgen)"
    env GOOSE_NATIVE_MESH_OUT="$mesh_run" GOOSE_NATIVE_TLS_FIXTURES="$root/GooseRemoteTests/Fixtures" \
      "$mesh_fixture" -test.run '^TestPinnedNativeMesh$' -test.v -test.timeout=0 > "$output/mesh-fixture.log" 2>&1 &
    mesh_pid=$!
    for attempt in {1..300}; do
      if [[ -f "$mesh_run/mesh-runtime.json" ]]; then break; fi
      if ! kill -0 "$mesh_pid" 2>/dev/null; then cat "$output/mesh-fixture.log" >&2; exit 1; fi
      sleep 0.1
    done
    if [[ ! -f "$mesh_run/mesh-runtime.json" ]]; then echo 'Mesh fixture did not start' >&2; exit 1; fi
    cp "$mesh_run/mesh-runtime.json" "$root/GooseRemoteTests/Fixtures/mesh-runtime.json"
  else
    rm -f "$root/GooseRemoteTests/Fixtures/mesh-runtime.json"
  fi
fi
xcodebuild -project "$root/GooseRemote.xcodeproj" -scheme GooseRemote -configuration Debug -destination "$destination" -derivedDataPath "$output" "${args[@]}" "$mode"

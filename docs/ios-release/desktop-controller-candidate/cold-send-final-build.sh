#!/bin/bash
set -euo pipefail
: "${SOURCE_COMMIT:?exact committed source required}"
: "${SOURCE_TREE:?exact committed tree required}"
cd /tmp/goose-cold-send-ae34c7c3a7
source bin/activate-hermit
export PATH="/usr/bin:/bin:/opt/homebrew/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/Projects/goose-controller-test-build/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export OPENSSL_DIR=/opt/homebrew/opt/openssl@3 OPENSSL_ROOT_DIR=/opt/homebrew/opt/openssl@3 PKG_CONFIG_PATH=/opt/homebrew/opt/openssl@3/lib/pkgconfig
export GOOSE_BUILD_SHA="$SOURCE_COMMIT" GOOSE_BUILD_VERSION=3.0.134
ulimit -n 4096
python3 /tmp/goose-cold-send-verify-runtime.py
cargo build --locked --config 'profile.dev.package."*".strip=false' -p goose-cli --bin goose
stage="/tmp/goose-cold-send-cli-${SOURCE_COMMIT:0:12}"
[[ ! -e "$stage" ]]
mkdir -m700 "$stage"
install -m755 "$CARGO_TARGET_DIR/debug/goose" "$stage/goose-gated"
printf '%s\n' "$SOURCE_COMMIT" > "$stage/source-commit.txt"
printf '%s\n' "$SOURCE_TREE" > "$stage/source-tree.txt"
shasum -a256 "$stage/goose-gated" > "$stage/input-cli-sha256.txt"
"$stage/goose-gated" --version
python3 - <<'VERIFY'
import os,pathlib
p=pathlib.Path('/tmp/goose-cold-send-cli-'+os.environ['SOURCE_COMMIT'][:12])/'goose-gated'
assert os.environ['SOURCE_COMMIT'].encode() in p.read_bytes()
print('COMMITTED_BUILD_STAMP_PASS')
VERIFY

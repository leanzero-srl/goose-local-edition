#!/bin/zsh
# wincheck.sh [ref] — does <ref> (default HEAD) compile for Windows? Run before pushing any merge that touches
# a Rust crate CI builds on Windows. 2026-09-26: two merges went red on "Build Rust Project on Windows"
# (goose_sidecar::placement unguarded; sysinfo's Windows SID used as a uid) only after they were pushed.
# The repo's .cargo/config.toml carries MSVC link-args with a space, which cargo-xwin refuses, so the check
# runs in a throwaway worktree without it (check never links) and its own target dir.
set -eu
ref=${1:-HEAD}; repo=/Users/mihaiperdum/Projects/goose
W=$(mktemp -d /tmp/wincheck.XXXXXX)
git -C $repo worktree add -q --detach $W $ref
trap "git -C $repo worktree remove --force $W; rm -rf $repo/target-win" EXIT
rm -f $W/.cargo/config.toml
source $repo/bin/activate-hermit >/dev/null 2>&1
cd $W && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR=$repo/target-win \
  cargo xwin check -q -p goose -p goose-sidecar -p leanzero-link --target x86_64-pc-windows-msvc 2>&1 | grep -E "^error" -A6 || true
echo "windows check exit ${pipestatus[1]}"

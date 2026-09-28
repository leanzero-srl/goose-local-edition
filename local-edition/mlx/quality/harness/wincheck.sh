#!/bin/zsh
# wincheck.sh [ref] — does <ref> (default HEAD) compile for Windows? Run before pushing any merge that touches
# a Rust crate CI builds on Windows. 2026-09-26: two merges went red on "Build Rust Project on Windows"
# (goose_sidecar::placement unguarded; sysinfo's Windows SID used as a uid) only after they were pushed.
# 2026-09-28: S8 (goose-cli swarm_engine.rs) used goose_sidecar::holders unguarded and this check stayed green —
# it never compiled goose-cli. It now checks goose-cli and goose-server too.
# The repo's .cargo/config.toml carries MSVC link-args with a space, which cargo-xwin refuses, so the check
# runs in a throwaway worktree without it (check never links) and its own target dir.
# 2026-09-28: two runs at once (an agent's and the coordinator's) shared target-win and each run's exit rm -rf'd
# it under the other. Runs now SERIALIZE on a mkdir lock (atomic); a lock whose owner pid is dead is taken over.
set -eu
ref=${1:-HEAD}; repo=/Users/mihaiperdum/Projects/goose
LOCK=$repo/.wincheck.lock
until mkdir $LOCK 2>/dev/null; do
  owner=$(cat $LOCK/pid 2>/dev/null || true)
  if [ -n "$owner" ] && ! kill -0 $owner 2>/dev/null; then rm -rf $LOCK; continue; fi
  sleep 5
done
echo $$ > $LOCK/pid
W=$(mktemp -d /tmp/wincheck.XXXXXX)
git -C $repo worktree add -q --detach $W $ref
trap "git -C $repo worktree remove --force $W; rm -rf $repo/target-win $LOCK" EXIT
rm -f $W/.cargo/config.toml
source $repo/bin/activate-hermit >/dev/null 2>&1
# goose-cli/goose-server pull aws-lc-sys for the HOST build scripts; under `cargo xwin check` it inherits
# TARGET_CC=clang-cl and fails on -isysroot (2026-09-28). So: xwin's env for the MSVC target, minus TARGET_CC/CXX/AR,
# then a plain cargo check.
cd $W || exit 2
eval "$(cargo xwin env --target x86_64-pc-windows-msvc)" || { echo "windows check: cargo xwin env failed"; exit 2; }
unset TARGET_CC TARGET_CXX TARGET_AR
# The exit code is cargo's own, read from its status — `a | b || true` made every run without an ^error line
# read as 0 (2026-09-28), which would have hidden an env failure.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR=$repo/target-win \
  cargo check -q -p goose -p goose-sidecar -p leanzero-link -p goose-cli -p goose-server --target x86_64-pc-windows-msvc > $W.log 2>&1 && code=0 || code=$?  # set -e must not end the script here
grep -E "^error" -A6 $W.log | head -60 || true
echo "windows check exit $code"
rm -f $W.log

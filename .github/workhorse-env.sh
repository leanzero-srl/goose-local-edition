#!/bin/bash
# Environment for a CI job on the self-hosted workhorse runner (see .github/workflows/ci.yml).
# $1 = the job id; each job keeps its own persistent cargo target dir.
set -euo pipefail
job="${1:?job id}"

# Pinned to the runner account's real toolchain, so a step that moves HOME still finds cargo.
echo "CARGO_HOME=$HOME/.cargo" >> "$GITHUB_ENV"
echo "RUSTUP_HOME=$HOME/.rustup" >> "$GITHUB_ENV"
echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
# cmake (llama-cpp-sys-2 builds llama.cpp with it) lives in Homebrew.
echo "/opt/homebrew/bin" >> "$GITHUB_PATH"

# Persistent target dir per job. cargo never garbage-collects target/, so a dir past 60 GB is
# started over (an exact path under ~/ci-tools, never a glob).
target="$HOME/ci-tools/goose-target/$job"
if [ -d "$target" ] && [ "$(du -sk "$target" | cut -f1)" -gt $((60 * 1024 * 1024)) ]; then
  echo "target dir $target is past 60 GB; starting it over"
  rm -rf "$target"
fi
mkdir -p "$target"
echo "CARGO_TARGET_DIR=$target" >> "$GITHUB_ENV"

# A fresh HOME for steps that RUN goose code: the workhorse also runs the Goose app, and a test
# outside goose-test-support's hermetic root must not read or write its config, data or sessions.
home="$RUNNER_TEMP/isolated-home"
rm -rf "$home"
mkdir -p "$home"
echo "ISOLATED_HOME=$home" >> "$GITHUB_ENV"
echo "GOOSE_DISABLE_KEYRING=1" >> "$GITHUB_ENV"

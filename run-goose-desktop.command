#!/usr/bin/env bash
#
# run-goose-desktop.command — one double-click to run Goose (Local Edition) desktop on macOS.
#
# Double-click this file in Finder (or run it in a terminal). It sets up the whole toolchain for you —
# Node 24 (nvm), pnpm 10.30 (corepack), and the hermit `just`/cargo environment — then rebuilds the
# backend only if the Rust source changed, and launches the desktop. No manual env dance.
#
#   Double-click / no args   Smart run: rebuild the goose backend only if crates/ changed, then launch.
#   --fast                   Skip the staleness check and launch immediately (fastest; may be stale).
#   --build                  Force a full backend rebuild before launching.
#   --package                Build a standalone, double-clickable Goose.app (then open it). Slowest.
#
set -euo pipefail

# Always operate from the repo root (Finder double-click starts in $HOME).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

MODE="smart"
case "${1:-}" in
  --fast)    MODE="fast" ;;
  --build)   MODE="build" ;;
  --package) MODE="package" ;;
  "")        MODE="smart" ;;
  *) echo "Unknown option: $1"; echo "Use: --fast | --build | --package"; exit 2 ;;
esac

step() { printf '\n\033[1;34m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[1;32m✓ %s\033[0m\n' "$*"; }
die()  { printf '\n\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

# --- Node 24 via nvm ------------------------------------------------------------------------------
step "Setting up Node 24"
if [ -s "$HOME/.nvm/nvm.sh" ]; then
  # shellcheck disable=SC1091
  \. "$HOME/.nvm/nvm.sh"
  nvm use 24 >/dev/null 2>&1 || nvm install 24 >/dev/null 2>&1 || true
fi
command -v node >/dev/null 2>&1 || die "Node not found. Install nvm + Node 24 (https://github.com/nvm-sh/nvm)."
NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
[ "$NODE_MAJOR" -ge 24 ] || die "Node $NODE_MAJOR found, but Goose desktop needs Node 24+. Run: nvm install 24"
ok "node $(node -v)"

# --- pnpm 10.30 via corepack (11 breaks workspace hoisting) ---------------------------------------
step "Setting up pnpm 10.30"
command -v corepack >/dev/null 2>&1 || die "corepack not found (ships with Node). Try: npm i -g corepack"
corepack prepare pnpm@10.30.0 --activate >/dev/null 2>&1 || true
ok "pnpm $(corepack pnpm --version 2>/dev/null || echo '?')"

# --- hermit (just + cargo) --------------------------------------------------------------------------
step "Activating build environment (just, cargo)"
# shellcheck disable=SC1091
source bin/activate-hermit >/dev/null 2>&1 || die "Could not activate hermit (bin/activate-hermit)."
command -v just >/dev/null 2>&1 || die "just not found after hermit activation."
ok "just $(just --version | awk '{print $2}')"

BUNDLED_BIN="ui/desktop/src/bin/goose"

needs_rebuild() {
  [ ! -f "$BUNDLED_BIN" ] && return 0
  # Rebuild if any Rust source is newer than the bundled backend binary.
  [ -n "$(find crates -name '*.rs' -newer "$BUNDLED_BIN" -print -quit 2>/dev/null)" ] && return 0
  return 1
}

build_backend() {
  step "Building the goose backend (this includes the Swarm provider)"
  just release-binary
  ok "backend built + bundled → $BUNDLED_BIN"
}

case "$MODE" in
  package)
    build_backend
    step "Packaging standalone Goose.app (double-clickable, self-contained)"
    just package-ui
    APP="$(ls -d "$REPO_ROOT"/ui/desktop/out/*/Goose.app 2>/dev/null | head -1)"
    [ -n "$APP" ] || die "package-ui finished but no Goose.app was produced."
    ok "built: $APP"
    printf '\n\033[1;34m▸ To install: drag Goose.app into /Applications. Opening it now…\033[0m\n'
    open "$APP"
    exit 0
    ;;
  build)  build_backend ;;
  smart)  if needs_rebuild; then build_backend; else ok "backend up to date (no crates/ changes) — skipping rebuild"; fi ;;
  fast)   ok "fast mode — skipping backend rebuild" ;;
esac

step "Launching Goose desktop"
echo "  (leave this window open; close it or press Ctrl-C to quit the app)"
exec just run-ui

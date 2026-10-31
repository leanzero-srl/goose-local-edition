#!/usr/bin/env bash
# Populate ui/desktop/src/bin with the tailscaled + tailscale binaries LeanZero Link's
# embedded mesh drives, so the packaged desktop app carries its OWN userspace tailscaled
# and joins a swarm on a machine with no system Tailscale install. The binaries ship via
# forge.config.ts `extraResource: ['src/bin', ...]` (same path as the goosed binary), and
# gooseServe.ts points LEANZERO_TAILSCALED / LEANZERO_TAILSCALE_CLI at them when present.
#
# Two sourcing strategies, in order:
#   1. `go build` the pinned tailscale.com version (in a throwaway module) — reproducible, cross-compiles to any
#      GOOS/GOARCH (set TS_GOOS / TS_GOARCH to cross-build; defaults to the host).
#   2. No Go: copy from a system Tailscale install for the HOST platform (dev bootstrap).
# Fails loud if neither can produce a working binary.
set -euo pipefail

TAILSCALE_VERSION="${TAILSCALE_VERSION:-1.98.5}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$(cd "$HERE/../src/bin" && pwd)"

host_ext=""
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) host_ext=".exe" ;;
esac
TAILSCALED="$BIN_DIR/tailscaled${host_ext}"
TAILSCALE="$BIN_DIR/tailscale${host_ext}"

verify () {  # a binary that does not run is worse than an absent one (silent SIGKILL)
  local b="$1"
  [[ -x "$b" ]] || { echo "fetch-tailscale: $b is not executable"; return 1; }
  "$b" --version >/dev/null 2>&1 || { echo "fetch-tailscale: $b does not EXECUTE"; return 1; }
}

build_with_go () {
  command -v go >/dev/null 2>&1 || return 1
  echo "fetch-tailscale: building tailscaled+tailscale @v${TAILSCALE_VERSION} with go ($(go version | awk '{print $3}'))"
  local goos="${TS_GOOS:-}" goarch="${TS_GOARCH:-}"
  [[ -n "$goos" ]] && export GOOS="$goos"
  [[ -n "$goarch" ]] && export GOARCH="$goarch"
  # `go build pkg@version` is refused outside module mode, so build inside a throwaway module,
  # with the exact toolchain tailscale's go.mod names (1.98.5 needs go1.26.3; go1.27 fails to
  # compile its go-json-experiment dependency; go downloads the named toolchain itself). The -X stamps are the
  # ones Homebrew's formula sets, so `tailscaled --version` reads 1.98.5-t<commit> like the
  # bundles before 3.0.139 (without them it reads 1.98.5-ERR-BuildInfo).
  local mod commit stamps
  mod="$(mktemp -d)"
  (
    cd "$mod"
    export GOFLAGS="" GOBIN=""
    gomod="$(go mod download -json "tailscale.com@v${TAILSCALE_VERSION}" | sed -n 's/.*"GoMod": "\(.*\)".*/\1/p' | head -1)"
    gover="$(awk '$1=="go"{print $2; exit}' "$gomod")"
    [[ -n "$gover" ]] || { echo "fetch-tailscale: no go version in tailscale.com@v${TAILSCALE_VERSION} go.mod" >&2; exit 1; }
    export GOTOOLCHAIN="${GOTOOLCHAIN:-go${gover}}"
    go mod init fetch-tailscale >/dev/null 2>&1 || exit 1
    go mod edit -go="$gover" || exit 1
    go get "tailscale.com/cmd/tailscaled@v${TAILSCALE_VERSION}" "tailscale.com/cmd/tailscale@v${TAILSCALE_VERSION}" || exit 1
    commit="$(go list -m -json "tailscale.com@v${TAILSCALE_VERSION}" | sed -n 's/.*"Hash": "\([0-9a-f]*\)".*/\1/p' | head -1)"
    [[ -n "$commit" ]] || { echo "fetch-tailscale: no commit for tailscale.com@v${TAILSCALE_VERSION}" >&2; exit 1; }
    stamps="-X tailscale.com/version.longStamp=${TAILSCALE_VERSION}-t${commit:0:9} -X tailscale.com/version.shortStamp=${TAILSCALE_VERSION} -X tailscale.com/version.gitCommitStamp=${commit}"
    go build -ldflags "$stamps" -o "$TAILSCALED" tailscale.com/cmd/tailscaled || exit 1
    go build -ldflags "$stamps" -o "$TAILSCALE" tailscale.com/cmd/tailscale || exit 1
  )
  local rc=$?
  rm -rf "$mod"
  return $rc
}

copy_from_system () {  # host platform only
  local sd sc
  sd="$(command -v tailscaled || true)"
  if [[ -z "$sd" ]]; then
    for c in /opt/homebrew/bin/tailscaled /usr/local/bin/tailscaled /opt/homebrew/Cellar/tailscale/*/bin/tailscaled /usr/sbin/tailscaled; do
      [[ -x "$c" ]] && { sd="$c"; break; }
    done
  fi
  sc="$(command -v tailscale || true)"
  if [[ -z "$sc" ]]; then
    for c in /opt/homebrew/bin/tailscale /usr/local/bin/tailscale /opt/homebrew/Cellar/tailscale/*/bin/tailscale; do
      [[ -x "$c" ]] && { sc="$c"; break; }
    done
  fi
  [[ -n "$sd" && -n "$sc" ]] || return 1
  echo "fetch-tailscale: copying from system install: $sd / $sc"
  cp -f "$sd" "$TAILSCALED"
  cp -f "$sc" "$TAILSCALE"
  # System binaries are often read-only; make them writable+executable so a later
  # codesign --force (release build) can replace their signature.
  chmod u+wx "$TAILSCALED" "$TAILSCALE"
}

mkdir -p "$BIN_DIR"
if build_with_go; then
  :
elif copy_from_system; then
  echo "fetch-tailscale: NOTE — copied host binaries (no Go). For a cross-platform/reproducible bundle install Go and re-run."
else
  echo "fetch-tailscale: FAILED — no Go toolchain and no system tailscale install found." >&2
  echo "  Install Go (then this builds v${TAILSCALE_VERSION} for any target) or install tailscale locally." >&2
  exit 1
fi

verify "$TAILSCALED"
verify "$TAILSCALE"
echo "fetch-tailscale: OK"
ls -la "$TAILSCALED" "$TAILSCALE"

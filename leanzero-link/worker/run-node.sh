#!/usr/bin/env bash
# Start the LeanZero Link auth worker as a self-hosted Node service (loopback only).
# Secrets come from an env file — this script reads NONE itself and the repo holds NONE.
# Publish the bound port with:  tailscale funnel "${PORT:-8791}"
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
env_file="${LEANZERO_LINK_ENV:-$HOME/.leanzero/link-worker.env}"

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$env_file"
  set +a
  echo "run-node: loaded env from $env_file" >&2
else
  echo "run-node: env file $env_file not found; starting with the current process env only" >&2
fi

cd "$here"
exec npx tsx src/node-server.ts

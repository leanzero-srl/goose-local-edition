#!/bin/bash
set -euo pipefail
cd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose
source bin/activate-hermit
[[ $(node --version) == v24.* ]]
export NODE_OPTIONS=--max-old-space-size=4096
cd ui/sdk
pnpm run generate
pnpm run build:ts
pnpm run typecheck:test
pnpm test
REPO=/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose STAGE=/tmp SOURCE_COMMIT=pending python3 /tmp/goose-node-management-package.py record-sdk-build
printf 'NODE_MANAGEMENT_SDK_GATES_PASS\n'

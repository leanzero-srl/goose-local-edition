#!/bin/bash
set -euo pipefail
: "${SOURCE_COMMIT:?committed and tested source required}"
: "${CLI:?absolute stamped CLI path required}"
: "${CLI_SHA256:?gated CLI hash required}"
: "${SDK_MANIFEST:?gated SDK distribution manifest required}"
export REPO=/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose
export STAGE=/tmp/goose-node-management-desktop
export BASELINE_APP='/tmp/goose-controller-incremental-43a8589d652f/Goose Swarm.app'
export SOURCE_COMMIT CLI CLI_SHA256 SDK_MANIFEST
[[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]
[[ "$CLI_SHA256" =~ ^[0-9a-f]{64}$ ]]
[[ ! -e "$STAGE" ]]
[[ $(df -k /System/Volumes/Data | awk 'NR==2{print $4}') -ge 20971520 ]]
git -C "$REPO" cat-file -e "$SOURCE_COMMIT^{commit}"
codesign --verify --deep --strict "$BASELINE_APP"
mkdir -m700 "$STAGE"
mkdir "$STAGE/source"
git -C "$REPO" archive "$SOURCE_COMMIT" | tar -x -C "$STAGE/source"
python3 /tmp/goose-node-management-package.py source
python3 /tmp/goose-node-management-package.py cli
python3 /tmp/goose-node-management-package.py baseline
python3 /tmp/goose-node-management-package.py payload
# APFS clones reuse dependency blocks without allowing package hooks to mutate the working tree.
cp -cRp "$REPO/ui/node_modules" "$STAGE/source/ui/node_modules"
mkdir -p "$STAGE/source/ui/desktop/node_modules/@aaif" "$STAGE/source/ui/sdk/node_modules"
ln -s ../../../sdk "$STAGE/source/ui/desktop/node_modules/@aaif/goose-sdk"
for dependency in @modelcontextprotocol @radix-ui; do
 ditto "$REPO/ui/desktop/node_modules/$dependency" "$STAGE/source/ui/desktop/node_modules/$dependency"
done
for dependency in @aaif @types undici-types; do
 ditto "$REPO/ui/sdk/node_modules/$dependency" "$STAGE/source/ui/sdk/node_modules/$dependency"
done
ditto "$REPO/ui/sdk/dist" "$STAGE/source/ui/sdk/dist"
python3 /tmp/goose-node-management-package.py sdk
ditto "$BASELINE_APP/Contents/Resources/bundled-mcps" "$STAGE/source/ui/desktop/bundled-mcps"
python3 /tmp/goose-node-management-package.py pins
for name in tailscale tailscaled; do
 install -m755 "$BASELINE_APP/Contents/Resources/bin/$name" "$STAGE/source/ui/desktop/src/bin/$name"
done
install -m755 "$CLI" "$STAGE/source/ui/desktop/src/bin/goose"
python3 /tmp/goose-node-management-package.py copied
cd "$STAGE/source"
source bin/activate-hermit
[[ $(node --version) == v24.* ]]
export NODE_OPTIONS=--max-old-space-size=8192
cd ui/desktop
node - <<'JS'
const fs=require('fs'),path=require('path');
const sdk=fs.realpathSync(require.resolve('@aaif/goose-sdk'));
if(!sdk.startsWith(path.resolve('../sdk/dist')+path.sep)) throw Error('SDK resolved outside isolated stage: '+sdk);
console.log('STAGE_LOCAL_SDK_PASS',sdk);
JS
pnpm run typecheck
pnpm exec vitest run src/components/nodes
# SDK generation/build is already gated and pinned above; avoid a second SDK build via pnpm package.
pnpm run i18n:compile
unset APPLE_TEAM_ID APPLE_ID APPLE_ID_PASSWORD GOOSE_LOCAL_SIGN_IDENTITY
pnpm exec electron-forge package
python3 /tmp/goose-node-management-package.py source
python3 /tmp/goose-node-management-package.py sdk
python3 /tmp/goose-node-management-package.py payload
python3 /tmp/goose-node-management-package.py packaged
app="$STAGE/source/ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$STAGE/Goose-Swarm-unsigned.zip"
shasum -a256 "$STAGE/Goose-Swarm-unsigned.zip" > "$STAGE/unsigned-archive-sha256.txt"
python3 /tmp/goose-node-management-package.py receipt
[[ $(df -k /System/Volumes/Data | awk 'NR==2{print $4}') -ge 10485760 ]]

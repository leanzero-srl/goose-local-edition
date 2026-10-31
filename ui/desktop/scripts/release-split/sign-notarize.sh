#!/bin/zsh
# Split release, signer Mac (the workhorse): sign an unsigned app built elsewhere, notarize, staple,
# build + notarize the DMG, zip and write latest-mac.yml into <release worktree>/ui/desktop/out/ so
# `just publish-release <ver>` runs unchanged. Light work: run under `nice -n 15 taskpolicy -b`.
# Usage: sign-notarize.sh <version> <release worktree> <unsigned.zip> <build-receipt.json> <tested cli sha256> [reference.app]
# Never copies signing material anywhere; refuses when memory pressure is not normal.
set -euo pipefail
VER=${1:?version} WT=${2:?release worktree} ZIP=${3:?unsigned zip} RECEIPT=${4:?build receipt} CLI_SHA=${5:?tested cli sha}
REF=${6:-}
HERE=${0:A:h}
STAGE=$(mktemp -d /tmp/goose-release-$VER-XXXXXX); rmdir $STAGE
OUT=$WT/ui/desktop/out; DIR="$OUT/Goose Swarm-darwin-arm64"; APP="$DIR/Goose Swarm.app"
log() { echo "=== $(date +%H:%M:%S) $*"; }
mem_ok() { local p; p=$(memory_pressure | awk -F': ' '/free percentage/{gsub("%","",$2);print $2}');
  log "memory free ${p}%"; [ "${p:-0}" -ge 25 ] || { echo "REFUSED: memory free ${p}% < 25% (MiMo rank 0 shares this Mac)"; exit 75; }; }
[ -e "$OUT" ] && { echo "REFUSED: $OUT exists; never sign over an earlier output"; exit 2; }
set -a; . "$HOME/.leanzero/apple/notary.env"; set +a
security unlock-keychain -p "$(cat "$HOME/.leanzero/apple/signing-keychain-password.txt")" "$HOME/Library/Keychains/goose-signing.keychain-db"
notarize() { local f=$1 json; json=$(xcrun notarytool submit "$f" --apple-id "$APPLE_ID" --password "$APPLE_ID_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait --output-format json)
  echo "$json"; echo "$json" | python3 -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get("status")=="Accepted" else 1)'; }

mem_ok; log "sign + receipts (stage $STAGE)"
node $HERE/sign-unsigned-app.cjs "$RECEIPT" "$ZIP" "$STAGE" "$CLI_SHA" ${REF:+"$REF"}
SIGNED="$STAGE/Goose Swarm.app"
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["signedFingerprintSha256"]; print("receipt machO", len(r["machOEquivalence"]), "controls", r["negativeControls"]["rejected"], r["machONegativeControl"]["nonSignatureCodeChangeRejected"])' "$STAGE/receipt.json"

mem_ok; log "notarize app"
python3 -I $HERE/bundle-fingerprint.py "$SIGNED" "$STAGE/before-upload.json"
cmp -s "$STAGE/before-upload.json" "$STAGE/after-signing.json" || { echo "signed app changed between receipt and upload"; exit 1; }
ditto -c -k --sequesterRsrc --keepParent "$SIGNED" "$STAGE/notarize-app.zip"
notarize "$STAGE/notarize-app.zip" | tee "$STAGE/notary-app.json"
xcrun stapler staple "$SIGNED"; xcrun stapler validate "$SIGNED"
python3 -I $HERE/bundle-fingerprint.py "$SIGNED" "$STAGE/after-staple.json"
python3 - "$STAGE/after-signing.json" "$STAGE/after-staple.json" <<'PY'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
changed = sorted(p for p in set(a) | set(b) if a.get(p) != b.get(p))
assert changed == ['Contents/CodeResources'] and 'Contents/CodeResources' not in a, changed
print('staple changed only Contents/CodeResources')
PY
codesign --verify --deep --strict "$SIGNED"
/usr/sbin/spctl -a -vvv -t exec "$SIGNED" 2>&1 | tee /dev/stderr | grep -q "Notarized Developer ID"

mem_ok; log "dmg"
mkdir -p "$DIR" "$OUT/make" "$OUT/dmgstage"
ditto "$SIGNED" "$APP"
ditto "$SIGNED" "$OUT/dmgstage/Goose Swarm.app"; ln -s /Applications "$OUT/dmgstage/Applications"
DMG="$OUT/make/Goose-Swarm-$VER.dmg"
hdiutil create -volname "Goose Swarm" -srcfolder "$OUT/dmgstage" -ov -format UDZO "$DMG"
notarize "$DMG" | tee "$STAGE/notary-dmg.json"
xcrun stapler staple "$DMG"; xcrun stapler validate "$DMG"
rm -rf "$OUT/dmgstage"

log "update zip + manifest"
python3 -I $HERE/bundle-fingerprint.py "$APP" "$STAGE/release-app.json"
cmp -s "$STAGE/release-app.json" "$STAGE/after-staple.json" || { echo "release copy differs from the stapled app"; exit 1; }
ditto -c -k --sequesterRsrc --keepParent "$APP" "$DIR/Goose-Swarm.zip"
(cd $WT/ui/desktop && node scripts/generate-mac-update-manifest.js --version $VER --directory "$DIR")
[ "$(shasum -a 256 "$APP/Contents/Resources/bin/goose" | cut -d' ' -f1)" = "$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["signedCliSha256"])' "$STAGE/receipt.json")" ]
cp "$STAGE"/receipt.json "$STAGE"/notary-*.json "$OUT/"
ls -la "$DIR" "$OUT/make"
log "DONE stage=$STAGE (delete after publish)"

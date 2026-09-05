#!/bin/bash
# Bootstrap Developer-ID signing + notarization on THIS Mac from the LeanZero signing bundle.
#
# The bundle lives at ~/.leanzero/apple on the workhorse (Mac Studio) and is NEVER in git:
#   goose-developer-id.p12          Developer ID Application cert + private key (Mihai Perdum, ZZ8MTZ6NRZ)
#   p12-password.txt                the .p12 password
#   notary.env                      APPLE_TEAM_ID / APPLE_ID / APPLE_ID_PASSWORD (app-specific password)
#   DeveloperIDG2CA.cer             Apple's Developer ID G2 intermediate
# On a machine without the bundle it is pulled over the existing `workhorse` SSH alias.
#
# It creates a DEDICATED keychain (goose-signing) with its own generated password, imports the
# identity with codesign in the ACL and the apple-tool partition set, and puts it first in the
# search list — so `codesign`, forge's osx-sign and notarytool run WITHOUT a keychain prompt and
# without anyone's login password. Idempotent: re-running repairs rather than duplicates.
set -euo pipefail

BUNDLE="$HOME/.leanzero/apple"
KEYCHAIN="$HOME/Library/Keychains/goose-signing.keychain-db"
IDENTITY="Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)"
SOURCE_HOST="${SIGNING_BUNDLE_HOST:-workhorse}"

mkdir -p "$BUNDLE" && chmod 700 "$BUNDLE"
for f in goose-developer-id.p12 p12-password.txt notary.env DeveloperIDG2CA.cer; do
  if [ ! -s "$BUNDLE/$f" ]; then
    echo "bootstrap-signing: fetching $f from $SOURCE_HOST"
    scp -q "$SOURCE_HOST:~/.leanzero/apple/$f" "$BUNDLE/$f"
  fi
  chmod 600 "$BUNDLE/$f"
done

if [ ! -s "$BUNDLE/signing-keychain-password.txt" ]; then
  openssl rand -hex 16 > "$BUNDLE/signing-keychain-password.txt"
  chmod 600 "$BUNDLE/signing-keychain-password.txt"
fi
KP="$(cat "$BUNDLE/signing-keychain-password.txt")"
P12P="$(cat "$BUNDLE/p12-password.txt")"

if [ ! -f "$KEYCHAIN" ]; then
  security create-keychain -p "$KP" "$KEYCHAIN"
  security set-keychain-settings "$KEYCHAIN"        # no auto-lock: a release build runs unattended
fi
security unlock-keychain -p "$KP" "$KEYCHAIN"
security import "$BUNDLE/DeveloperIDG2CA.cer" -k "$KEYCHAIN" >/dev/null 2>&1 || true
if ! security find-identity -v -p codesigning "$KEYCHAIN" | grep -q "$IDENTITY"; then
  security import "$BUNDLE/goose-developer-id.p12" -k "$KEYCHAIN" -P "$P12P" \
    -T /usr/bin/codesign -T /usr/bin/security -T /usr/bin/productsign >/dev/null
fi
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KP" "$KEYCHAIN" >/dev/null
# goose-signing first, then whatever was already in the user search list (dedupe).
current=$(security list-keychains -d user | tr -d '" ' | grep -v "^$" | grep -v "goose-signing" || true)
# shellcheck disable=SC2086
security list-keychains -d user -s "$KEYCHAIN" $current

echo "bootstrap-signing: identity"
security find-identity -v -p codesigning "$KEYCHAIN" | grep "$IDENTITY"

# The proof: sign a scratch binary WITHOUT any prompt and read the authority back.
cp /usr/bin/true /tmp/goose-signing-selftest
codesign --force --options runtime --keychain "$KEYCHAIN" -s "$IDENTITY" /tmp/goose-signing-selftest
codesign -dvv /tmp/goose-signing-selftest 2>&1 | grep -E 'Authority=Developer ID Application|TeamIdentifier=ZZ8MTZ6NRZ'
rm -f /tmp/goose-signing-selftest

# The notarization login, proven read-only against Apple.
set -a; . "$BUNDLE/notary.env"; set +a
xcrun notarytool history --apple-id "$APPLE_ID" --password "$APPLE_ID_PASSWORD" --team-id "$APPLE_TEAM_ID" | head -3
echo "bootstrap-signing: READY — \`just release-notarized <version>\` will sign and notarize on this Mac."

#!/usr/bin/env bash
# Stop macOS re-prompting "goose wants to use your confidential information" on every rebuild.
#
# A keychain item carries TWO independent gates:
#   1. the trusted-application ACL  -- satisfied permanently by signing every build with the
#      stable "Goose Local Dev" certificate (the Justfile already does this); the ACL records
#      a leaf-hash requirement, which does not move between builds.
#   2. the PartitionID ACL          -- a list of partition IDs. Code that carries a Team
#      Identifier partitions as `teamid:XXXX` and is stable; code WITHOUT one (every ad-hoc
#      and every self-signed build, because codesign does not derive a Team ID from a
#      self-signed certificate) partitions as `cdhash:<this build>`. That is why clicking
#      "Always Allow" never ends: each click appends one more dead cdhash. The `goose` item
#      had 24 of them.
#
# Only gate 2 needs this script, and only once. It clears the partition list so the item stops
# being bound to individual builds; gate 1 still restricts access to code signed with the cert.
#
# It PROVES the mechanism on a throwaway item first and refuses to touch a real item if the
# proof fails. Expect to be asked for your login password once per item.
set -euo pipefail

KC="$HOME/Library/Keychains/login.keychain-db"
IDENTITY="Goose Local Dev"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"; security delete-generic-password -s kctest-partition-probe -a kctest-partition-probe "$KC" >/dev/null 2>&1 || true' EXIT

security find-identity -p codesigning | grep -q "$IDENTITY" || {
    echo "No '$IDENTITY' identity. Run: just setup-signing-identity" >&2; exit 1; }

cat > "$WORK/probe.c" <<'EOF'
#include <stdio.h>
#include <string.h>
#include <Security/Security.h>
int main(int argc, char **argv) {
    SecKeychainSetUserInteractionAllowed(false);
    SecKeychainRef kc = NULL;
    if (SecKeychainOpen(argv[1], &kc) != errSecSuccess) { puts("OPEN_FAIL"); return 2; }
    UInt32 len = 0; void *data = NULL;
    OSStatus s = SecKeychainFindGenericPassword(kc, (UInt32)strlen(argv[2]), argv[2],
                                                (UInt32)strlen(argv[3]), argv[3], &len, &data, NULL);
    if (s == errSecSuccess) { puts("ALLOWED"); SecKeychainItemFreeContent(NULL, data); return 0; }
    printf("BLOCKED status=%d\n", (int)s);
    return 1;
}
EOF
cc -o "$WORK/probe" "$WORK/probe.c" -framework Security -framework CoreFoundation 2>/dev/null
codesign --force --sign "$IDENTITY" "$WORK/probe" 2>/dev/null

echo "== proving the partition gate on a throwaway item =="
security delete-generic-password -s kctest-partition-probe -a kctest-partition-probe "$KC" >/dev/null 2>&1 || true
security add-generic-password -A -a kctest-partition-probe -s kctest-partition-probe -w probe "$KC"
if "$WORK/probe" "$KC" kctest-partition-probe kctest-partition-probe; then
    echo "Throwaway item was readable before clearing its partition list -- this machine does not"
    echo "enforce partition IDs, so the prompts have some other cause. Nothing changed." >&2
    exit 1
fi

echo "== clearing the throwaway item's partition list (login password follows) =="
security set-generic-password-partition-list -S "" -s kctest-partition-probe -a kctest-partition-probe "$KC" >/dev/null
if ! "$WORK/probe" "$KC" kctest-partition-probe kctest-partition-probe; then
    echo
    echo "An empty partition list does NOT grant access on this macOS build." >&2
    echo "Nothing was changed on any real goose item. The remaining options are a paid Apple" >&2
    echo "Developer ID certificate (it carries a Team Identifier, so the partition is stable)," >&2
    echo "or moving goose's secrets off the login keychain." >&2
    exit 1
fi
echo "== proven: an empty partition list allows the cert-signed build =="

while IFS='|' read -r svc acct; do
    echo "-- $svc / $acct"
    security set-generic-password-partition-list -S "" -s "$svc" -a "$acct" "$KC" >/dev/null
done <<'ITEMS'
goose|secrets
goose-sb7-xai-api|goose-sb7-xai
goose-benchmark-minimax-api|goose-sb7-minimax-m3
goose-benchmark-minimax-api|goose-sb7-minimax-m3-cp
goose-benchmark-moonshot-api|goose-sb7-kimi-k3
goose-benchmark-muse-api|goose-sb7-muse-models
goose-benchmark-qwen-api|goose-sb7-qwen38-max
Goose Safe Storage|Goose Key
ITEMS

echo
echo "Done. The next cert-signed build still prompts ONCE per item -- every entry in those items'"
echo "trusted-application lists was recorded against a dead ad-hoc build. Click Always Allow; that"
echo "records the certificate's leaf hash, which never moves again."

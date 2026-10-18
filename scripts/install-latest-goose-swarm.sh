#!/bin/zsh
# Install the latest Goose Swarm release from leanzero-srl/goose-local-edition on THIS Mac.
# Waits for any running benchmark to finish first (installing quits the app, which would kill the run).
# usage: scripts/install-latest-goose-swarm.sh [vX.Y.Z]
set -e
APP="/Applications/Goose Swarm.app"
TAG=${1:-$(curl -fsSI https://github.com/leanzero-srl/goose-local-edition/releases/latest | awk -F/ 'tolower($1) ~ /^location/ {print $NF}' | tr -d '\r')}
WANT=${TAG#v}
HAVE=$(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo none)
echo "installed $HAVE, latest $WANT"
[ "$HAVE" = "$WANT" ] && exit 0

while pgrep -f bench/run_build.py >/dev/null; do
  echo "$(date +%H:%M) a benchmark is running; waiting for it to finish"; sleep 300
done

TMP=$(mktemp -d)
curl -fL -o "$TMP/app.zip" "https://github.com/leanzero-srl/goose-local-edition/releases/download/$TAG/Goose-Swarm-darwin-arm64.zip"
ditto -x -k "$TMP/app.zip" "$TMP/x"
NEW="$TMP/x/Goose Swarm.app"
codesign --verify --deep --strict "$NEW"
spctl --assess --type execute "$NEW"
[ "$(defaults read "$NEW/Contents/Info" CFBundleShortVersionString)" = "$WANT" ]

osascript -e 'tell application "Goose Swarm" to quit' || true
for _ in $(seq 1 30); do pgrep -f "Goose Swarm.app/Contents/MacOS/Goose Swarm" >/dev/null || break; sleep 1; done
for pid in $(pgrep -f "Goose Swarm.app/Contents/MacOS/Goose Swarm"); do kill "$pid"; done
rm -rf "$APP" && ditto "$NEW" "$APP"
rm -rf "$TMP"
echo "installed $(defaults read "$APP/Contents/Info" CFBundleShortVersionString)"
open "$APP"

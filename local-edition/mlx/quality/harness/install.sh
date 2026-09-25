#!/bin/zsh
# install.sh <version> — install a finished notarized build on BOTH Macs, safely.
# Refuses unless the build log says ">>> DONE" AND the DMG + app for <version> exist. Copies the new app
# beside the old one first, then swaps — a failed step never leaves a Mac without an app (2026-09-25:
# a wait loop keyed on the word "Invalid" in the build's JSON output fired early; the old one-liner
# rm -rf'd the installed app before ditto found no build).
set -euo pipefail
v=$1; repo=${REPO:-/Users/mihaiperdum/Projects/goose}/ui/desktop
dmg="$repo/out/make/Goose-Swarm-$v.dmg"; app="$repo/out/Goose Swarm-darwin-arm64/Goose Swarm.app"
grep -q ">>> DONE" ~/goose-builds/release-$v.log || { echo "build $v has not finished (no >>> DONE)"; exit 2; }
[ -f "$dmg" ] && [ -d "$app" ] || { echo "missing $dmg or $app"; exit 2; }
built=$(defaults read "$app/Contents/Info.plist" CFBundleShortVersionString)
[ "$built" = "$v" ] || { echo "built app is $built, not $v"; exit 2; }
# Studio: copy the DMG, stage the app beside the old one, then swap.
scp -q "$dmg" workhorse-tb:/tmp/gs-$v.dmg
ssh workhorse "set -e; mp=\$(hdiutil attach -nobrowse -readonly /tmp/gs-$v.dmg | grep -o '/Volumes/.*' | head -1); rm -rf '/Applications/Goose Swarm.new.app'; ditto \"\$mp/Goose Swarm.app\" '/Applications/Goose Swarm.new.app'; hdiutil detach -quiet \"\$mp\"; rm /tmp/gs-$v.dmg; osascript -e 'quit app \"Goose Swarm\"' 2>/dev/null || true; for i in \$(seq 1 30); do pgrep -f 'Goose Swarm.app/Contents/MacOS/Goose Swarm' >/dev/null || break; sleep 1; done; rm -rf '/Applications/Goose Swarm.app'; mv '/Applications/Goose Swarm.new.app' '/Applications/Goose Swarm.app'; echo studio \$(defaults read '/Applications/Goose Swarm.app/Contents/Info.plist' CFBundleShortVersionString); open -a '/Applications/Goose Swarm.app'"
# This Mac: the same, then relaunch with the CDP port the harness uses.
rm -rf "/Applications/Goose Swarm.new.app"; ditto "$app" "/Applications/Goose Swarm.new.app"
osascript -e 'quit app "Goose Swarm"' 2>/dev/null || true
for i in $(seq 1 30); do pgrep -f "Goose Swarm.app/Contents/MacOS/Goose Swarm" >/dev/null || break; sleep 1; done
rm -rf "/Applications/Goose Swarm.app"; mv "/Applications/Goose Swarm.new.app" "/Applications/Goose Swarm.app"
spctl -a -vv "/Applications/Goose Swarm.app" 2>&1 | head -1
echo "macbook $(defaults read '/Applications/Goose Swarm.app/Contents/Info.plist' CFBundleShortVersionString)"
open -a "/Applications/Goose Swarm.app" --args --remote-debugging-port=9333
for i in $(seq 1 60); do curl -s 127.0.0.1:9333/json/version >/dev/null && break; sleep 1; done; echo up

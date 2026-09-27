#!/bin/zsh
# install.sh <version> — install a finished notarized build on BOTH Macs, safely, and PROVE the new app runs.
# Refuses unless the build log says ">>> DONE" AND the DMG + app for <version> exist. Copies the new app
# beside the old one first, then swaps — a failed step never leaves a Mac without an app (2026-09-25:
# a wait loop keyed on the word "Invalid" in the build's JSON output fired early; the old one-liner
# rm -rf'd the installed app before ditto found no build).
# Q-180 (2026-09-27): a quit that did not happen was not noticed — the bundle was swapped under the LIVE old
# app, `open -a` re-focused it, and "3.0.59 up" was read from the plist on disk while 3.0.58 ran for an hour
# (E2E #3g void). Now: no swap while the old app runs (exit 3, naming the pid and any dialog holding it), and
# "up" means a main process that STARTED after the swap.
set -euo pipefail
v=$1; repo=${REPO:-/Users/mihaiperdum/Projects/goose}/ui/desktop
dmg="$repo/out/make/Goose-Swarm-$v.dmg"; app="$repo/out/Goose Swarm-darwin-arm64/Goose Swarm.app"
MAIN='Goose Swarm.app/Contents/MacOS/Goose Swarm'
grep -q ">>> DONE" ~/goose-builds/release-$v.log || { echo "build $v has not finished (no >>> DONE)"; exit 2; }
[ -f "$dmg" ] && [ -d "$app" ] || { echo "missing $dmg or $app"; exit 2; }
built=$(defaults read "$app/Contents/Info.plist" CFBundleShortVersionString)
[ "$built" = "$v" ] || { echo "built app is $built, not $v"; exit 2; }

# Studio: copy the DMG, stage beside the old app, quit, REFUSE to swap if it still runs, swap, launch, prove.
scp -q "$dmg" workhorse-tb:/tmp/gs-$v.dmg
ssh workhorse "set -e; mp=\$(hdiutil attach -nobrowse -readonly /tmp/gs-$v.dmg | grep -o '/Volumes/.*' | head -1); rm -rf '/Applications/Goose Swarm.new.app'; ditto \"\$mp/Goose Swarm.app\" '/Applications/Goose Swarm.new.app'; hdiutil detach -quiet \"\$mp\"; rm /tmp/gs-$v.dmg
osascript -e 'quit app \"Goose Swarm\"' 2>/dev/null || true
for i in \$(seq 1 60); do pgrep -f '$MAIN' >/dev/null || break; sleep 1; done
if pgrep -f '$MAIN' >/dev/null; then echo \"studio: the old app did not quit (pid \$(pgrep -f '$MAIN' | head -1)) — NOT swapping\"; exit 3; fi
for p in \$(ps -Ao pid=,ppid=,args= | awk '\$2==1 && /Goose Swarm.app\\/Contents\\/Resources\\/bin\\/(goose serve|tailscaled --tun)/ {print \$1}'); do echo \"studio: reaping orphan pid \$p\"; kill \$p; done
rm -rf '/Applications/Goose Swarm.app'; mv '/Applications/Goose Swarm.new.app' '/Applications/Goose Swarm.app'
swap=\$(date +%s); open -a '/Applications/Goose Swarm.app'
for i in \$(seq 1 60); do p=\$(pgrep -f '$MAIN' | head -1); [ -n \"\$p\" ] && break; sleep 1; done
started=\$(date -j -f '%a %b %d %T %Y' \"\$(ps -o lstart= -p \$p)\" +%s)
[ \$started -ge \$swap ] || { echo \"studio: running app pid \$p started before the swap\"; exit 3; }
echo studio \$(defaults read '/Applications/Goose Swarm.app/Contents/Info.plist' CFBundleShortVersionString) running pid \$p"

# This Mac: the same, then relaunch with the CDP port the harness uses.
rm -rf "/Applications/Goose Swarm.new.app"; ditto "$app" "/Applications/Goose Swarm.new.app"
osascript -e 'quit app "Goose Swarm"' 2>/dev/null || true
for i in $(seq 1 60); do pgrep -f "$MAIN" >/dev/null || break; sleep 1; done
if pgrep -f "$MAIN" >/dev/null; then
  held=$(pgrep -f "$MAIN" | head -1)
  # Name what holds it (a confirm the quit raised) instead of swapping under a live app.
  dialog=$(node -e "
const {chromium}=require('/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core');
chromium.connectOverCDP('http://127.0.0.1:9333').then(async b=>{const p=b.contexts()[0].pages().find(x=>x.url().includes('index.html'));
const d=p.getByRole('dialog'); console.log(await d.count()? (await d.first().innerText()).replace(/\s+/g,' ').slice(0,200):'no dialog'); process.exit(0)}).catch(e=>{console.log('cdp: '+e.message);process.exit(0)})" 2>/dev/null)
  echo "macbook: the old app did not quit (pid $held; on screen: $dialog) — NOT swapping"; exit 3
fi
# Q-223 (2026-09-27): the 3.0.61 goosed survived the quit as an orphan (ppid 1) and kept Link's tailscaled, so the
# new app's Link refused to connect. Until the app's quit path is fixed, reap orphaned goose servers of THIS bundle
# per pid (never killpg — gate 4), then their tailscaled once its owner is gone.
reap_orphans() {
  for p in $(ps -Ao pid=,ppid=,args= | awk '$2==1 && /Goose Swarm.app\/Contents\/Resources\/bin\/goose serve/ {print $1}'); do
    echo "macbook: reaping orphaned goose serve pid $p"; kill $p; done
  sleep 3
  for p in $(ps -Ao pid=,ppid=,args= | awk '$2==1 && /Goose Swarm.app\/Contents\/Resources\/bin\/tailscaled --tun/ {print $1}'); do
    echo "macbook: reaping orphaned tailscaled pid $p"; kill $p; done
}
reap_orphans
rm -rf "/Applications/Goose Swarm.app"; mv "/Applications/Goose Swarm.new.app" "/Applications/Goose Swarm.app"
spctl -a -vv "/Applications/Goose Swarm.app" 2>&1 | head -1
swap=$(date +%s)
open -a "/Applications/Goose Swarm.app" --args --remote-debugging-port=9333
for i in $(seq 1 60); do curl -s 127.0.0.1:9333/json/version >/dev/null && break; sleep 1; done
p=$(pgrep -f "$MAIN" | head -1)
started=$(date -j -f '%a %b %d %T %Y' "$(ps -o lstart= -p $p)" +%s)
[ $started -ge $swap ] || { echo "macbook: the app on 9333 (pid $p) started BEFORE the swap — the old build is still running"; exit 3; }
echo "macbook $(defaults read '/Applications/Goose Swarm.app/Contents/Info.plist' CFBundleShortVersionString) running pid $p (started after the swap)"
echo up

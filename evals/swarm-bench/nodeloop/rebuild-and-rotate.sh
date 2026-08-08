#!/bin/sh
# Chain the rotation to the build instead of deferring it to a human tick — AND MARK WHAT IT KILLS.
#
# The first version of this script did the rotation correctly and corrupted the corpus doing it.
# Killing a cell does not abort its row: the sweep records the dead unit as a COMPLETED measurement
# with a real-looking score and void=False, aborted=False, timed_out=False. Three of my rotations
# landed as 0.0563, 0.0561, 0.0561 — indistinguishable from genuinely bad runs — and the third
# OVERWROTE baseline-n3-r0's 0.9033, the best result this campaign has produced.
#
# Those near-zero rows are not merely noise. Every spread and mean in this campaign is computed over
# the result files, so a rotation was silently manufacturing the very dispersion F533 exists to
# measure. Speeding the loop up was quietly destroying the thing the loop is for.
#
# So the kill now OWNS its consequence: it waits for the sweep to write the row, then marks it void
# with a reason. Void rows are excluded by analysis rather than deleted, because a deleted row looks
# like a unit that was never scheduled, and those are not the same thing.
set -u
SCRATCH=/private/tmp/claude-501/-Users-mihaiperdum-Projects-goose/124573f3-de2d-4c0d-a30d-b877e482d4b1/scratchpad
LOG="$SCRATCH/relbuild.log"
OUT="$SCRATCH/rotate.log"
BIN=/Users/mihaiperdum/Projects/goose/target/release/goose
RUNS=/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop

say() { echo "$(date '+%H:%M:%S') $*" >> "$OUT"; }

say "waiting for the release build to finish"
i=0
while pgrep -f "cargo build --release -p goose-cli" > /dev/null 2>&1; do
  i=$((i + 1))
  [ "$i" -gt 540 ] && { say "GIVING UP — build still running after 45 min"; exit 1; }
  sleep 5
done

if ! grep -q "Finished .release." "$LOG" 2>/dev/null; then
  say "BUILD DID NOT SUCCEED — leaving the running cell alone, nothing rotated"
  tail -20 "$LOG" >> "$OUT"
  exit 1
fi

SHA=$(git -C /Users/mihaiperdum/Projects/goose rev-parse --short HEAD)
say "build OK, HEAD $SHA, binary mtime $(stat -f '%Sm' "$BIN" 2>/dev/null)"

# WHICH cell is about to die — read from the sweep's own NOW line, not guessed.
CELL=$(grep -E "^>>> " "$RUNS/loop.log" 2>/dev/null | tail -1 | sed -n 's/.*NOW: \([^ ]*\).*/\1/p')
say "cell about to be rotated: ${CELL:-<unknown>}"

PID=$(pgrep -f "target/release/goose swarm run" | head -1)
if [ -n "${PID:-}" ]; then
  PGID=$(ps -o pgid= -p "$PID" | tr -d ' ')
  say "killing in-flight cell pid $PID pgid $PGID — it runs a stale engine"
  kill -TERM -"$PGID" 2>/dev/null
  sleep 5
  pgrep -f "target/release/goose swarm run" > /dev/null 2>&1 \
    && { say "still alive, escalating to KILL"; kill -KILL -"$PGID" 2>/dev/null; }
else
  say "no cell in flight — the next one the sweep starts already picks up the new binary"
fi

# THE HALF THE FIRST VERSION WAS MISSING. Wait for the sweep to write the dead unit's row, then void
# it. Bounded: if no row appears in 3 minutes the sweep did not record one, which is fine and is said
# out loud rather than assumed.
if [ -n "${CELL:-}" ]; then
  RES="$RUNS/$CELL/nodeloop-result.json"
  j=0
  while [ "$j" -lt 36 ]; do
    if [ -f "$RES" ] && [ "$(find "$RES" -mmin -3 2>/dev/null | wc -l | tr -d ' ')" = "1" ]; then
      python3 - "$RES" "$SHA" <<'PY' >> "$OUT" 2>&1
import json, sys
p, sha = sys.argv[1], sys.argv[2]
d = json.load(open(p))
d["void"] = True
d["void_reason"] = (f"ROTATION KILL: stopped mid-run to rebuild onto {sha}. This row is an "
                    "artefact of the harness, not a measurement of the engine — its score reflects "
                    "when it was killed and must never enter a mean or a spread.")
json.dump(d, open(p, "w"), indent=1)
print(f"  voided {p} (was score={d.get('score')})")
PY
      break
    fi
    j=$((j + 1)); sleep 5
  done
  [ "$j" -ge 36 ] && say "no fresh result row appeared for $CELL within 3 min — nothing to void"
fi

say "sweep alive: $(pgrep -f sweep.py | head -1 | wc -l | tr -d ' ') — next cell runs $SHA"

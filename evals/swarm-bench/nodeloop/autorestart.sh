#!/usr/bin/env bash
# Restart the sweep the MOMENT the STOP'd supervisor exits, without waiting for a human tick.
#
# WHY THIS EXISTS. Three committed fixes (F327 curve_first, F328 CURVE_REPS=8, F330 the watchdog
# silence rule) are invisible to the running supervisor, because a running process does not see
# source edits (L23). `STOP` is armed, so the supervisor exits cleanly once the current unit records
# — and from that instant the fleet is IDLE until someone runs `loop.sh start`.
#
# Tying that transition to a 5-minute conversational tick means up to 5 minutes of dead fleet on a
# good day, and an unbounded stall if a tick is missed or a context compacts across the boundary. The
# unattended rule is that the loop must live in a PROCESS, not in a conversation; this is the
# smallest process that satisfies it for the one transition that is currently manual.
#
# GUARDS, because an auto-restarter that fires at the wrong moment is worse than none:
#   - only ever acts while STOP is present. If STOP is gone, someone restarted by hand; exit.
#   - refuses while a `goose swarm run` engine is alive, so it cannot start a second sweep on top
#     of a unit that is still writing.
#   - `loop.sh start` itself refuses if a sweep is already running, so a double-fire is a no-op.
#   - bounded by MAX_WAIT; it is a nudge, not a daemon that outlives the question.
set -uo pipefail
cd "$(dirname "$0")"

LOG="$PWD/autorestart.log"
MAX_WAIT=${MAX_WAIT:-9000}          # 2.5 h — well past the longest observed unit (8488 s)
POLL=10

say() { echo "[$(date '+%H:%M:%S')] $*" >> "$LOG"; }

say "armed; waiting for the STOP'd supervisor to exit (max ${MAX_WAIT}s)"
waited=0
while [ "$waited" -lt "$MAX_WAIT" ]; do
  if [ ! -f STOP ]; then
    say "STOP is gone — someone restarted by hand. Standing down."
    exit 0
  fi
  if [ -z "$(pgrep -f 'nodeloop/sweep.py')" ]; then
    # The supervisor is down. Do NOT start while an engine is still writing a unit.
    engines=$(pgrep -f 'goose swarm run' | wc -l | tr -d ' ')
    if [ "$engines" != "0" ]; then
      say "supervisor down but $engines engine(s) still alive — waiting for the unit to finish"
    else
      say "supervisor down, 0 engines — restarting"
      rm -f STOP
      ./loop.sh start >> "$LOG" 2>&1
      sleep 5
      say "after start: $(./loop.sh status 2>&1 | head -1)"
      say "engines now: $(pgrep -f 'goose swarm run' | wc -l | tr -d ' ')"
      exit 0
    fi
  fi
  sleep "$POLL"
  waited=$((waited + POLL))
done
say "MAX_WAIT reached without the supervisor exiting — standing down, NOTHING was changed"

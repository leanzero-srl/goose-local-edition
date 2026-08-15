#!/usr/bin/env bash
# The STANDING watchdog — the answer to the supervisor that died at 18:35 and was silent for 75 min.
#
# Runs from launchd every WATCHDOG_INTERVAL (com.leanzero.swarmbench.watchdog, StartInterval 120),
# so it survives session loss, terminal loss, and operator absence — the three things a
# conversational tick cannot. It does exactly three things and refuses everything else:
#
#   1. STAND DOWN silently while STOP is present. A deliberate stop (boundary work, batch
#      building, an operator decision) must not fight its own watchdog. This is the one rule that
#      keeps an auto-restarter from being worse than none.
#   2. ALARM when health.py says BAD: append a line to ALARM (the operator tick reads and clears
#      it — a file cannot be missed the way a log line was), and raise a macOS notification.
#      health.py already knows every BAD state this campaign has measured: dead loop without STOP,
#      stale heartbeat under a live engine, no-progress, consecutive failures.
#   3. RESTART only in the one provably-safe case: supervisor GONE + no STOP + ZERO engines.
#      With an engine alive (the 22h orphan case), restarting would contend for the fleet —
#      boundary.py's standing advice is alarm-and-wait, so that is what happens.
#
# Everything else — scoring, boundaries, rebuilds, relevance kills — belongs to the operator loop.
# The watchdog's job is only that silence can no longer be mistaken for health.
set -uo pipefail
cd "$(dirname "$0")"

LOG="$PWD/watchdog.log"
say() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG"; }
alarm() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$PWD/ALARM"
  /usr/bin/osascript -e "display notification \"$*\" with title \"swarm-bench watchdog\"" 2>/dev/null || true
  say "ALARM: $*"
}

# 1. Deliberate stop: stand down silently.
[ -f STOP ] && exit 0

# 2. Ask the instrument, never re-derive (L2). Exit 2 == BAD.
HEALTH_OUT=$(/usr/bin/env python3 health.py 2>&1)
RC=$?
if [ "$RC" -lt 2 ]; then
  exit 0
fi

BADLINE=$(echo "$HEALTH_OUT" | grep -m1 "BAD" || echo "health.py exit $RC")
# END-ANCHORED, because `pgrep -f` matches any process whose COMMAND LINE contains the pattern —
# measured on this watchdog's own first firing: an operator shell that merely QUOTED the pattern
# ('...pgrep -f nodeloop/sweep.py...' inside a verification one-liner) read as a live sweep, the
# restart branch was skipped, and the fleet stayed down a full extra interval. The real supervisor's
# command line ENDS with the script path (Popen([python, '-u', 'nodeloop/sweep.py'])); a shell
# quoting it keeps talking afterwards. The engine pattern cannot be end-anchored (the prompt trails
# it) — a false engine match only delays the restart one interval, which is the safe direction.
# F839 HOLD: an operator phase that legitimately runs engines WITHOUT the sweep (the
# parallel-n1 block) hits the "nothing is running" window at batch boundaries, and the
# auto-restart then revives a sweep whose evict machinery kills the phase's engines as
# intruders — measured: all 8 n1 rows died at 0.045 in one night. touch HOLD to disarm;
# rm HOLD to re-arm. The hold is loud, not silent.
if [ -f "$(dirname "$0")/HOLD" ]; then
  say "HOLD present — watchdog disarmed by operator"
  exit 0
fi
SWEEP=$(pgrep -f 'nodeloop/sweep\.py$' | head -1)
ENGINES=$(pgrep -f 'goose swarm run' | wc -l | tr -d ' ')

if [ -z "$SWEEP" ] && [ "$ENGINES" = "0" ]; then
  # The one safe restart: nothing is running and nothing was asked to stop.
  alarm "supervisor dead, fleet empty — auto-restarting the sweep"
  ./loop.sh start >> "$LOG" 2>&1
  sleep 5
  NEW=$(pgrep -f 'nodeloop/sweep\.py$' | head -1)
  if [ -n "$NEW" ]; then
    say "restarted: sweep pid $NEW"
  else
    alarm "auto-restart FAILED — loop.sh start produced no sweep. Operator needed."
  fi
elif [ -z "$SWEEP" ]; then
  alarm "supervisor DEAD with $ENGINES live engine(s) — orphaned run. Not restarting (fleet contention); operator must decide. ${BADLINE}"
elif echo "$HEALTH_OUT" | grep "BAD" | grep -qiE "heartbeat|wedged|did not get the pool"; then
  # Supervisor AND engine alive: only a WEDGE-class BAD is the watchdog's business. The other BAD
  # lines (last-unit age, consecutive failures) are progress states the 5-minute operator tick
  # reads and acts on — alarming every 120s on a condition a healthy run clears by itself is the
  # alarm-that-cannot-clear (L193), and it trains the reader to ignore the one that matters.
  alarm "${BADLINE}"
fi

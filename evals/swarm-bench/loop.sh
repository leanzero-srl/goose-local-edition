#!/usr/bin/env bash
# One place to control the sweep. No remembering pids, ports or nohup incantations.
#
#   ./loop.sh start     launch it, truly detached (survives any shell or chat session dying)
#   ./loop.sh status    where it is, what it is doing, what is next
#   ./loop.sh watch     follow the headline lines only
#   ./loop.sh results   the score table so far
#   ./loop.sh stop      stop it (verdicts already on disk are kept)
#   ./loop.sh resume    same as start — finished episodes are skipped automatically
set -uo pipefail
cd "$(dirname "$0")"

LOG="${SWARM_BENCH_LOG:-$PWD/runs/sweep.log}"
mkdir -p "$(dirname "$LOG")"

pid() { pgrep -f 'bench/sweep.py' | head -1; }

case "${1:-status}" in
  start|resume)
    if [ -n "$(pid)" ]; then echo "already running (pid $(pid))"; exit 0; fi
    # start_new_session detaches from the process GROUP, not just the parent. nohup+disown alone
    # left ppid pointing at the launching shell and the sweep died with it.
    python3 -c "
import subprocess, sys
subprocess.Popen([sys.executable,'-u','bench/sweep.py'],
                 stdout=open('$LOG','a'), stderr=subprocess.STDOUT,
                 start_new_session=True)"
    sleep 3
    P=$(pid)
    echo "started pid=${P:-?} ppid=$(ps -o ppid= -p "${P:-0}" 2>/dev/null | tr -d ' ') (1 = detached)"
    echo "log: $LOG"
    ;;
  status)
    P=$(pid)
    if [ -n "$P" ]; then
      echo "RUNNING  pid=$P  elapsed=$(ps -o etime= -p "$P" | tr -d ' ')"
    else
      echo "NOT RUNNING"
    fi
    echo
    grep -E '^>>> \[|^ +NEXT:|^\[(run|done|skip|stale|retry|fail|warn|GATE)' "$LOG" 2>/dev/null | tail -8
    ;;
  watch)
    tail -f "$LOG" | grep -E --line-buffered '^>>> \[|^ +NEXT:|^\[(done|skip|stale|retry|fail|GATE)|SWEEP COMPLETE'
    ;;
  results)
    python3 - <<'PY'
import json, pathlib
rows = []
for f in sorted(pathlib.Path('runs/build').glob('*/verdict.json')):
    try:
        v = json.loads(f.read_text())
    except Exception:
        continue
    rows.append(v)
if not rows:
    print("no verdicts yet"); raise SystemExit
rows.sort(key=lambda v: -v.get('score', 0))
print(f"{'entrant':<16}{'rep':>4}{'score':>8}   A    B    C    D   scorer")
for v in rows:
    t = v.get('tiers', {})
    cell = lambda k: f"{100 * t.get(k, {}).get('mean', 0):>3.0f}%"
    print(f"{v.get('entrant', '?'):<16}{v.get('rep', 0):>4}{100 * v.get('score', 0):>7.1f}%  "
          f"{cell('A')} {cell('B')} {cell('C')} {cell('D')}  {v.get('scorer_version', 'UNVERSIONED')}")
stale = {v.get('scorer_version') for v in rows}
if len(stale) > 1:
    print(f"\n!! mixed scorer versions {sorted(x or 'UNVERSIONED' for x in stale)} — "
          f"rows are NOT comparable until the stale ones are re-run")
PY
    ;;
  stop)
    P=$(pid)
    if [ -z "$P" ]; then echo "not running"; exit 0; fi
    kill "$P" 2>/dev/null
    pkill -f 'bench/run_build.py' 2>/dev/null
    echo "stopped (verdicts on disk are kept; ./loop.sh resume picks up where it left off)"
    ;;
  *)
    sed -n '2,12p' "$0"
    ;;
esac

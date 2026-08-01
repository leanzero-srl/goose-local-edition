#!/usr/bin/env bash
# Control surface for the dispatch-quality loop. No pids, no nohup incantations to remember.
#
#   ./loop.sh start     launch it, truly detached (survives this shell and this chat session)
#   ./loop.sh status    running or not, and the last few NOW/NEXT lines
#   ./loop.sh watch     follow the headline lines only
#   ./loop.sh results   the arm table so far, mechanism counts first
#   ./loop.sh stop      stop it after the current unit; results on disk are kept
#   ./loop.sh resume    same as start — finished units are skipped
set -uo pipefail
cd "$(dirname "$0")"

LOG="${NODELOOP_LOG:-$PWD/../runs/nodeloop/loop.log}"
mkdir -p "$(dirname "$LOG")"

pid() { pgrep -f 'nodeloop/sweep.py' | head -1; }

case "${1:-status}" in
  start|resume)
    if [ -n "$(pid)" ]; then echo "already running (pid $(pid))"; exit 0; fi
    rm -f STOP
    # start_new_session detaches from the process GROUP, not just the parent. Measured on this
    # machine: nohup+disown left ppid pointing at the launching shell and the job died with it
    # (repair.py, 2026-08-01 08:58, 90 seconds in). -u or every monitor is blind for hours.
    python3 -c "
import subprocess, sys
subprocess.Popen([sys.executable,'-u','nodeloop/sweep.py'],
                 cwd='$PWD/..',
                 stdout=open('$LOG','a'), stderr=subprocess.STDOUT,
                 start_new_session=True)"
    sleep 3
    P=$(pid)
    if [ -z "$P" ]; then
      echo "FAILED to start — last log lines:"; tail -20 "$LOG"; exit 1
    fi
    echo "started pid=$P ppid=$(ps -o ppid= -p "$P" | tr -d ' ') (1 = detached and safe)"
    echo "log: $LOG"
    ;;
  status)
    P=$(pid)
    if [ -n "$P" ]; then
      echo "RUNNING  pid=$P  elapsed=$(ps -o etime= -p "$P" | tr -d ' ')"
    else
      echo "NOT RUNNING"
    fi
    [ -f STOP ] && echo "STOP sentinel present — it will exit after the current unit"
    echo
    grep -E '^>>> |^    NEXT:|^\[(done|fail|retry|stop|grow|warn)' "$LOG" 2>/dev/null | tail -10
    ;;
  watch)
    # Failure signatures too: a monitor grepping only the happy path stays silent through a
    # crashloop, and silence looks exactly like "still running".
    tail -f "$LOG" | grep -E --line-buffered \
      '^>>> |^    NEXT:|^\[(done|fail|retry|stop|grow|warn)|Traceback|Error|Killed'
    ;;
  results)
    python3 - <<'PY'
import json, pathlib, collections
rows = collections.defaultdict(list)
for f in sorted(pathlib.Path('../runs/nodeloop').glob('*/nodeloop-result.json')):
    try:
        r = json.loads(f.read_text())
    except Exception:
        continue
    rows[(r.get('arm'), r.get('nodes'))].append(r)
if not rows:
    print("no results yet"); raise SystemExit
versions = {r.get('audit_version') for rs in rows.values() for r in rs}
if len(versions) > 1:
    print(f"!! mixed audit versions {versions} — these rows are NOT comparable\n")
builds = {r.get('engine_build') for rs in rows.values() for r in rs}
if len(builds) > 1:
    print(f"!! MIXED ENGINE BUILDS {builds} — rows across a rebuild measure different engines")
    print("   and are NOT comparable. Re-baseline rather than averaging across the boundary.\n")
print(f"{'arm':<18}{'nodes':>5}{'n':>3}  {'score mean':>10} {'spread':>8}  "
      f"{'fallbacks':>9} {'kind-mm%':>9} {'wall min':>9}  void")
for (arm, nodes), rs in sorted(rows.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
    ok = [r for r in rs if not r.get('timed_out') and not r.get('aborted')
          and not r.get('void') and r.get('score') is not None]
    sc = [r['score'] for r in ok]
    fb = [x for x in (r.get('audit', {}).get('detail_fallback_count') for r in ok) if x is not None]
    km = [x for x in (r.get('audit', {}).get('kind_mismatch_pct') for r in ok) if x is not None]
    wl = [r['wall_secs'] for r in ok if r.get('wall_secs')]
    mean = f"{sum(sc)/len(sc):.1%}" if sc else "-"
    spread = f"{(max(sc)-min(sc))*100:.0f}pts" if len(sc) > 1 else "-"
    print(f"{arm:<18}{nodes if nodes is not None else '?':>5}{len(rs):>3}  "
          f"{mean:>10} {spread:>8}  "
          f"{(sum(fb)/len(fb) if fb else 0):>9.1f} "
          f"{(sum(km)/len(km) if km else 0):>9.1f} "
          f"{(sum(wl)/len(wl)/60 if wl else 0):>9.0f}  "
          f"{sum(1 for r in rs if r.get('void'))}")
print()
print("fallbacks = tasks whose spec never got past the architect's one-liner (swarm.rs:12353).")
print("void = the engine did not build the pool the unit asked for; those are excluded, never averaged.")
print("A score delta below the replicate spread is not a result; read the mechanism columns.")
PY
    ;;
  check)
    python3 "$PWD/health.py"
    ;;
  abort)
    # Cut the CURRENT swarm run loose. The loop itself keeps going and moves to the next unit;
    # this only kills the doomed episode so it stops holding the single addressable worker.
    N=0
    for P in $(pgrep -f 'goose swarm run'); do
      kill -9 -- "-$(ps -o pgid= -p "$P" | tr -d ' ')" 2>/dev/null || kill -9 "$P" 2>/dev/null
      N=$((N+1))
    done
    echo "aborted $N engine process group(s); the loop continues with the next unit"
    ;;
  boundary)
    # THE PASS BOUNDARY, as a procedure rather than as memory. Engine fixes accumulate while a
    # campaign runs, because rebuilding mid-campaign voids comparability (complete() requires a
    # matching engine_build). Crossing the boundary is a multi-step sequence with real failure
    # modes, and I did it by hand once: rebuild only while the fleet is IDLE, VERIFY the markers
    # landed because compiling is not shipping, and never restart on a binary that is missing one.
    #
    #   ./loop.sh boundary MARKER [MARKER...]
    #
    # Each MARKER is a string that MUST appear in the rebuilt binary — typically the env var or
    # event name of a fix being shipped. With no markers it refuses, because a boundary crossed
    # without verification is the exact failure this exists to prevent.
    shift || true
    if [ "$#" -eq 0 ]; then
      echo "refusing: name at least one MARKER that must be present in the rebuilt binary."
      echo "  e.g. ./loop.sh boundary GOOSE_SWARM_DETAIL_BUDGET_SECS detail_fallback"
      exit 2
    fi
    GOOSE_BIN="$HOME/Projects/goose/target/release/goose"
    BEFORE=$(stat -f '%m-%z' "$GOOSE_BIN" 2>/dev/null || echo none)
    echo "== 1. stopping the loop and the in-flight unit"
    touch STOP
    for P in $(pgrep -f 'goose swarm run'); do
      kill -9 -- "-$(ps -o pgid= -p "$P" | tr -d ' ')" 2>/dev/null || kill -9 "$P" 2>/dev/null
    done
    kill -9 "$(pgrep -f 'nodeloop/sweep.py' | head -1)" 2>/dev/null
    sleep 2
    echo "== 2. fleet must be IDLE before a rebuild (it shares this machine's CPU)"
    ~/.lmstudio/bin/lms ps --json 2>/dev/null | python3 -c "
import sys,json
d=json.load(sys.stdin)
busy=[i['identifier'] for i in d if i.get('status') not in ('idle',None)]
print('   busy:',busy or 'none')
sys.exit(1 if busy else 0)" || { echo "   fleet BUSY — not rebuilding. Re-run when idle."; exit 1; }
    echo "== 3. rebuilding (nice, so a resident node keeps its CPU)"
    ( cd "$HOME/Projects/goose" && . bin/activate-hermit >/dev/null 2>&1;       nice -n 5 cargo build --release -p goose-cli 2>&1 | tail -2 )
    echo "== 4. VERIFYING the markers actually shipped — compiling is not shipping"
    MISSING=0
    for M in "$@"; do
      printf "   %-38s " "$M"
      if strings "$GOOSE_BIN" 2>/dev/null | grep -qF -- "$M"; then echo present; else echo ABSENT; MISSING=1; fi
    done
    AFTER=$(stat -f '%m-%z' "$GOOSE_BIN" 2>/dev/null || echo none)
    echo "== engine_build $BEFORE -> $AFTER"
    if [ "$MISSING" -ne 0 ]; then
      echo "REFUSING to restart: a marker is missing from the binary. Fix it before crossing."
      exit 1
    fi
    if [ "$BEFORE" = "$AFTER" ]; then
      echo "REFUSING to restart: the binary did not change. Nothing was rebuilt."
      exit 1
    fi
    echo "== 5. boundary is safe to cross. Old results are now stale by engine_build and will NOT"
    echo "      be counted; park them (mv ../runs/nodeloop ../runs/nodeloop-preboundary-<n>) if you"
    echo "      want them out of the way, then: ./loop.sh start"
    ;;
  stop)
    touch STOP
    echo "STOP written — the loop exits after the current unit (results are kept)."
    ;;
  *)
    echo "usage: $0 {start|status|check|watch|results|abort|boundary|stop|resume}"; exit 2 ;;
esac

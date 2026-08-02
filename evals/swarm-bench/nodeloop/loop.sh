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
    # A check nobody runs is not a check. Refuse to launch a campaign whose arms cannot fire on this
    # binary — an absent lever produces a confident "no effect" that looks exactly like data.
    if ! "$0" preflight >/dev/null 2>&1; then
      echo "REFUSING to start — preflight failed:"; "$0" preflight; exit 1
    fi
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
    grep -E '^>>> |^    NEXT:|^\[(done|fail|retry|stop|grow|warn|abandon|abort|HARNESS|watch)' "$LOG" 2>/dev/null | tail -10
    ;;
  watch)
    # Failure signatures too: a monitor grepping only the happy path stays silent through a
    # crashloop, and silence looks exactly like "still running".
    tail -f "$LOG" | grep -E --line-buffered \
      '^>>> |^    NEXT:|^\[(done|fail|retry|stop|grow|warn|abandon|abort|HARNESS|watch)|Traceback|Error|Killed'
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
          and not r.get('abandoned') and not r.get('void')
          and r.get('harness_ok') is not False and r.get('score') is not None]
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
  preflight)
    # Can every QUEUED arm actually fire on the binary the loop will run? An arm whose env lever is
    # absent from the engine is byte-identical to baseline and records a confident "no effect" —
    # a fabricated null, and the most expensive kind of wrong answer because it looks like data.
    # This already happened: 34 hours were queued against a binary with no
    # GOOSE_SWARM_DETAIL_BUDGET_SECS. The watchdog now catches it at minute one; this catches it
    # before a single unit starts.
    python3 - <<'PREFLIGHT'
import importlib.util, sys, pathlib, subprocess
sys.path.insert(0, str(pathlib.Path('.').resolve()))
sys.path.insert(0, str(pathlib.Path('../bench').resolve()))
spec = importlib.util.spec_from_file_location("sweep", "sweep.py")
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
import run_build
out = subprocess.run(["strings", str(run_build.GOOSE)], capture_output=True, text=True).stdout
print(f"engine_build {m.engine_build()}")
bad = []
for a in m.arms_now():
    if not a["env"]:
        print(f"  {a['name']:<20} (no lever — baseline)")
        continue
    for var in a["env"]:
        ok = var in out
        print(f"  {a['name']:<20} {var:<36} {'present' if ok else 'ABSENT — arm cannot fire'}")
        if not ok:
            bad.append((a["name"], var))
if bad:
    print(f"\nFAIL: {len(bad)} arm(s) would record a fabricated 'no effect': {bad}")
    raise SystemExit(1)
print("\nevery queued arm can fire on this binary")
PREFLIGHT
    ;;
  selftest)
    # Adversarially audit the HARNESS itself. With a unit dir it also checks the invariants that
    # aggregate numbers cannot see; without one it runs each instrument's own controls.
    shift || true
    python3 "$PWD/selftest.py" "$@"
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
    # With no arguments the canonical list in ./MARKERS is used. That file exists because this list
    # used to live only in a prompt, and a marker dropped from a prompt is a fix that silently never
    # shipped — the exact failure this check exists to prevent. Arguments still override it.
    if [ "$#" -eq 0 ] && [ -f MARKERS ]; then
      OLDIFS=$IFS; IFS=$'\n'
      set -- $(grep -vE '^\s*(#|$)' MARKERS)
      IFS=$OLDIFS
      echo "== using $# marker(s) from ./MARKERS"
    fi
    if [ "$#" -eq 0 ]; then
      echo "refusing: name at least one MARKER that must be present in the rebuilt binary,"
      echo "  or list them in ./MARKERS (one per line)."
      echo "  e.g. ./loop.sh boundary GOOSE_SWARM_DETAIL_BUDGET_SECS detail_fallback"
      exit 2
    fi
    GOOSE_BIN="$HOME/Projects/goose/target/release/goose"
    BEFORE=$(stat -f '%m-%z' "$GOOSE_BIN" 2>/dev/null || echo none)
    # PRE-FLIGHT BEFORE ANYTHING IS KILLED. The marker check below is authoritative but runs after
    # the supervisor, the engine and the rebuild have all been spent — so a marker that is a comment,
    # a fn name, or a typo refuses a perfectly correct binary and there is nothing left to salvage.
    # THREE times that happened. preflight.py decides the same question from source, in seconds, with
    # the fleet still running, so the only reason to reach step 1 is that the answer was yes.
    # Running it here rather than trusting myself to remember is what stops the fourth occurrence.
    if [ -f preflight.py ]; then
      echo "== 0. boundary pre-flight (source-level; nothing killed yet)"
      if ! python3 preflight.py; then
        echo
        echo "refusing to cross: a marker above would report ABSENT on a CORRECT rebuild."
        echo "  nothing has been stopped — the run and the fleet are untouched. Fix MARKERS, re-run."
        exit 3
      fi
    fi
    echo "== 1. stopping the loop and the in-flight unit"
    touch STOP
    # SUPERVISOR FIRST, THEN THE ENGINE. Killing the engine first leaves the sweep alive for the
    # moment it takes to notice, and it does exactly what it is built to do: score the half-finished
    # tree and write a result. MEASURED — a unit killed at 57 minutes was recorded score=0.2999,
    # void=false, aborted=false, indistinguishable from a completed run, and it went straight into the
    # results table as a clean 1-node row. A truncated run that looks finished is worse than no row.
    kill -9 "$(pgrep -f 'nodeloop/sweep.py' | head -1)" 2>/dev/null
    sleep 1
    for P in $(pgrep -f 'goose swarm run'); do
      kill -9 -- "-$(ps -o pgid= -p "$P" | tr -d ' ')" 2>/dev/null || kill -9 "$P" 2>/dev/null
    done
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
    # cargo prints "Finished" and then places the artifact, so a marker check fired immediately
    # afterwards can read a partially-written Mach-O and report EVERY marker absent. MEASURED: the
    # first boundary run refused with all four markers ABSENT while `strings` on the same path a
    # moment later found all four. A false REFUSAL is the safe direction, but it is still a broken
    # check, and trusting it would have sent me to fix a defect that did not exist. Wait for the
    # binary to stop changing before reading it.
    echo "== 3b. waiting for the artifact to settle before reading it"
    LAST=""; STABLE=0
    for _ in $(seq 1 30); do
      CUR=$(stat -f '%m-%z' "$GOOSE_BIN" 2>/dev/null || echo none)
      if [ "$CUR" = "$LAST" ]; then STABLE=$((STABLE+1)); else STABLE=0; fi
      [ "$STABLE" -ge 2 ] && break
      LAST="$CUR"; sleep 1
    done
    echo "   settled at $CUR"
    echo "== 4. VERIFYING the markers actually shipped — compiling is not shipping"
    MISSING=0
    for M in "$@"; do
      printf "   %-38s " "$M"
      # NO PIPE. `grep -q` exits the moment it matches, which SIGPIPEs `strings`, and `set -o
      # pipefail` at the top of this script then reports the whole pipeline as failed — so a marker
      # that IS present reads ABSENT. MEASURED: this refused a boundary with all four markers
      # "missing" while `strings | grep` on the same path in an interactive shell found all four.
      # It would have refused every boundary forever, and the false direction was only safe by luck.
      # Process substitution keeps grep's exit status the only one that matters.
      if grep -qF -- "$M" <(strings "$GOOSE_BIN" 2>/dev/null); then echo present; else echo ABSENT; MISSING=1; fi
    done
    AFTER=$(stat -f '%m-%z' "$GOOSE_BIN" 2>/dev/null || echo none)
    echo "== engine_build $BEFORE -> $AFTER"
    # == 4b. RECLAIM DISK, at the only genuinely idle moment there is.
    #
    # MIN_FREE_GB=15 is a hard abort in the sweep's watchdog, so disk is an unattended failure mode:
    # MEASURED, free fell 56 -> 40 GB in forty minutes of build/test cycles, which was a scheduled
    # mass-abort a couple of hours out that would have looked like a mystery at 3am.
    #
    # The consumer is target/debug — cargo check/test grow it without bound (measured 64 GB) and the
    # sweep NEVER reads it; the engine runs target/release/goose. So debug is pure reclaim.
    #
    # DELIBERATELY NOT `goose-clean --go`, which removes EVERY target/ including the release binary
    # this boundary just built and verified. That skill's own guard refuses while a swarm is live, and
    # here it would be worse than useless: it would force a COLD release rebuild (tens of minutes of
    # fleet time) to reclaim space that was not scarce. This runs AFTER the rebuild and the marker
    # check precisely so the binary in flight is never a candidate.
    echo "== 4b. reclaiming regenerable build cache (debug only — release is the live binary)"
    FREE_BEFORE=$(df -g / | tail -1 | awk '{print $4}')
    rm -rf "$HOME/Projects/goose/target/debug" 2>/dev/null
    for T in "$HOME/Projects/goose/evals/swarm-gym/apps/polish-4-minimal/target" \
             "$HOME/Projects/goose/evals/swarm-gym/apps/lang-proof/target" \
             "$HOME/Projects/goose/evals/swarm-gym/overnight/vcs/target"; do
      rm -rf "$T" 2>/dev/null
    done
    sleep 2
    FREE_AFTER=$(df -g / | tail -1 | awk '{print $4}')
    # NOTHING HERE MAY EVER DELETE EVIDENCE. Asserted, not merely intended, because the deletion list
    # above is the kind of thing a future edit widens casually. Every one of these is a primary source
    # this project has already drawn a published conclusion from:
    #   runs/**                     run.jsonl, verdicts, vendor traces — every occupancy and score number
    #   ~/.local/state/goose/logs   llm_request.*.jsonl — how F87/F91 measured that 51% of every prompt
    #                               was inherited hints, and the ONLY way to test that prediction
    #   ~/.local/share/goose/sessions  per-worker tool traces
    #   nodeloop/FINDINGS.md        the findings themselves
    # Build output regenerates. None of this does.
    for KEEP in "$HOME/Projects/goose/evals/swarm-bench/runs" \
                "$HOME/.local/state/goose/logs" \
                "$HOME/.local/share/goose/sessions" \
                "$HOME/Projects/goose/evals/swarm-bench/nodeloop/FINDINGS.md"; do
      [ -e "$KEEP" ] || { echo "   !! EVIDENCE MISSING AFTER CLEANUP: $KEEP — refusing"; exit 3; }
    done
    if [ -x "$GOOSE_BIN" ]; then
      echo "   free ${FREE_BEFORE}G -> ${FREE_AFTER}G; live binary + all evidence intact"
    else
      echo "   !! LIVE BINARY MISSING AFTER CLEANUP — refusing to continue"; exit 3
    fi
    if [ "$MISSING" -ne 0 ]; then
      echo "REFUSING to restart: a marker is missing from the binary. Fix it before crossing."
      exit 1
    fi
    # The MARKERS are the evidence; "the binary changed" is only a proxy for them, and it goes wrong
    # on a repeated invocation — the second attempt sees the binary the FIRST one already rebuilt and
    # calls it unchanged, refusing a boundary that is in fact ready. An unchanged binary now WARNS.
    # A missing marker still refuses, because that is the failure this exists to prevent.
    if [ "$BEFORE" = "$AFTER" ]; then
      echo "   note: binary unchanged this invocation (already rebuilt); the markers verify it is current"
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
    echo "usage: $0 {start|status|check|selftest|preflight|watch|results|abort|boundary|stop|resume}"; exit 2 ;;
esac

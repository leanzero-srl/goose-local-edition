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
    # PARK A POPULATED RUN TREE BEFORE REUSING IT. Run directories are REUSED across units, so
    # `pkill` + `start` — as opposed to `boundary`, which parks — silently OVERWRITES the previous
    # unit's run.jsonl. MEASURED (F224): discarding a crippled arm and restarting destroyed every raw
    # observation behind three findings. That run was useless as a 3-node SCORE and was simultaneously
    # the only place a mechanism had ever been observed on the wire. Parking costs a copy.
    # ⚠⚠ THE PARK USED TO DESTROY WHAT IT EXISTED TO SAVE. `cp -R "$RUNDIR"/*/ "$PARK/"` copies each
    # source directory's CONTENTS, not the directory itself, so twelve unit directories collapsed into
    # ONE park directory and eleven were silently overwritten — last writer wins. MEASURED: all twelve
    # existing parks hold exactly ONE run.jsonl each, four run_ids appear on disk more than once as
    # duplicate copies of whichever unit sorted last, and `swarm-1node-r0`'s original log
    # (run_id swarm-20260803-100147948, the only 1-node observation this campaign had) is GONE —
    # overwritten in place by the new unit reusing that directory, and never parked because the park
    # had already discarded it.
    #
    # `cp -R "$RUNDIR" "$PARK"` copies the TREE. The trailing `/*/` was the whole bug.
    # F329 is the downstream symptom: those duplicate run_ids are what made one run look like four.
    # F830: REFUSE BEFORE PARKING. The watchdog's redundant start call raced a live
    # supervisor: the park ran first, duplicating the tree under a running engine while
    # the instance-lock refusal came only later. An already-running supervisor must end
    # the start attempt before ANY side effect.
    if [ -n "$(pid)" ]; then echo "already running (pid $(pid))"; exit 0; fi
    RUNDIR="$(cd "$(dirname "$0")" && pwd)/../runs/nodeloop"
    if [ -d "$RUNDIR" ] && [ -n "$(find "$RUNDIR" -mindepth 2 -name run.jsonl -print -quit 2>/dev/null)" ]; then
      PARK="$RUNDIR-parked-$(date +%s)"
      cp -R "$RUNDIR" "$PARK" 2>/dev/null
      # A park that saved fewer runs than the tree held is not a park. Count both and say so.
      SRC=$(find "$RUNDIR" -mindepth 2 -name run.jsonl 2>/dev/null | wc -l | tr -d ' ')
      DST=$(find "$PARK"   -mindepth 2 -name run.jsonl 2>/dev/null | wc -l | tr -d ' ')
      echo "parked previous run tree -> $(basename "$PARK")  ($DST of $SRC run logs)"
      [ "$DST" != "$SRC" ] && echo "  !! PARK LOST $((SRC-DST)) RUN LOG(S) — evidence loss, do NOT proceed blind"
    fi
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
      f"{'1-liners':>9} {'kind-mm%':>9} {'wall min':>9}  void")
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
print("1-liners = tasks whose description is a bare one-line brief: either their slice owner returned")
print("           no spec, or the synthesis splice matched no slice for that task.")
print("void = the engine did not build the pool the unit asked for; those are excluded, never averaged.")
print("A score delta below the replicate spread is not a result; read the mechanism columns.")
PY
    ;;
  check)
    # WATCHDOG ALARMS FIRST. The standing watchdog (launchd, watchdog.sh) appends here when
    # health goes BAD with nobody looking — the 18:35 death was silent for 75 minutes because
    # detection lived only in a check nobody ran. An un-cleared alarm is the loudest thing on
    # every tick until the operator acts and clears it; clearing without acting is the one move
    # this line cannot prevent and the one the discipline forbids.
    if [ -s ALARM ]; then
      echo "  🔴 WATCHDOG ALARM(S) PENDING — act, then clear with: rm ALARM"
      sed 's/^/     /' ALARM
    fi
    python3 "$PWD/health.py"
    # HELD-COMMIT PRESSURE, made visible every tick instead of left to my judgement.
    #
    # "Do not cross a boundary per fix — batch them" is a real rule and it rotted into an excuse:
    # 51 engine commits sat unshipped for most of a day while TWO arms and SEVEN registered
    # predictions were all blocked on that one crossing, because "let the running unit finish" got
    # treated as absolute when it was always a trade-off. The trade-off flips once the batch is
    # large — a killed unit costs ~2h of fleet time, an unshipped batch costs every experiment that
    # depends on it, plus every hour the engine runs without fixes that are already written.
    #
    # Judgement is exactly what failed here, so this does not ask for judgement. It counts.
    python3 - <<'HELD'
import subprocess, pathlib
ROOT = pathlib.Path.home() / "Projects/goose"
BIN = ROOT / "target/release/goose"
if not BIN.is_file():
    raise SystemExit(0)
built = int(BIN.stat().st_mtime)
r = subprocess.run(["git", "log", "--oneline", f"--since=@{built}", "--", "crates/"],
                   capture_output=True, text=True, cwd=str(ROOT))
held = [l for l in r.stdout.splitlines() if l.strip()]
if not held:
    print("  [OK] no engine commits held — the running binary is current")
    raise SystemExit(0)
n = len(held)
print(f"  {'[WARN]' if n < 8 else '[ACT] '} {n} engine commit(s) HELD — NOT in the running binary:")
for l in held[:4]:
    print(f"         {l}")
if n > 4:
    print(f"         ... and {n - 4} more")
if n >= 8:
    print("         CROSS THE BOUNDARY — at this size the batch costs more than the unit it kills.")
    print("         ./loop.sh boundary  (pre-flights first and refuses without killing anything)")
HELD
    # STALE SUPERVISOR. A running python process holds sweep.py as it was at launch, so editing
    # QUESTIONS/ARMS changes nothing until it restarts — an edit that is invisible to the thing it
    # was written for. MEASURED twice in twenty minutes: an arm set was queued and reordered while
    # the live sweep kept the old list, and the second time only caught it because the NEXT line in
    # the log still named the old arm. Same treatment as held commits: counted, not remembered.
    python3 - <<'STALE'
import subprocess, pathlib, time
HERE = pathlib.Path(__file__).resolve().parent if "__file__" in dir() else pathlib.Path(".")
src = pathlib.Path("sweep.py").resolve()
if not src.is_file():
    raise SystemExit(0)
pid = subprocess.run(["pgrep", "-f", "nodeloop/sweep.py"], capture_output=True, text=True).stdout.split()
if not pid:
    raise SystemExit(0)
# macOS `ps` has NO `etimes` — it errors with "keyword not found" and the check then silently did
# NOTHING, which is worse than not having it: a gate that prints neither verdict reads as a pass.
# Only `etime` exists, formatted [[dd-]hh:]mm:ss, so parse that.
et = subprocess.run(["ps", "-o", "etime=", "-p", pid[0]], capture_output=True, text=True).stdout.strip()
if not et:
    raise SystemExit(0)
days, _, clock = et.rpartition("-")
parts = [int(x) for x in clock.split(":")]
while len(parts) < 3:
    parts.insert(0, 0)
secs = parts[0] * 3600 + parts[1] * 60 + parts[2] + (int(days) * 86400 if days else 0)
started = time.time() - secs
if src.stat().st_mtime > started:
    print(f"  [ACT]  sweep.py was EDITED {int((src.stat().st_mtime - started)/60)} min AFTER the "
          f"running supervisor started (pid {pid[0]}).")
    print("         The live queue is the OLD one — your edit is invisible until it restarts.")
    print("         Restart it (supervisor BEFORE engine) or the arm you just added will never run.")
else:
    print("  [OK] the running supervisor is newer than sweep.py — the live queue is current")
STALE
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
# BENCH3: BENCH_-prefixed keys are HARNESS-side levers — read by run_build/score_build (python),
# never by the engine binary, so `strings <binary>` is the wrong oracle for them. Their oracle is
# the bench sources: a BENCH_ key nobody reads there is exactly as dead as an engine lever the
# binary lacks. (Measured 2026-08-14: the first amend_feature restart was refused on its three
# perfectly-live BENCH_ keys.)
bench_dir = pathlib.Path('../bench').resolve()
bench_src = "".join(f.read_text() for f in bench_dir.glob("*.py"))
print(f"engine_build {m.engine_build()}")
bad = []

# F708: AN ARM WITH reps>0 THAT NO QUESTION NAMES CAN NEVER BE SCHEDULED, and this used to be a
# WARNING printed on EVERY sweep pass. It went unread for weeks — and the two arms it named,
# e2e_oracle_off and spec_sized_plan, turned out to describe the two largest defects a five-agent
# audit later rediscovered from scratch. AN ALERT THAT FIRES EVERY ITERATION IS WALLPAPER; the
# refusal belongs at STARTUP, beside the lever-absent check, which exists for the same reason: a
# campaign that cannot run what it claims to schedule must not start.
named = {q["arm"] for q in m.QUESTIONS}
# ⚠ `reps` DEFAULTS TO 1, NOT 0 — an ARMS entry usually has no reps key at all (reps live in
# QUESTIONS), so a missing key means SCHEDULABLE. My first draft of this check wrote
# `a.get("reps", 0)`, which made it silently unable to fire on exactly the arms it exists to
# catch — the same "a check that cannot fire" defect this refusal was added to prevent, in the
# refusal itself. The predicate is copied verbatim from sweep.py's own orphan scan so the two
# cannot drift.
orphans = [a["name"] for a in m.arms_now() if a["name"] not in named and a.get("reps", 1) > 0]
for o in orphans:
    print(f"  {o:<20} {'reps>0 but named in NO question':<36} CANNOT EVER BE SCHEDULED")
    bad.append((o, "no question references this arm"))
# DUPLICATE NAMES — the other way an arm's reasoning goes missing, and the one that leaves no trace.
# MEASURED (F721): I added a `doc_fetch` arm and question for C10 without checking, and both already
# existed. The copy sorted FIRST, so every by-name lookup would have returned MY gate text and
# silently discarded the original's F389 correction — the one recording that the `/v1` readout is
# OVERTAKEN and would score an INERT arm as FIRED. A duplicate does not fail; it SHADOWS, and the
# thing it shadows is usually the correction someone added after being burned once already.
from collections import Counter as _C
for _label, _names in (("ARMS", [a["name"] for a in m.ARMS]),
                       ("QUESTIONS", [q["arm"] for q in m.QUESTIONS])):
    for _n, _c in _C(_names).items():
        if _c > 1:
            print(f"  {_n:<20} {'appears ' + str(_c) + 'x in ' + _label:<36} DUPLICATE — the first SHADOWS the rest")
            bad.append((_n, f"duplicated {_c}x in {_label}"))
for a in m.arms_now():
    if not a["env"]:
        print(f"  {a['name']:<20} (no lever — baseline)")
        continue
    for var in a["env"]:
        if var.startswith("BENCH_"):
            ok = var in bench_src
            print(f"  {a['name']:<20} {var:<36} "
                  f"{'harness-side: read by bench' if ok else 'BENCH_ key NOTHING reads — arm cannot fire'}")
        else:
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
      echo "  e.g. ./loop.sh boundary GOOSE_SWARM_SINK_SHARD slices_opened"
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

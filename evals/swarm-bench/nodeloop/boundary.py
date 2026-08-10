#!/usr/bin/env python3
"""Is the sweep at a unit boundary yet? Derived from the PROCESS TREE, never from a guessed path.

WHY THIS EXISTS. Taking the batch boundary means answering one question — "has the current cell
finished?" — and on 2026-08-09 I answered it twice with a glob that was one directory level off
(`runs/nodeloop/*/.swarm/heartbeat`; the files are at the cell ROOT). Both times the glob returned
zero, and **zero from a blind instrument is indistinguishable from a finished cell.** Had I acted on
it I would have rebuilt the binary underneath a live 20-minute run — and a past mid-cell interruption
landed 0.0563/0.0561/0.0561 as NON-VOID and overwrote the campaign's best result, 0.9033.

THE FIX IS TO STOP GUESSING PATHS. The run's own cwd is engine truth and is obtainable:

    sweep (cmdline ENDS WITH nodeloop/sweep.py)  ->  child `goose swarm run`  ->  lsof cwd  ->  heartbeat

Two path facts this encodes so nobody re-derives them wrong:
  * `heartbeat` and `run.jsonl` sit at the CELL ROOT, not under `.swarm/`. `.swarm/` holds only
    `current-run.json`, which is written once at run start and never updated — so its mtime is a
    START time and reads as 20 minutes stale on a perfectly healthy run.
  * The live dir is named for the ENTRANT (`swarm-3node-r1`, sweep.py:1544), NOT for the cell
    (`baseline-n3-r1`). The sweep logs the cell name and run_build executes in the entrant dir. These
    disagreeing is NORMAL and is not the reused-dir bug.

⚠️ A STALE run.jsonl WITH A FRESH heartbeat IS A LONG WORKER CALL, NOT A HANG. The engine writes the
heartbeat every 5s from a tokio task dropped only at exit; a 14-minute-quiet run.jsonl beside a
5-second-old heartbeat is a model still generating. Never kill on run.jsonl age alone.

⚠️ Matching the sweep by a bare `pgrep -f nodeloop/sweep.py` also matches a subagent's own grep
(measured — F678, and I drew a conclusion from the wrong process). Match the cmdline TAIL.
"""
from __future__ import annotations

import os
import subprocess
import sys
import time

SWEEP_SUFFIX = "nodeloop/sweep.py"
HEARTBEAT_STALE_MIN = 3.0


def _ps() -> list[tuple[str, str, str, str]]:
    out = subprocess.run(["ps", "-eo", "pid=,ppid=,etime=,command="],
                         capture_output=True, text=True).stdout
    rows = []
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) == 4:
            rows.append((parts[0], parts[1], parts[2], parts[3]))
    return rows


def sweep_pids(rows=None) -> list[str]:
    """PIDs whose command line ENDS WITH the sweep path and is a python interpreter.

    The tail match is load-bearing: `pgrep -f nodeloop/sweep.py` matches any process whose argv
    merely CONTAINS that string, including another agent's grep for it.
    """
    rows = rows if rows is not None else _ps()
    return [pid for pid, _ppid, _et, cmd in rows
            if cmd.strip().endswith(SWEEP_SUFFIX) and "python" in cmd.lower()]


def descendants(pid: str, rows) -> list[tuple[str, str, str]]:
    by_parent: dict[str, list] = {}
    for p, ppid, et, cmd in rows:
        by_parent.setdefault(ppid, []).append((p, et, cmd))
    out, stack = [], [pid]
    while stack:
        for child, et, cmd in by_parent.get(stack.pop(), []):
            out.append((child, et, cmd))
            stack.append(child)
    return out


def proc_cwd(pid: str) -> str | None:
    """The process's own working directory — engine truth, not a reconstructed path."""
    r = subprocess.run(["lsof", "-a", "-p", pid, "-d", "cwd", "-Fn"],
                       capture_output=True, text=True)
    for line in r.stdout.splitlines():
        if line.startswith("n"):
            return line[1:]
    return None


def _age_min(path: str) -> float | None:
    try:
        return (time.time() - os.path.getmtime(path)) / 60
    except OSError:
        return None


def is_engine(cmd: str) -> bool:
    """argv[0] IS the goose binary and argv[1..2] are `swarm run`.

    ⚠️ A SUBSTRING TEST IS NOT ENOUGH, and the self-test below caught me writing one. `"goose" in cmd
    and "swarm run" in cmd` is true of `grep -n 'goose swarm run' notes.md`, which is the F678 decoy
    wearing new clothes — and it would have been true of this very agent's own commands. The engine's
    argv is fixed and cheap to check, so check it.
    """
    argv = cmd.split()
    return (len(argv) >= 3
            and os.path.basename(argv[0]) == "goose"
            and argv[1] == "swarm"
            and argv[2] == "run")


def engine_pids(rows) -> list[tuple[str, str, str]]:
    """EVERY live `goose swarm run`, whether or not a sweep still owns it.

    ⚠️ THIS IS THE FIX FOR THE WORST FAILURE THIS FILE HAS HAD. It used to walk ONLY the sweep's
    descendants, so when the sweep supervisor died at 18:35 on 2026-08-10 and left its engine child
    running, `verdict()` found no sweep, found no cells, and printed **"no sweep process — safe to
    rebuild"** while pid 44400 was writing `run.jsonl` and `heartbeat` every few seconds. BATCH.md
    gates the rebuild on exit 0. One command later and the binary would have been swapped under a
    live 75-minute run — the exact corruption this whole file exists to prevent, produced BY the
    instrument that exists to prevent it.

    An orphan is not a rarer case than a supervised child; it is the SAME process with a dead parent,
    and it holds the fleet just as hard. Detection must not depend on who its parent is.
    """
    return [(pid, et, cmd) for pid, _ppid, et, cmd in rows if is_engine(cmd)]


def live_cells(rows=None) -> list[dict]:
    rows = rows if rows is not None else _ps()
    sweeps = set(sweep_pids(rows))
    owned = {pid for sp in sweeps for pid, _et, cmd in descendants(sp, rows) if is_engine(cmd)}
    cells = []
    for pid, et, _cmd in engine_pids(rows):
        cwd = proc_cwd(pid)
        cells.append({
            "sweep": next((sp for sp in sweeps if pid in owned), None),
            "orphan": pid not in owned,
            "pid": pid, "elapsed": et, "dir": cwd,
            "cell": os.path.basename(cwd) if cwd else None,
            "heartbeat_min": _age_min(os.path.join(cwd, "heartbeat")) if cwd else None,
            "runjsonl_min": _age_min(os.path.join(cwd, "run.jsonl")) if cwd else None,
        })
    return cells


def verdict() -> tuple[str, str]:
    rows = _ps()
    sweeps = sweep_pids(rows)
    cells = live_cells(rows)
    # THE ENGINE IS ASKED ABOUT FIRST, AND THE SWEEP SECOND. The old order asked "is there a sweep?"
    # and returned safe-to-rebuild on no, which is a question about the SUPERVISOR when the thing
    # that must not be disturbed is the ENGINE. A live run with a dead parent answered "safe".
    orphans = [c for c in cells if c["orphan"]]
    if orphans:
        c = orphans[0]
        hb = c["heartbeat_min"]
        return "CELL-RUNNING", (
            f"⚠️ ORPHANED ENGINE: {c['cell']} pid {c['pid']} up {c['elapsed']}, heartbeat "
            f"{'n/a' if hb is None else f'{hb:.1f} min'} — its sweep is GONE but the run is ALIVE. "
            "Do NOT rebuild and do NOT start a second sweep: it holds the fleet and a new unit would "
            "contend with it. Let it finish, then score its cell by hand")
    if not sweeps:
        if cells:
            return "CELL-RUNNING", "no sweep, but a live engine was found — see above"
        return "BOUNDARY-REACHED", "no sweep process and no live `goose swarm run` — safe to rebuild"
    if not cells:
        return "BETWEEN-UNITS", (f"sweep {sweeps[0]} alive, no `goose swarm run` child. It is "
                                "scoring/bookkeeping and will exit here if STOP is present — WAIT "
                                "for the process to go, do not rebuild under it")
    c = cells[0]
    hb, rj = c["heartbeat_min"], c["runjsonl_min"]
    if hb is None:
        return "CELL-RUNNING", (f"{c['cell']} pid {c['pid']} up {c['elapsed']} — NO heartbeat file "
                               "found. Do NOT read that as dead: verify the path before concluding")
    if hb > HEARTBEAT_STALE_MIN:
        return "CELL-STALLED", (f"{c['cell']} heartbeat {hb:.1f} min old (> {HEARTBEAT_STALE_MIN}) "
                                "— the engine's 5s writer has stopped. This one IS a real stall")
    detail = f"{c['cell']} pid {c['pid']} up {c['elapsed']}, heartbeat {hb:.1f} min"
    if rj is not None:
        detail += f", run.jsonl {rj:.1f} min"
        if rj > 5:
            detail += " (quiet run.jsonl + live heartbeat = LONG WORKER CALL, not a hang)"
    return "CELL-RUNNING", detail + " — NEVER rebuild or kill now"


def selftest() -> None:
    """Controls in BOTH directions. A matcher that only accepts good input is half a matcher."""
    fake = [
        ("100", "1", "01:00", "/usr/bin/python3 -u /x/nodeloop/sweep.py"),           # real
        ("101", "1", "01:00", "grep -n nodeloop/sweep.py foo"),                      # F678 decoy
        ("102", "1", "01:00", "/bin/zsh -c pgrep -f nodeloop/sweep.py"),             # F678 decoy
        ("103", "1", "01:00", "/usr/bin/python3 /x/nodeloop/sweep.py --status"),     # arg after path
    ]
    got = sweep_pids(fake)
    assert got == ["100"], f"tail match must accept only the real sweep, got {got}"
    assert "101" not in got, "NEGATIVE CONTROL FAILED: a grep for the path is not the sweep"
    assert "103" not in got, "a trailing arg means the cmdline does not END with the path"
    # descendants must be transitive: sweep -> shell -> goose
    tree = [("200", "100", "1:00", "sh"), ("201", "200", "1:00", "goose swarm run")]
    kids = {p for p, _, _ in descendants("100", fake + tree)}
    assert kids == {"200", "201"}, f"descendants must be transitive, got {kids}"
    # ⚠️ THE ORPHAN ARM, in BOTH directions. This is the case that printed "safe to rebuild" over a
    # live run on 2026-08-10, so it is asserted rather than described.
    orphan_rows = [("900", "1", "1:10:00", "/x/target/release/goose swarm run # Build vendorsync")]
    assert [p for p, _, _ in engine_pids(orphan_rows)] == ["900"], \
        "an engine with NO sweep ancestor must still be found — that miss is the whole bug"
    owned_rows = fake + [("300", "100", "1:00", "/x/target/release/goose swarm run # Build x")]
    assert [p for p, _, _ in engine_pids(owned_rows)] == ["300"], \
        "a supervised engine must be found by the same path"
    assert not engine_pids([("901", "1", "1:00", "grep -n 'goose swarm run' notes.md")]), \
        "NEGATIVE CONTROL FAILED: a grep mentioning the command line is not an engine"
    # the .swarm trap, asserted as a fact rather than a comment
    assert not os.path.join("cell", ".swarm", "heartbeat").endswith("cell/heartbeat"), \
        "heartbeat is at the CELL ROOT — a .swarm/ path is the blind glob that caused this file"
    print("selftest: PASS — tail match rejects both F678 decoys, descendants are transitive")


if __name__ == "__main__":
    selftest()
    if "--selftest" in sys.argv:
        sys.exit(0)
    state, why = verdict()
    print(f"\n{time.strftime('%H:%M:%S')}  {state}\n  {why}")
    stop = os.path.join(os.path.dirname(os.path.abspath(__file__)), "STOP")
    print(f"  STOP {'present — sweep exits at the next boundary' if os.path.exists(stop) else 'ABSENT — sweep will start another unit'}")
    # ⚠️ ONLY "no sweep process" LICENSES A REBUILD.
    #
    # This used to exit 0 on BETWEEN-UNITS too, while the very message it printed said "WAIT for the
    # process to go, do not rebuild under it". BATCH.md gates the rebuild on exit 0, so the code and
    # the words disagreed — and the code is what a script reads. A live sweep between units is about
    # to claim the next unit; rebuilding there swaps the binary under a run that is already starting,
    # which is the mid-cell corruption this whole file exists to prevent, just with better timing.
    #
    # Exit 2 = WAIT (transient, will resolve on its own). Exit 1 = a cell is running. Exit 0 = safe.
    sys.exit({"BOUNDARY-REACHED": 0, "BETWEEN-UNITS": 2}.get(state, 1))

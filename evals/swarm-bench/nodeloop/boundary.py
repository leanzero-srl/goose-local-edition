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


def live_cells(rows=None) -> list[dict]:
    rows = rows if rows is not None else _ps()
    cells = []
    for sp in sweep_pids(rows):
        for pid, et, cmd in descendants(sp, rows):
            if "goose" not in cmd or "swarm run" not in cmd:
                continue
            cwd = proc_cwd(pid)
            cells.append({
                "sweep": sp, "pid": pid, "elapsed": et, "dir": cwd,
                "cell": os.path.basename(cwd) if cwd else None,
                "heartbeat_min": _age_min(os.path.join(cwd, "heartbeat")) if cwd else None,
                "runjsonl_min": _age_min(os.path.join(cwd, "run.jsonl")) if cwd else None,
            })
    return cells


def verdict() -> tuple[str, str]:
    rows = _ps()
    sweeps = sweep_pids(rows)
    if not sweeps:
        return "BOUNDARY-REACHED", "no sweep process — safe to rebuild"
    cells = live_cells(rows)
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

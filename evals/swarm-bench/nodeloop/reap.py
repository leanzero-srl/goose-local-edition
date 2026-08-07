#!/usr/bin/env python3
"""Manual, dry-run-by-default entry point to the sweep's OWN orphan reaper.

THIS FILE DELIBERATELY IMPLEMENTS NOTHING. Its first draft was a second, independent reaper with a
different discriminator (command name vs working directory) and a different age floor (10 min vs 3 h)
— two versions of one rule, which is the failure mode this campaign keeps rediscovering. `sweep.py`
already owned the rule, with two guards mine did not have: it matches on the process's CWD being
inside `runs/` (so nothing of Mihai's can ever match) and it walks ppid to the sweep's root (so the
live engine, its shells and their children are protected by construction, however they are grouped).

What the first draft DID surface, and what went back into the real reaper rather than staying here,
is that its three-hour age floor was too slow for the commonest leak: the two pytest orphans burning
50 CPU-minutes during the cell they were corrupting were 55 and 48 minutes old. `orphan_age_secs`
now gives ppid-1 processes a ten-minute floor, because a reparented process has no waiter by
definition rather than by inference from age.

⚠️ A RUNNING SWEEP DOES NOT SEE THIS EDIT. The supervisor that has been up for a day is executing the
old function from memory, so between now and its next restart this script is the only path to the new
floor — which is exactly why a manual entry point is worth having at all.

    python3 reap.py            # report only
    python3 reap.py --kill     # actually signal
"""
import argparse
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)


def describe(pids: list[int]) -> None:
    if not pids:
        print("  (none)")
        return
    # `-p` takes a COMMA-SEPARATED list. Passing the pids as separate operands made macOS `ps` fall
    # back to a much wider listing: a kill of two bench orphans printed a block that included the
    # operator's own shells, the running Claude session and its MCP servers. Nothing was ever at risk
    # — the kill goes by process GROUP and those are in different groups — but a destructive tool that
    # displays processes it is not killing is a hazard on its own, because the next reader has no way
    # to tell the display from the target list.
    out = subprocess.run(
        ["ps", "-o", "pid,ppid,pgid,etime,time,command", "-p", ",".join(str(p) for p in pids)],
        capture_output=True, text=True).stdout
    shown = 0
    for line in out.splitlines():
        print("  " + line[:150])
        shown += 1
    if shown - 1 != len(pids):  # minus the header
        print(f"  ⚠ ps listed {shown - 1} row(s) for {len(pids)} pid(s) — do NOT trust this display")


WATCHDOG_SECS = 420  # the engine's own "agent stalled — no progress" bar; not a number I chose

# Kept minimal and LOCAL to the reporter below. The orphan RULE still lives in sweep.py and is not
# duplicated here (L302) — these only parse `ps` for a read-only report, and nothing here signals.
DENY = re.compile(r"sweep\.py|lmstudio|LM Studio|/lms\b|Claude|claude|/node\b|Electron|reap\.py")


def ps_rows() -> list[dict]:
    out = subprocess.run(["ps", "-eo", "pid,ppid,pgid,etime,time,command"],
                         capture_output=True, text=True, check=True).stdout.splitlines()[1:]
    rows = []
    for line in out:
        parts = line.split(None, 5)
        if len(parts) < 6 or not parts[0].isdigit():
            continue
        rows.append({"pid": int(parts[0]), "ppid": int(parts[1]), "pgid": int(parts[2]),
                     "etime": parts[3], "cpu": parts[4], "cmd": parts[5]})
    return rows


def etime_minutes(etime: str) -> float:
    """`ps` elapsed: [[dd-]hh:]mm:ss. Parsed, not eyeballed — a 24-hour process and a 24-second one
    differ by three characters."""
    days = 0
    if "-" in etime:
        d, etime = etime.split("-", 1)
        days = int(d)
    bits = [int(x) for x in etime.split(":")]
    while len(bits) < 3:
        bits.insert(0, 0)
    return days * 1440 + bits[0] * 60 + bits[1] + bits[2] / 60.0


def cpu_minutes(cputime: str) -> float:
    bits = cputime.replace(".", ":").split(":")
    if len(bits) == 3:      # mm:ss:frac
        return int(bits[0]) + int(bits[1]) / 60.0
    if len(bits) == 4:      # hh:mm:ss:frac
        return int(bits[0]) * 60 + int(bits[1]) + int(bits[2]) / 60.0
    return 0.0


def hung_children() -> list[dict]:
    """REPORT (never kill) worker-spawned processes that are hanging RIGHT NOW, mid-run.

    F502/L308: the ppid-1 discriminator this file's reaper relies on is USELESS DURING A RUN. The
    engine is one process hosting every worker as a tokio task, so a worker's child keeps the ENGINE
    as its parent until the run exits — it only becomes an orphan afterwards, long after the damage.
    Measured live: a `python3 -m pytest` holding port 8080 in `serve_forever`, 13.5 minutes old, its
    attempt abandoned 6.5 minutes earlier, and the reaper saw nothing at any age floor.

    THE AGE BAR IS THE ENGINE'S OWN NUMBER, not one I picked. A child older than `worker_timeout`'s
    420-second no-progress window has already outlived the point at which the engine would declare its
    worker stalled — so if a worker were waiting on it, that worker has been retried already. That
    makes 420s the principled bar rather than a tuned one.

    Reports only. Killing mid-cell changes the environment during a measurement, and the whole reason
    this exists is that I noted the behaviour and moved on instead of instrumenting it.
    """
    rows = ps_rows()
    root = os.path.join(os.path.dirname(HERE), "runs")
    try:
        out = subprocess.run(["lsof", "-d", "cwd", "-Fpn"], capture_output=True, text=True,
                             timeout=60).stdout
    except Exception:
        return []
    cwds: dict[int, str] = {}
    pid = None
    for line in out.splitlines():
        if line.startswith("p"):
            pid = int(line[1:]) if line[1:].isdigit() else None
        elif line.startswith("n") and pid is not None:
            cwds.setdefault(pid, line[1:])
    by_pid = {r["pid"]: r for r in rows}
    hung = []
    for p, cwd in cwds.items():
        r = by_pid.get(p)
        if not r or not cwd.startswith(root):
            continue
        if r["ppid"] == 1:
            continue  # already an orphan — the reaper above owns it
        if "goose" in r["cmd"] and "swarm run" in r["cmd"]:
            continue  # the engine itself lives in a run directory by design
        if DENY.search(r["cmd"]):
            continue
        age = etime_minutes(r["etime"]) * 60
        if age < WATCHDOG_SECS:
            continue
        ports = []
        try:
            li = subprocess.run(["lsof", "-p", str(p), "-a", "-i", "-nP"],
                                capture_output=True, text=True, timeout=20).stdout
            ports = sorted({w.split(":")[-1].split("(")[0]
                            for line in li.splitlines()[1:] for w in line.split()
                            if ":" in w and "LISTEN" in line})
        except Exception:
            pass
        hung.append({**r, "age_min": age / 60, "cpu_min": cpu_minutes(r["cpu"]), "ports": ports})
    return sorted(hung, key=lambda h: -h["age_min"])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--kill", action="store_true", help="actually signal; default is report only")
    ap.add_argument("--orphan-age-secs", type=int, default=600)
    args = ap.parse_args()

    import sweep  # noqa: E402 — path set above

    # Reported BEFORE signalling, so what was killed is auditable rather than asserted. The dry pass
    # also protects the operator from a change in the rule they did not expect: if this ever lists
    # something surprising, they see it before anything dies.
    doomed = sweep.reap_run_orphans(orphan_age_secs=args.orphan_age_secs, dry_run=True)
    print(f"=== ORPHANS UNDER {os.path.join(os.path.dirname(HERE), 'runs')} ({len(doomed)}) ===")
    describe(doomed)

    # Printed on EVERY invocation rather than behind a flag: a hung child is invisible to the orphan
    # scan for the whole run (F502), which is exactly the window in which it does its damage. A check
    # you have to remember to ask for is one you will not run on the night it matters.
    hung = hung_children()
    print(f"\n=== HUNG MID-RUN (older than the engine's own {WATCHDOG_SECS}s stall bar; "
          f"REPORTED, never killed) ({len(hung)}) ===")
    if not hung:
        print("  (none)")
    for h in hung:
        blocked = "BLOCKED" if h["cpu_min"] < 0.5 else f"SPINNING {h['cpu_min']:.1f} cpu-min"
        ports = f"  holds {','.join(h['ports'])}" if h["ports"] else ""
        print(f"  pid {h['pid']:<7} ppid {h['ppid']:<7} age {h['age_min']:6.1f}m  {blocked}{ports}")
        print(f"      {h['cmd'][:120]}")

    if not args.kill:
        print("\nDRY RUN — nothing signalled. Re-run with --kill.")
        return 0
    killed = sweep.reap_run_orphans(orphan_age_secs=args.orphan_age_secs)
    print(f"\nkilled {len(killed)} process(es): {sorted(killed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

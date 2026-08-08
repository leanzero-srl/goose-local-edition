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
import json
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


def live_run_log() -> tuple:
    """The RUNNING engine's own event log, resolved from its CWD.

    CELL NAME IS NOT APP ROOT (F588). `resolve_app_root` redirects a run out of its cell directory
    into `runs/nodeloop/swarm-<N>node-r<rep>`, and that tree is MOVED into the cell directory when
    the run ends — so `<cell>/run.jsonl` is stale for the whole life of the run and only becomes
    correct after it no longer matters. The engine's own working directory IS the app root, which
    needs no breadcrumb and cannot go stale while the process exists.

    Returns (path, None) or (None, reason). NEVER returns "clean" for "could not look" — L340.
    """
    live = [r for r in ps_rows() if "swarm run" in r["cmd"] and "/goose" in r["cmd"]]
    if not live:
        return None, "no engine running — nothing to join against"
    pid = live[0]["pid"]
    try:
        out = subprocess.run(["lsof", "-a", "-p", str(pid), "-d", "cwd", "-Fn"],
                             capture_output=True, text=True, timeout=15).stdout
    except (subprocess.SubprocessError, OSError) as exc:
        return None, f"could not read the engine's cwd ({exc}) — UNKNOWN, not clean"
    cwd = next((l[1:] for l in out.splitlines() if l.startswith("n/")), None)
    if not cwd:
        return None, "lsof gave no cwd for the engine — UNKNOWN, not clean"
    log = os.path.join(cwd, "run.jsonl")
    if not os.path.isfile(log):
        return None, f"engine cwd {cwd} has no run.jsonl yet — UNKNOWN, not clean"
    return log, None


def widowed_children(log_path: str, min_outlived_mins: float = 5.0) -> list:
    """Processes STILL ALIVE that were already running when a task reported DONE.

    MEASURED, AND THIS IS WHY IT EXISTS. `test-entry-validation` was dispatched at 15:04:43, its
    pytest started at 15:09:30, the task reported `task_completed status=done` at 15:16:06 after 683
    seconds — and that pytest was STILL BLOCKED at 15:44 when it was finally reaped, 25 minutes after
    its own task claimed success and well after the entire run had finished. This file's own
    docstring records two earlier pytest orphans at 55 and 48 minutes burning 50 CPU-minutes "during
    the cell they were corrupting". So a task's success report says nothing about its children, and
    nothing in this directory joined the two.

    ⚠️ THIS REPORTS A WINDOW OVERLAP, NOT A PROVEN PARENT LINK, and the wording matters more than
    usual: five attributions were wrong in one day, four of them because a plausible story was
    published before the marker that would have tested it. `ps` does not record who spawned a
    reparented process — ppid is 1 precisely because the parent is gone — so this can say "alive
    across a completion boundary" and must not say "leaked by task X".

    ⚠️ SCOPED TO THE BENCH, AND THE FIRST DRAFT WAS NOT. Filtering only on `ppid == 1` flagged 666
    processes on a live machine — every launchd daemon on the box — which is a check that reports
    everything and therefore reports nothing, the mirror image of a check that cannot fail. The
    filter is now the same discriminator `sweep.py`'s real reaper uses: the process must reference
    the bench runs directory, so nothing of Mihai's own can ever appear.

    ⚠️ KNOWN LIMITATION, STATED RATHER THAN HIDDEN: the path appears in the argv of the SHELL that a
    worker spawns, not in the argv of its grandchildren. The measured case is exactly that shape — a
    `bash -c cd <runs>/... && python3 -m pytest ...` whose own children carry no path — so this
    catches the shell that owns the leak while a grandchild reparented independently would be missed.
    """
    import datetime as dt
    rows, now = [], None
    comps = []
    try:
        for line in open(log_path, errors="replace"):
            if '"task_completed"' not in line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if e.get("event") != "task_completed":
                continue
            try:
                t = dt.datetime.fromisoformat(str(e.get("ts")).replace("Z", "+00:00")).timestamp()
            except (ValueError, TypeError):
                continue
            comps.append((t, e.get("task_id"), e.get("status")))
    except OSError:
        return []
    if not comps:
        return []
    now = dt.datetime.now().timestamp()
    runs_root = os.path.join(os.path.dirname(HERE), "runs")
    for r in ps_rows():
        # ppid 1 = reparented, so no waiter exists by definition rather than by inference from age.
        # The runs-root match is what keeps this from reporting every daemon on the machine.
        if r["ppid"] != 1 or "swarm run" in r["cmd"] or runs_root not in r["cmd"]:
            continue
        started = now - etime_minutes(r["etime"]) * 60
        after = [c for c in comps if c[0] > started]
        if not after:
            continue
        first = min(after)
        # A FLOOR, BECAUSE ppid==1 IS NORMAL DURING A LIVE RUN. F502/L308, recorded in this file's
        # own reaper notes: a worker's shell exits the instant it spawns its child, so the child is
        # reparented immediately and a healthy cell produces ppid-1 processes constantly. Measured
        # here: a real worker shell in the live cell's app root appeared in the first unfloored pass
        # and had already exited seconds later. The leak this exists to catch outlived its task's
        # completion by 25 MINUTES, and the two earlier pytest orphans were 55 and 48 minutes old, so
        # a five-minute floor separates the two populations by an order of magnitude. It is a
        # BLIND SPOT as well as a filter — a leak that dies at four minutes is invisible — which is
        # why it is a parameter the controls set to zero rather than a constant.
        if (now - first[0]) / 60.0 < min_outlived_mins:
            continue
        rows.append({"pid": r["pid"], "cmd": r["cmd"][:110],
                     "age_min": etime_minutes(r["etime"]),
                     "outlived_min": (now - first[0]) / 60.0,
                     "task": first[1], "status": first[2], "n_completions": len(after)})
    return sorted(rows, key=lambda x: -x["outlived_min"])


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
    # A task reporting DONE is not evidence its children are (L393). Printed unconditionally for the
    # same reason as the hung block: a check you must remember to ask for is one you will not run on
    # the night it matters.
    log, why = live_run_log()
    if log is None:
        print(f"\n=== OUTLIVED A COMPLETED TASK === UNKNOWN — {why}")
    else:
        widows = widowed_children(log)
        print(f"\n=== STILL ALIVE ACROSS A COMPLETION BOUNDARY "
              f"(window overlap, NOT a proven parent link) ({len(widows)}) ===")
        if not widows:
            print("  (none — every reparented process predates this run's first completion)")
        for w in widows:
            print(f"  pid {w['pid']:<7} age {w['age_min']:6.1f}m  outlived {w['outlived_min']:6.1f}m "
                  f"of completions  first-crossed '{w['task']}' (status={w['status']}, "
                  f"{w['n_completions']} completion(s) since)")
            print(f"      {w['cmd']}")

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

    # THE TIMING RULE IS NOW A GUARD, BECAUSE I JUST FAILED TO EXECUTE IT BY HAND.
    #
    # F506 decided: reap only BETWEEN units, never during one, because within-cell mess the engine
    # made is DATA and cross-cell survival is an ARTEFACT. I then fired it two minutes AFTER the next
    # cell had already started, because my landing signal — the scratch directory being reset — only
    # happens when the NEXT unit begins. The rule was right and my trigger was inherently late.
    #
    # A tempting simplification is that ppid-1 already encodes the distinction: F502 showed a live
    # worker's child stays parented to the engine for the whole run. THAT IS NOT SAFE. A worker that
    # launches `cmd &` — the exact pattern the sweep's own reaper docstring quotes — orphans its
    # child IMMEDIATELY, while its cell is still running. So a live cell really can produce ppid-1
    # processes, and the age floor alone would eventually kill one.
    #
    # The correct test is provenance, not age: a process that STARTED BEFORE the currently-live
    # engine cannot belong to it. That makes --kill safe at any moment and removes the timing rule
    # from my head, which is where it failed.
    live = [r for r in ps_rows() if "swarm run" in r["cmd"] and "/goose" in r["cmd"]]
    if live:
        engine_age = max(etime_minutes(r["etime"]) for r in live)
        by_pid = {r["pid"]: r for r in ps_rows()}
        younger = [p for p in doomed
                   if p in by_pid and etime_minutes(by_pid[p]["etime"]) < engine_age]
        if younger:
            print(f"\n🔴 REFUSING — {len(younger)} target(s) are YOUNGER than the live engine "
                  f"({engine_age:.1f}m old), so they may belong to the run in flight: {younger}")
            print("   Within-cell mess the engine made is DATA. Re-run once this cell has landed.")
            return 3
        print(f"\n(an engine is live, {engine_age:.1f}m old; every target predates it — cross-cell, "
              f"safe to clear)")
    killed = sweep.reap_run_orphans(orphan_age_secs=args.orphan_age_secs)
    print(f"\nkilled {len(killed)} process(es): {sorted(killed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

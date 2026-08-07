#!/usr/bin/env python3
"""Reap the processes a worker leaves behind when its attempt is abandoned.

MEASURED, and it is the reason this file exists: while `swarm-3node-r0` was scoring, the machine was
carrying SEVEN detached processes from this bench. Two were pytest runs spinning at 39% and 63% of a
core, for 53 and 46 minutes, having burned 20m49s and 28m51s of CPU between them. The other five were
`vendorsync` servers from EARLIER CELLS still holding ports 18002, 18098, 18099, 18766 and 18768 —
one of them five hours old.

The mechanism: a judge kill (or a stall retry) aborts the worker's tokio future, which cancels the
await but does NOT kill the OS process the worker's shell tool spawned. The child is reparented to
init and keeps running. The engine then reports "agent stalled — no progress for 420s" and restarts
the attempt, which runs the same command and creates ANOTHER orphan. `test-meridian-edge` did exactly
that twice, at run minutes 85.4 and 94.5, and the two orphans' start times match both to the minute.

Why this corrupts measurements and not just tidiness:

  * CPU CONTENTION. Every orphan competes with the LM Studio fleet on the same machine, so a later
    cell runs on a slower box than an earlier one — and the effect GROWS with node count, because
    more nodes mean more workers mean more orphans. That is an asymmetry pointing the same way as
    the result being measured, which is the worst kind.
  * PORT CAPTURE. Orphaned servers hold fixed ports for hours. A later cell whose tests bind the same
    port fails for a reason that has nothing to do with the code it is grading.

SAFETY, because this kills things on a machine that is also running a 24/7 fleet:

  * Dry run is the DEFAULT. `--kill` is required to actually signal anything.
  * NOTHING is killed unless it is detached (ppid 1). A live process still owned by the sweep, by a
    shell, or by the engine belongs to someone and is never touched.
  * An AGE FLOOR (default 10 min) spares anything young enough to plausibly be live work.
  * A hard DENY list protects the sweep itself, LM Studio, the scorer and this interpreter.
  * The process GROUP is signalled, not the process: `bash -c ... | tail` leaves a pipeline, and
    killing only the leader leaves the rest running (the unattended-loop lesson, applied to
    grandchildren for the first time).
  * It prints what it SPARED and why, not only what it killed. A reaper whose output is a kill list
    cannot be audited for over-reach, and over-reach here means killing the fleet.
"""
import argparse
import os
import re
import signal
import subprocess
import sys

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"

# Commands this bench's WORKERS spawn. Deliberately specific: a bare `python3` match would sweep up
# the sweep, the scorer and anything else the user is running.
WORKER_SPAWNED = re.compile(
    r"(?:^|[\s/])(?:pytest|vendorsync|uvicorn|http\.server|go\s+test|go\s+run|npm\s+(?:test|start))\b"
)

# Never signalled, whatever else matches. `sweep.py` is the campaign; `lms`/`LM Studio` is the fleet
# and its ~23GB of loaded weights; `Claude`/`node` covers the harness driving all of this.
DENY = re.compile(r"sweep\.py|lmstudio|LM Studio|/lms\b|Claude|claude|/node\b|Electron|reap\.py")


def ps_rows() -> list[dict]:
    out = subprocess.run(
        ["ps", "-eo", "pid,ppid,pgid,etime,time,command"],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()[1:]
    rows = []
    for line in out:
        parts = line.split(None, 5)
        if len(parts) < 6:
            continue
        pid, ppid, pgid, etime, cputime, cmd = parts
        try:
            rows.append({"pid": int(pid), "ppid": int(ppid), "pgid": int(pgid),
                         "etime": etime, "cpu": cputime, "cmd": cmd})
        except ValueError:
            continue
    return rows


def etime_minutes(etime: str) -> float:
    """`ps` elapsed time: [[dd-]hh:]mm:ss. Parsed rather than eyeballed — a 24-hour orphan and a
    24-second one differ by three characters."""
    days = 0
    if "-" in etime:
        d, etime = etime.split("-", 1)
        days = int(d)
    bits = [int(x) for x in etime.split(":")]
    while len(bits) < 3:
        bits.insert(0, 0)
    h, m, s = bits
    return days * 1440 + h * 60 + m + s / 60.0


def cpu_minutes(cputime: str) -> float:
    bits = cputime.replace(".", ":").split(":")
    if len(bits) == 3:  # mm:ss:frac
        return int(bits[0]) + int(bits[1]) / 60.0
    if len(bits) == 4:  # hh:mm:ss:frac
        return int(bits[0]) * 60 + int(bits[1]) + int(bits[2]) / 60.0
    return 0.0


def classify(rows: list[dict], min_age_min: float) -> tuple[list[dict], list[tuple[dict, str]]]:
    reap, spare = [], []
    for r in rows:
        cmd = r["cmd"]
        if not WORKER_SPAWNED.search(cmd):
            continue  # not something this bench's workers spawn — not our business at all
        if DENY.search(cmd):
            spare.append((r, "protected: fleet / campaign / harness"))
            continue
        if r["pid"] == os.getpid():
            spare.append((r, "this reaper"))
            continue
        if r["ppid"] != 1:
            spare.append((r, f"still owned by pid {r['ppid']} — someone is waiting on it"))
            continue
        age = etime_minutes(r["etime"])
        if age < min_age_min:
            spare.append((r, f"only {age:.1f} min old (floor {min_age_min:.0f}) — may be live work"))
            continue
        r["age_min"] = age
        r["cpu_min"] = cpu_minutes(r["cpu"])
        reap.append(r)
    return reap, spare


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--kill", action="store_true",
                    help="actually signal; without it this only reports")
    ap.add_argument("--min-age-min", type=float, default=10.0)
    args = ap.parse_args()

    rows = ps_rows()
    reap, spare = classify(rows, args.min_age_min)

    # THE KILL UNIT IS THE GROUP, SO THE REPORT MUST BE THE GROUP. The first draft of this file listed
    # only the matched leaders and reported "0.1 CPU-minutes burned" — the real figure was ~50, because
    # every `bash -c ... | tail` leader idles at zero while the pytest child inside its group spins. A
    # reaper that understates the damage 500x argues for its own uselessness, and one that names a pgid
    # without listing that group's members is asking to be trusted about what it is about to kill.
    by_pgid: dict[int, list[dict]] = {}
    for r in rows:
        by_pgid.setdefault(r["pgid"], []).append(r)
    doomed_pgids = sorted({r["pgid"] for r in reap})
    protected = [r for r in rows if DENY.search(r["cmd"]) and r["pgid"] in doomed_pgids]

    print(f"=== SPARED ({len(spare)}) ===")
    for r, why in spare:
        note = " (⚠ but its GROUP is doomed — the group kill takes it)" if r["pgid"] in doomed_pgids else ""
        print(f"  pid {r['pid']:<7} {r['etime']:>11}  {why}{note}\n      {r['cmd'][:110]}")

    print(f"\n=== GROUPS TO BE KILLED ({len(doomed_pgids)}) ===")
    total_cpu = 0.0
    for pgid in doomed_pgids:
        members = by_pgid.get(pgid, [])
        gcpu = sum(cpu_minutes(m["cpu"]) for m in members)
        total_cpu += gcpu
        print(f"  pgid {pgid:<7} {len(members)} member(s), {gcpu:.1f} CPU-minutes burned")
        for m in members:
            print(f"      pid {m['pid']:<7} age {etime_minutes(m['etime']):7.1f}m  "
                  f"cpu {cpu_minutes(m['cpu']):6.1f}m  {m['cmd'][:92]}")
    print(f"\n  {len(reap)} orphan leaders in {len(doomed_pgids)} groups, "
          f"{total_cpu:.1f} CPU-minutes already burned")

    if protected:
        print("\n🔴 ABORT — a PROTECTED process shares a doomed process group. Killing the group would "
              "take the fleet or the campaign with it:")
        for r in protected:
            print(f"      pid {r['pid']} pgid {r['pgid']}  {r['cmd'][:100]}")
        return 3

    if not args.kill:
        print("\nDRY RUN — nothing signalled. Re-run with --kill.")
        return 0
    if not reap:
        return 0
    # Group-kill, newest signal last: TERM the group, and leave KILL to a second pass rather than
    # escalating blind — a pytest mid-write to the cell tree should get its chance to unwind.
    killed = 0
    for pgid in sorted({r["pgid"] for r in reap}):
        try:
            os.killpg(pgid, signal.SIGTERM)
            killed += 1
            print(f"  SIGTERM -> process group {pgid}")
        except ProcessLookupError:
            print(f"  process group {pgid} already gone")
        except PermissionError:
            print(f"  ⚠ refused process group {pgid} (not ours)")
    print(f"\nsignalled {killed} process groups")
    return 0


if __name__ == "__main__":
    sys.exit(main())

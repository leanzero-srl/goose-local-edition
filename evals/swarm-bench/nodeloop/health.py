#!/usr/bin/env python3
"""Is the loop actually working, or only alive? Exit 0 OK, 1 WARN, 2 BAD.

"Still running" is not health. Every check below names a real way this loop has wasted, or could
waste, hours of fleet time while looking perfectly fine in the log. Each one prints the evidence
it judged on, so a verdict can be argued with rather than trusted.

BAD means stop and fix — do not wait for the current unit to finish.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "runs" / "nodeloop"
LOG = OUT / "loop.log"
STOP = HERE / "STOP"

# A unit is a full swarm build. Measured walls on this fleet are 1.9-2.5h, so these are generous
# on purpose: a threshold tighter than the work it watches produces false alarms, and a loop that
# cries wolf gets ignored.
HEARTBEAT_STALE_SECS = 600        # engine writes every 5s; 10 min dead means wedged, not busy
NO_PROGRESS_SECS = 4 * 3600       # no unit finished in 4h when units take ~2h
MIN_FREE_GB = 15                  # goose's target/ and per-run trees fill a disk quietly
RECENT_FAILS_BAD = 2              # two consecutive failed/timed-out units is systematic


class Report:
    def __init__(self) -> None:
        self.lines: list[tuple[str, str]] = []

    def add(self, level: str, msg: str) -> None:
        self.lines.append((level, msg))

    @property
    def worst(self) -> int:
        return max([{"OK": 0, "WARN": 1, "BAD": 2}[lv] for lv, _ in self.lines] or [0])

    def render(self) -> str:
        out = [f"  [{lv}] {msg}" for lv, msg in self.lines]
        verdict = {0: "OK", 1: "WARN", 2: "BAD"}[self.worst]
        head = f"nodeloop health: {verdict}   {time.strftime('%H:%M:%S')}"
        tail = ("\n  -> STOP THE LOOP AND FIX. Do not wait for the current unit."
                if self.worst == 2 else "")
        return "\n".join([head, *out]) + tail


def pgrep(pattern: str) -> list[int]:
    try:
        r = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True, timeout=15)
        return [int(p) for p in r.stdout.split() if p.strip().isdigit()]
    except Exception:
        return []


def results() -> list[dict]:
    rs = []
    for f in OUT.glob("*/nodeloop-result.json"):
        try:
            r = json.loads(f.read_text())
            r["_mtime"] = f.stat().st_mtime
            rs.append(r)
        except Exception:
            continue
    return sorted(rs, key=lambda r: r["_mtime"])


def main() -> int:
    rep = Report()
    stopping = STOP.is_file()
    loop_pids = pgrep("nodeloop/sweep.py")
    engine_pids = pgrep("goose swarm run")

    # 1. The loop process. Absent without a STOP sentinel means it died, and a dead loop reports
    #    nothing at all — the failure mode that looks identical to a quiet night.
    if loop_pids:
        rep.add("OK", f"loop alive pid={loop_pids[0]}"
                      + (" (STOP present — will exit after this unit)" if stopping else ""))
    elif stopping:
        rep.add("OK", "loop stopped on its STOP sentinel (intentional)")
    else:
        rep.add("BAD", "loop process is GONE and no STOP sentinel was set — it died")

    # 2. The engine. A loop with no engine and no recent completion is spinning, not working.
    rs = results()
    last_done = max((r["_mtime"] for r in rs), default=None)
    since_done = (time.time() - last_done) if last_done else None
    if engine_pids:
        rep.add("OK", f"engine running pid={engine_pids[0]}")
    elif loop_pids and since_done is not None and since_done < 600:
        rep.add("OK", "no engine right now — a unit finished in the last 10 min")
    elif loop_pids:
        rep.add("WARN", "loop is alive but NO engine is running — it may be between units")

    # 3. Heartbeat. The engine writes it every 5s from a task killed only on Drop, so a stale
    #    heartbeat under a live engine means wedged. `lms ps` showing GENERATING is NOT a
    #    contradiction: a busy fleet under PARALLEL:1 still heartbeats.
    beats = sorted(OUT.glob("*/heartbeat"), key=lambda p: p.stat().st_mtime, reverse=True)
    if engine_pids:
        if not beats:
            rep.add("WARN", "engine running but no heartbeat file found yet")
        else:
            age = int(time.time() - beats[0].stat().st_mtime)
            if age > HEARTBEAT_STALE_SECS:
                rep.add("BAD", f"heartbeat {age}s stale under a LIVE engine — the run is wedged "
                               f"({beats[0]})")
            else:
                rep.add("OK", f"heartbeat {age}s old")

    # 4. Progress. Units take ~2h; nothing finishing in 4h means stuck, not slow.
    if since_done is None:
        started = LOG.stat().st_mtime if LOG.is_file() else time.time()
        waited = int(time.time() - started)
        lvl = "BAD" if waited > NO_PROGRESS_SECS else "OK"
        rep.add(lvl, f"no unit has finished yet ({waited // 60} min since the loop started)")
    else:
        lvl = "BAD" if since_done > NO_PROGRESS_SECS else "OK"
        rep.add(lvl, f"last unit finished {int(since_done) // 60} min ago ({len(rs)} on disk)")

    # 5. Systematic failure. Two consecutive dead units means the next ten will die the same way.
    recent = rs[-RECENT_FAILS_BAD:]
    if len(recent) == RECENT_FAILS_BAD and all(
            r.get("failed") or r.get("timed_out") for r in recent):
        how = ", ".join(f"{r['arm']}r{r['rep']}="
                        + ("failed" if r.get("failed") else "timed_out") for r in recent)
        rep.add("BAD", f"the last {RECENT_FAILS_BAD} units all died the same way ({how})")

    # 6. A flat timeout measures the timeout, never the entrant. Any timeout is a WARN because the
    #    score it produced says nothing about the swarm and must not be averaged in.
    tos = [r for r in rs if r.get("timed_out")]
    if tos:
        rep.add("WARN", f"{len(tos)} unit(s) hit the cap — their scores measure the cap, not the "
                        f"swarm: {[r['arm'] + 'r' + str(r['rep']) for r in tos]}")

    # 7. Comparability. The fleet changing under the loop voids cross-arm comparison, and it has
    #    silently degraded before (three runs labelled 1/2/3-node all ran on one device).
    pools = {r.get("actual_nodes") for r in rs if r.get("actual_nodes") is not None}
    if len(pools) > 1:
        rep.add("BAD", f"pool size CHANGED across units {sorted(pools)} — arms are no longer "
                       f"comparable, the fleet moved under the loop")
    elif pools:
        rep.add("OK", f"pool size stable at {pools.pop()} device(s) across {len(rs)} unit(s)")

    # 8. A broken instrument reports zeros, and a zero from a blind probe is a fabricated result.
    audited = [r for r in rs if isinstance(r.get("audit"), dict) and r["audit"].get("dispatches")]
    if rs and not audited:
        rep.add("BAD", "no unit produced a dispatch audit — the instrument is blind, so every "
                       "number it emits is unearned")
    elif audited:
        errs = [r for r in audited if r["audit"].get("audit_error")]
        if errs:
            rep.add("WARN", f"{len(errs)} unit(s) had an audit error")

    # 9. Disk. goose's target/ has reached 190GB before and a full disk kills a run mid-write.
    free_gb = shutil.disk_usage(os.path.expanduser("~")).free / 1e9
    lvl = "BAD" if free_gb < MIN_FREE_GB else ("WARN" if free_gb < MIN_FREE_GB * 2 else "OK")
    rep.add(lvl, f"{free_gb:.0f} GB free on the home volume")

    # 10. Strays. An orphaned engine contends for the one addressable worker and skews everything
    #     after it, silently — measured at 33 minutes once.
    if len(engine_pids) > 1:
        rep.add("BAD", f"{len(engine_pids)} engines running at once {engine_pids} — an orphan is "
                       f"contending for the single addressable worker")

    print(rep.render())
    return rep.worst


if __name__ == "__main__":
    sys.exit(main())

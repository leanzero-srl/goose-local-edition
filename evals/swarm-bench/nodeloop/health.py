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

import sweep          # engine_build() — provenance from the row, never from file mtime (F338)

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

    # 5b. A UNIT THAT DIES BEFORE IT STARTS SETS NEITHER `failed` NOR `timed_out`, so check 5 above
    #     cannot see it — and that blindness let a dead fleet consume most of a backlog while this
    #     script printed OK.
    #
    #     MEASURED 2026-08-05: at 08:03:59 all three LM Studio nodes went from GENERATING to no
    #     models loaded (fleet-samples.tsv), LM Link still connected. Every unit after that returned
    #     in ~0.2s with score 0.0, `actual_pool: None`, no run log at all. In twenty minutes the
    #     sweep burned through 79 units of backlog — reps 8, 9, 10, 11 across every lever arm —
    #     recording a confident 0.0 for each. `./loop.sh check` said OK the whole time, because
    #     "process alive + a unit finished recently" is exactly what a fleet outage looks like from
    #     here: units finish FASTER than ever.
    #
    #     The scorer already knew. Every one of those rows carries `harness_ok: False` and the loop
    #     log says verbatim "the numbers from this unit are NOT evidence". The verdict existed and
    #     this check simply did not read it. That is the third alarm defect today — F330 rang above
    #     the line it guarded, F339 could never clear, and this one could never fire.
    # F817: a row voided as an OPERATOR INCIDENT is already marked, ledgered and acted on — it is
    # the void mechanism working, exactly like F795b's boundary-STOP class. Feeding 141 such rows
    # back through these detectors raised BAD every 2 minutes for hours after the incident was
    # closed — the unclearable-alarm failure documented below, third instance. Excluded from every
    # retrospective detector; NOT from anything that watches the live system.
    def op_incident(r: dict) -> bool:
        # "kill artifact" joined 2026-08-15 (F830): the third void class that is already
        # acted-on by construction — alarming on it re-created the unclearable alarm within
        # minutes of the class being minted.
        reason = str(r.get("void_reason", ""))
        return bool(r.get("void")) and ("operator incident" in reason
                                        or "kill artifact" in reason)

    recent_dead = [r for r in rs[-8:] if r.get("harness_ok") is False and not op_incident(r)]
    if recent_dead:
        rep.add("BAD", f"{len(recent_dead)} of the last {min(len(rs), 8)} unit(s) recorded "
                       f"harness_ok=False — the scorer says their numbers are NOT evidence: "
                       + "; ".join(f"{r.get('arm')}-n{r.get('nodes')}-r{r.get('rep')}"
                                   for r in recent_dead[:5]))

    #     And the shape that produced it, stated independently of the flag: a unit is a full build
    #     measured in HOURS. Anything returning in under a minute did not run, whatever it recorded.
    instant = [r for r in rs[-8:] if (r.get("wall_secs") or 0) < 60 and not r.get("aborted")
               and not op_incident(r)]
    if len(instant) >= 2:
        rep.add("BAD", f"{len(instant)} of the last {min(len(rs), 8)} unit(s) finished in under 60s "
                       f"(a unit takes ~2h) — they never ran; check the fleet has models loaded "
                       f"(`lms ps`) before anything else")

    # 6. A flat timeout measures the timeout, never the entrant. Any timeout is a WARN because the
    #    score it produced says nothing about the swarm and must not be averaged in.
    tos = [r for r in rs if r.get("timed_out")]
    if tos:
        rep.add("WARN", f"{len(tos)} unit(s) hit the cap — their scores measure the cap, not the "
                        f"swarm: {[r['arm'] + 'r' + str(r['rep']) for r in tos]}")

    # 7. Comparability. Pool size now VARIES BY DESIGN — the sweep runs 1, 2 and 3-node cells — so
    #    variety is expected and only a mismatch between what a unit asked for and what the engine
    #    actually built is a defect. That mismatch is what made three runs labelled 1/2/3-node all
    #    measure the same 1-device pool, so it is checked per unit, never in aggregate.
    #    ⚠ SCOPE THE ALARM TO THE CURRENT ENGINE, OR IT BECOMES A STANDING FALSE POSITIVE.
    #    A void row is PERMANENT: `kind_prompt-n3-r0` and `scoped_contracts-n3-r0` ran on
    #    2026-08-03 against a binary rebuilt on 2026-08-04, were CORRECTLY voided at the time, and
    #    then tripped this check on every subsequent tick — printing BAD and
    #    "-> STOP THE LOOP AND FIX. Do not wait for the current unit." forever, while all four
    #    curve cells had pool 3/3 and the live 1-node cell had worker_count 1.
    #
    #    An unattended alarm that can never clear is worse than no alarm: it orders a halt that is
    #    always wrong, and the first time it is RIGHT nobody will believe it. Same failure as F325's
    #    stall detector, which read "not significant" under a real win because its test had an
    #    unreachable branch.
    #
    #    Scoped by `engine_build`, which the row carries and which no file copy can alter — NOT by
    #    file mtime, which `cp -R` rewrites (F338/L183).
    cur = sweep.engine_build()
    # F784/F795b: a row the sweep itself voided at a boundary STOP is the void MECHANISM WORKING,
    # not a pool shortfall — alarming on it re-creates the exact unclearable-alarm failure this
    # comment block documents (measured: 100+ BAD ticks over 3.5 hours for one correctly-voided
    # kill victim, drowning every real alarm the whole time).
    voids = [r for r in rs
             if r.get("void") and r.get("engine_build") == cur
             and "boundary STOP" not in str(r.get("void_reason", ""))
             and not op_incident(r)]
    stale_voids = [r for r in rs if r.get("void") and r.get("engine_build") != cur]
    if voids:
        rep.add("BAD", "unit(s) did not get the pool they asked for: "
                       + "; ".join(f"{r['arm']}-n{r.get('nodes')}-r{r.get('rep')} "
                                   f"({r.get('void_reason')})" for r in voids))
    if stale_voids:
        rep.add("OK", f"{len(stale_voids)} void row(s) from EARLIER engine builds, correctly "
                      f"excluded and not actionable: "
                      + "; ".join(f"{r['arm']}-n{r.get('nodes')}-r{r.get('rep')}"
                                  for r in stale_voids))
    elif rs:
        # A FAILED unit has no pool at all, so `nodes`/`actual_nodes` are None — and sorting a set
        # containing None against an int raises TypeError, which crashed the whole health check on the
        # first failed unit. The health check is what tells an unattended operator the sweep is sick;
        # it must never be the thing that dies when something goes wrong.
        got = sorted(
            {(r.get("nodes"), r.get("actual_nodes")) for r in rs},
            key=lambda t: tuple(-1 if v is None else v for v in t),
        )
        rep.add("OK", f"every unit got the pool it asked for {got}")

    # 8. A broken instrument reports zeros, and a zero from a blind probe is a fabricated result.
    #
    # BUT A ZERO HAS TWO CAUSES AND THEY CALL FOR OPPOSITE RESPONSES, and this check could not tell
    # them apart. MEASURED: it reported "the instrument is blind, so every number it emits is
    # unearned" — the loudest verdict it has — about two units that had simply DIED BEFORE
    # DISPATCHING. One was cut loose at 24 minutes still inside the skeleton draft; the other failed
    # at 0 minutes on a port collision. Both produced `dispatches: 0`, and both were correct to.
    #
    # A false BAD is not a harmless over-warning. This check exists so an unattended operator can
    # trust one line, and a check that cries blind at every early death trains its reader to ignore
    # it — which is exactly when the real blindness would slip through. So: only a unit that RAN TO
    # COMPLETION can indict the instrument.
    finished = [r for r in rs if not r.get("aborted") and r.get("score") is not None]
    audited = [r for r in rs if isinstance(r.get("audit"), dict) and r["audit"].get("dispatches")]
    if finished and not audited:
        rep.add("BAD", "no COMPLETED unit produced a dispatch audit — the instrument is blind, so "
                       "every number it emits is unearned")
    elif rs and not audited:
        dead = len(rs) - len(finished)
        rep.add("WARN", f"no dispatch audit yet, but {dead} of {len(rs)} unit(s) died before "
                        f"dispatching (aborted or failed) — that is a RUN problem, not a blind "
                        f"instrument; chase why the units are dying")
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
    #
    # F814b: DEBOUNCED + EXACT. The raw pattern also matched EPHEMERAL processes (worker shell
    # tool-calls whose command line happens to contain similar text) — measured: three
    # consecutive checks each named a DIFFERENT second pid that was dead moments later, while
    # the true count was one. A real orphan persists; a match that cannot survive a two-second
    # resample was never an engine. Only processes running the actual release binary count.
    if len(engine_pids) > 1:
        import time as _time
        _time.sleep(2)
        resample = set(pgrep("goose swarm run"))
        persistent = []
        for pid in engine_pids:
            if pid not in resample:
                continue
            try:
                cmd = subprocess.run(["ps", "-o", "command=", "-p", str(pid)],
                                     capture_output=True, text=True).stdout
                if "target/release/goose" in cmd or "target/debug/goose" in cmd:
                    persistent.append(pid)
            except Exception:
                # A pid that vanished before ps could look at it is BY DEFINITION not a
                # persistent orphan — fail-open here counted the checker's own dead
                # transients as engines (measured: three alarms, three different pids,
                # each gone within seconds; the real count was one throughout).
                continue
        if len(persistent) > 1:
            rep.add("BAD", f"{len(persistent)} engines running at once {persistent} — an orphan "
                           f"is contending for the single addressable worker")

    print(rep.render())
    return rep.worst


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""WHERE does the fleet go idle? Per-phase wall-clock and node occupancy. Exit 0.

occupancy.py answers "how much of the fleet did this run use" for the run as a whole and for the
execute window. That is the right number for goal one, and it is also too coarse to act on: a run at
0.35 overall and 0.80 during execute has already told you the loss is OUTSIDE execute, and then
stops. This instrument splits the run at the engine's own phase markers and reports each phase
separately, so the answer to "what do I fix" is a phase name rather than a shrug.

THE PAIRING IS NOT RE-IMPLEMENTED HERE. It is imported from occupancy.py via `_spans`. Re-deriving
those spans by hand is a documented way to be wrong: an ad-hoc version paired dispatch[i] with
completion[i] across a retry and reported a 1484s solo window where the true figure was 55.9s.

THE HONEST GAP, stated up front. Only EXECUTE and the repair tail dispatch tasks, so only they have
`task_dispatched`/`task_completed` and therefore measurable node-seconds. Research, the planning
skeleton, the detail fan and contracts are real model calls on real nodes that emit no dispatch pair
at all. For those phases this instrument reports busy=None, NEVER busy=0 — a zero would read as "the
fleet was idle for 29 minutes" when the truth is "nobody measured it", and those are opposite
findings calling for opposite fixes. Where a phase does carry per-call timing (`detail_completed`
now emits `secs`), it is used and labelled `partial` — because it covers the detail fan but not the
skeleton draft that precedes it.

Usage:
    python3 phases.py <run-dir> [...]
    python3 phases.py --json <run-dir>
    python3 phases.py --self-test
"""
from __future__ import annotations

import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import occupancy  # noqa: E402  — the span pairing lives there and is not duplicated here

PHASES_VERSION = "ph-1"

# Each phase is [start marker, end marker). The markers are events the engine already emits; this
# instrument invents no boundaries of its own. `plan_loaded` ends planning AND begins execute
# because the engine dispatches the first task in the same instant it publishes the DAG.
PHASE_SPEC = [
    ("research", "run_started", "research_completed",
     "scouts read the world; produces the findings every later phase quotes"),
    ("plan", "research_completed", "confidence_retarget",
     "best-of-N skeleton draft + the confidence/redraft decision"),
    ("detail", "confidence_retarget", "plan_loaded",
     "the fan that writes each subtask's real spec, one call per device"),
    ("execute", "plan_loaded", "complete_failed_tasks",
     "the DAG runs; this is the only phase the scheduler fully owns"),
    ("tail", "complete_failed_tasks", "run_finished",
     "verify -> findings -> fix, looped until green; the serial repair tail"),
]

# Phases known to run real model calls without emitting a dispatch pair. This set is a DEFAULT, not
# the rule — the rule is applied from the data in `analyse`: a window containing no dispatch at all
# is UNMEASURED whatever its name.
#
# `tail` is in this set and it is the reason the rule had to become data-driven. It was first
# classified as measurable because the repair loop is obviously doing work, and this instrument duly
# reported occupancy 0.00 for it on all six archived runs — which reads as "26 minutes with the
# whole fleet idle" and is exactly the fabricated zero the module docstring warns about. MEASURED:
# the 26.5-minute tail of preboundary-7 emits TWELVE events, every one a phase summary
# (spec_contract, complete_verify, review, ...), and NOT ONE task_dispatched. The fix worker's calls
# go out through a path that reports nothing, so the tail's occupancy is not low — it is UNKNOWN.
UNDISPATCHED = {"research", "plan", "detail", "tail"}


def _marker_times(events: list[dict]) -> dict[str, float]:
    """First occurrence of each marker. FIRST, not last: `spec_contract` and `complete_verify`
    repeat once per repair round, and taking the last would swallow the whole tail into a phase
    boundary."""
    out: dict[str, float] = {}
    for e in events:
        n = e.get("event")
        ts = occupancy.parse_ts(e.get("ts"))
        if n and ts is not None and n not in out:
            out[n] = ts
    return out


def analyse(path) -> dict:
    events = occupancy.read_events(path)
    if not events:
        return {"phases_version": PHASES_VERSION, "path": str(path), "phases": [],
                "note": "empty log — nothing measured, which is not zero occupancy"}

    occ = occupancy.analyse(path)
    spans = occ.get("_spans") or []
    n = occ.get("pool_size")
    marks = _marker_times(events)
    t0 = occ.get("_t0")
    t_end = occ.get("_t_end")

    # Per-call durations for the phases that dispatch nothing. Only the detail fan emits one today.
    detail_secs: list[float] = [
        float(e.get("secs") or 0.0) for e in events if e.get("event") == "detail_completed"
    ]

    def clip_busy(a: float, b: float) -> tuple[float, dict[str, float]]:
        """Node-seconds inside [a,b), per device, as a UNION per device — never a sum. A device
        with weight>1 can hold two spans at once and summing them credits it more busy time than
        the window contains, which is how two archived 1-device runs scored occupancy 1.28."""
        per_dev: dict[str, list[tuple[float, float]]] = {}
        for s in spans:
            lo, hi = max(s["start"], a), min(s["end"], b)
            if hi > lo:
                per_dev.setdefault(s["device"], []).append((lo, hi))

        def union(iv):
            tot, cs, ce = 0.0, None, None
            for x, y in sorted(iv):
                if ce is None or x > ce:
                    if ce is not None:
                        tot += ce - cs
                    cs, ce = x, y
                else:
                    ce = max(ce, y)
            return tot + (ce - cs if ce is not None else 0.0)

        dev = {d: union(iv) for d, iv in per_dev.items()}
        return sum(dev.values()), dev

    out = []
    for name, start_ev, end_ev, what in PHASE_SPEC:
        a = marks.get(start_ev, t0 if start_ev == "run_started" else None)
        b = marks.get(end_ev)
        if b is None and end_ev == "run_finished":
            b = t_end
        if a is None or b is None or b < a:
            out.append({"phase": name, "what": what, "wall_secs": None,
                        "note": f"marker missing ({start_ev} -> {end_ev}) — phase not measured"})
            continue
        wall = b - a
        busy, per_dev = clip_busy(a, b)
        # THE RULE, applied from the data and not from the phase's name: a window with no dispatch
        # in it has no occupancy, however long it is. Trusting the name alone is what produced a
        # published 0.00 for the tail.
        dispatched_here = sum(
            1 for e in events
            if e.get("event") == "task_dispatched"
            and (occupancy.parse_ts(e.get("ts")) or -1) >= a
            and (occupancy.parse_ts(e.get("ts")) or -1) < b
        )
        blind = name in UNDISPATCHED or (dispatched_here == 0 and not per_dev)
        row = {
            "phase": name,
            "what": what,
            "wall_secs": round(wall, 1),
            "share_of_run": round(wall / (t_end - t0), 4) if (t_end and t0 and t_end > t0) else None,
            "devices_that_worked": len(per_dev),
            "dispatches": dispatched_here,
        }
        if blind:
            row["busy_node_secs"] = None
            row["occupancy"] = None
            if name == "detail" and detail_secs:
                # PARTIAL by construction: it covers the fan's own calls, not the skeleton or any
                # retry around them, so it is a FLOOR on busy and therefore a floor on occupancy.
                floor = sum(detail_secs)
                row["measured_call_secs"] = round(floor, 1)
                row["occupancy_floor"] = round(floor / (wall * n), 4) if (wall and n) else None
                row["calls"] = len(detail_secs)
            row["note"] = "phase dispatches no tasks — busy is UNMEASURED, not zero"
            if busy > 0:
                # Spans that merely OVERLAP the window (a task still finishing as the phase
                # started). Real node-seconds, but they belong to the previous phase's work, so
                # they are reported apart rather than folded into an occupancy this phase did not
                # earn.
                row["spillover_node_secs"] = round(busy, 1)
        else:
            row["busy_node_secs"] = round(busy, 1)
            row["occupancy"] = round(busy / (wall * n), 4) if (wall and n) else None
            row["per_device_secs"] = {k: round(v, 1) for k, v in sorted(per_dev.items())}
        out.append(row)

    measured = [p for p in out if p.get("occupancy") is not None]
    return {
        "phases_version": PHASES_VERSION,
        "path": str(path),
        "pool_size": n,
        "wall_secs": round(t_end - t0, 1) if (t_end and t0) else None,
        "phases": out,
        "unmeasured_wall_secs": round(
            sum(p["wall_secs"] for p in out
                if p.get("wall_secs") and p.get("occupancy") is None), 1),
        "worst_measured_phase": min(measured, key=lambda p: p["occupancy"])["phase"] if measured else None,
    }


def render(a: dict) -> str:
    if not a.get("phases"):
        return f"{a.get('path')}: {a.get('note', 'nothing to report')}"
    L = [f"{a['path']}  pool={a.get('pool_size')}  wall={(a.get('wall_secs') or 0)/60:.1f}m"]
    L.append(f"  {'phase':<9} {'wall':>8} {'%run':>6} {'busy':>9} {'occ':>6}  what")
    for p in a["phases"]:
        if p.get("wall_secs") is None:
            L.append(f"  {p['phase']:<9} {'—':>8} {'—':>6} {'—':>9} {'—':>6}  {p.get('note','')}")
            continue
        w = f"{p['wall_secs']/60:.1f}m"
        pc = f"{100*(p.get('share_of_run') or 0):.0f}%"
        if p.get("occupancy") is None:
            floor = p.get("occupancy_floor")
            busy = "unmeas." if floor is None else f">{p['measured_call_secs']/60:.0f}m"
            occ = "?" if floor is None else f">{floor:.2f}"
        else:
            busy = f"{p['busy_node_secs']/60:.0f}m"
            occ = f"{p['occupancy']:.2f}"
        L.append(f"  {p['phase']:<9} {w:>8} {pc:>6} {busy:>9} {occ:>6}  {p['what']}")
    L.append(f"  unmeasured wall: {a['unmeasured_wall_secs']/60:.1f}m "
             f"({100*a['unmeasured_wall_secs']/(a['wall_secs'] or 1):.0f}% of the run has NO "
             f"occupancy number at all)")
    if a.get("worst_measured_phase"):
        L.append(f"  worst measured phase: {a['worst_measured_phase']}")
    return "\n".join(L)


def self_test() -> int:
    """Controls in both directions, plus the vacuous-truth trap. An instrument that cannot fail
    its own controls has no standing to publish a number about the fleet."""
    import tempfile

    fails: list[str] = []

    def check(name, got, want, tol=0.02):
        ok = (got is None and want is None) or (
            got is not None and want is not None and abs(got - want) <= tol)
        if not ok:
            fails.append(f"{name}: got {got!r}, want {want!r}")

    def ts(sec):
        from datetime import datetime, timezone
        return datetime.fromtimestamp(1_700_000_000 + sec, timezone.utc).isoformat()

    def write(events) -> str:
        d = pathlib.Path(tempfile.mkdtemp())
        (d / "run.jsonl").write_text("\n".join(json.dumps(e) for e in events))
        return str(d)

    devs = ["d0", "d1", "d2"]
    pool = [{"id": d} for d in devs]
    perfect_dir: list[str] = []   # the SAME path twice, or the determinism check compares tempdirs

    # CONTROL 1 — a perfect execute phase: 3 devices busy the whole window.
    ev = [{"event": "run_started", "pool": pool, "ts": ts(0)},
          {"event": "research_completed", "ts": ts(10)},
          {"event": "confidence_retarget", "ts": ts(20)},
          {"event": "plan_loaded", "ts": ts(30), "tasks": []}]
    for i, d in enumerate(devs):
        ev.append({"event": "task_dispatched", "task_id": f"t{i}", "device": d, "ts": ts(30)})
        ev.append({"event": "task_completed", "task_id": f"t{i}", "device": d, "ts": ts(130)})
    ev += [{"event": "complete_failed_tasks", "ts": ts(130)},
           {"event": "run_finished", "ts": ts(160)}]
    perfect_dir.append(write(ev))
    a = analyse(perfect_dir[0])
    ph = {p["phase"]: p for p in a["phases"]}
    check("perfect execute occupancy", ph["execute"]["occupancy"], 1.0)
    check("execute wall", ph["execute"]["wall_secs"], 100.0, tol=0.6)

    # CONTROL 2 — the opposite: one device does everything, two idle. Must be ~1/3.
    ev2 = [e for e in ev if not (e.get("event", "").startswith("task_") and e.get("device") != "d0")]
    a2 = analyse(write(ev2))
    ph2 = {p["phase"]: p for p in a2["phases"]}
    check("1-of-3 execute occupancy", ph2["execute"]["occupancy"], 1 / 3)

    # THE TRAP THIS INSTRUMENT EXISTS TO AVOID. Research/plan/detail dispatch nothing, so a
    # span-based measure sees no work there. It must report None, never 0.0 — reporting 0.0 would
    # say "the fleet idled through planning", which is a claim nobody has evidence for.
    for name in ("research", "plan", "detail"):
        if ph[name]["occupancy"] is not None:
            fails.append(f"{name}: occupancy must be None (unmeasured), got {ph[name]['occupancy']}")
        if ph[name].get("busy_node_secs") is not None:
            fails.append(f"{name}: busy must be None, got {ph[name]['busy_node_secs']}")

    # VACUOUS TRUTH — an empty log must produce nothing, not a full-marks phase table.
    empty = analyse(write([]))
    if empty.get("phases"):
        fails.append("empty log produced phases — must produce none")

    # A MISSING MARKER must degrade that phase to unmeasured, not silently absorb its wall into a
    # neighbour. Drop `confidence_retarget` (a run where the redraft never fired) and both the plan
    # and detail phases lose their boundary.
    ev3 = [e for e in ev if e.get("event") != "confidence_retarget"]
    a3 = analyse(write(ev3))
    ph3 = {p["phase"]: p for p in a3["phases"]}
    if ph3["plan"]["wall_secs"] is not None or ph3["detail"]["wall_secs"] is not None:
        fails.append("a missing marker must leave its phases unmeasured, not reassign the time")
    if ph3["execute"]["occupancy"] is None:
        fails.append("a missing planning marker must not break the execute measurement")

    # DETERMINISM — two passes over identical input must agree exactly.
    if json.dumps(analyse(perfect_dir[0]), sort_keys=True) != json.dumps(a, sort_keys=True):
        fails.append("non-deterministic: two passes over identical input disagreed")

    # SHARES MUST NOT EXCEED THE RUN. A phase table whose parts sum past the whole is partitioning
    # the timeline wrong, which is the failure mode that would make every conclusion here bogus.
    tot = sum(p["wall_secs"] for p in a["phases"] if p.get("wall_secs"))
    if tot > (a["wall_secs"] or 0) + 1:
        fails.append(f"phase walls sum to {tot} on a {a['wall_secs']}s run — phases overlap")

    for f in fails:
        print(f"FAIL {f}")
    if fails:
        return 1
    print(f"self-test OK ({PHASES_VERSION}) — perfect/1-node controls, the unmeasured-is-not-zero "
          f"trap, missing markers, vacuous truth, determinism and the partition invariant all pass")
    return 0


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith("--")]
    if "--self-test" in argv:
        return self_test()
    if not args:
        print(__doc__)
        return 0
    for p in args:
        a = analyse(p)
        print(json.dumps(a, indent=2) if "--json" in argv else render(a))
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

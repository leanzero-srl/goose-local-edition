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

THE BOUNDARIES ARE NOW FIRST-CLASS. The old segmentation inferred the plan->detail boundary from
`confidence_retarget`, an event that only fired when the redraft ladder fired — so a run that
converged first time reported plan and detail as "marker missing" and lost both walls. The linear
engine emits {"event":"phase","phase":"open"|"ask"|"research"|"synthesis"|"review"} at every
planning boundary, so this instrument reads the boundary the engine declares instead of guessing one
from a side effect. Downstream of the plan the markers are still inferred, because the engine
declares no phase event there: `plan_loaded` opens BUILD, the first `complete_verify` opens
INTEGRATE, and the first fix dispatch opens REPAIR.

PHASES THAT DID NOT RUN ARE NOT PHASES THAT WERE NOT MEASURED. ASK only fires when the opener listed
an open decision, CONTRACTS/PILLARS only when their gates are on, REPAIR only when the app came back
red. Those rows say "did not run", and only a MANDATORY phase missing its marker is reported as an
instrument or engine fault — conflating the two turns a healthy green-first-time run into a table of
red rows.

THE HONEST GAP, stated up front. Only BUILD and the repair tail dispatch tasks, so only they have
`task_dispatched`/`task_completed` and therefore measurable node-seconds. Every planning phase is
real model calls on real nodes that emit no dispatch pair at all. For those phases this instrument
reports busy=None, NEVER busy=0 — a zero would read as "the fleet was idle for 29 minutes" when the
truth is "nobody measured it", and those are opposite findings calling for opposite fixes. There is
no longer ANY partial figure to offer: the detail fan's per-call `secs` died with `detail_completed`,
and what the new phases publish (`slices_opened.secs`, `research_completed.secs`) is the phase's own
wall, not node-seconds. Multiplying it by the slice count would be a CEILING dressed as the floor
this table used to print, so nothing is printed.

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

PHASES_VERSION = "ph-2"

# Each phase is [first start marker present, first end marker present). Markers are events the engine
# already emits; this instrument invents no boundaries of its own. A `phase` event is addressed as
# `phase:<name>` so the five planning boundaries are distinguishable — they all share event="phase".
#
# WHY THE MARKERS ARE LISTS. The alternative that has to be lived with is a run losing a whole
# phase's wall because a conditional neighbour never fired: OPEN ends at ASK when the opener raised an
# open decision and at RESEARCH when it did not, and INTEGRATE ends at the first fix dispatch on a red
# app and at `run_finished` on a green one. The EARLIEST candidate present wins — earliest, not
# first-listed, so a run where the repair tail happens to emit its wave event before its first
# dispatch cannot hand REPAIR's opening minutes to INTEGRATE.
#
# `optional=True` means the phase legitimately does not exist in some runs. `plan_loaded` ends REVIEW
# (or PREP) and begins BUILD because the engine dispatches the first task in the same instant it
# publishes the DAG.
PHASE_SPEC = [
    ("startup", ["run_started"], ["phase:open"], False,
     "pool resolution and config, before the opener's first call"),
    ("open", ["phase:open"], ["phase:ask", "phase:research"], False,
     "one node cuts the request into balanced semantic slices"),
    ("ask", ["phase:ask"], ["phase:research"], True,
     "the opener's open decisions get answered; the proxy answers when unattended"),
    ("research", ["phase:research"], ["phase:synthesis"], False,
     "one slice per node; each owner writes that module's full spec"),
    ("synthesis", ["phase:synthesis"], ["phase:review"], False,
     "one node wires the slices into a task DAG; the engine splices the specs in"),
    ("review", ["phase:review"], ["contracts", "pillars", "plan_loaded"], False,
     "structural patches only, until the reviewer asks for no change"),
    ("prep", ["contracts", "pillars"], ["plan_loaded"], True,
     "the CONTRACTS/PILLARS fans, when their gates are on"),
    ("build", ["plan_loaded"], ["complete_verify", "run_finished"], False,
     "the DAG runs; this is the only phase the scheduler fully owns"),
    ("integrate", ["complete_verify"], ["complete_fix_dispatched", "complete_fix_wave",
                                        "run_finished"], False,
     "the assembled app is run and graded; the verdict that decides green"),
    ("repair", ["complete_fix_dispatched", "complete_fix_wave"], ["run_finished"], True,
     "findings -> fix -> re-verify, looped; only fires on a red app"),
]

# Phases known to run real model calls without emitting a dispatch pair. This set is a DEFAULT, not
# the rule — the rule is applied from the data in `analyse`: a window containing no dispatch at all
# is UNMEASURED whatever its name.
#
# `repair` is in this set and it is the reason the rule had to become data-driven. It was first
# classified as measurable because the repair loop is obviously doing work, and this instrument duly
# reported occupancy 0.00 for it on all six archived runs — which reads as "26 minutes with the
# whole fleet idle" and is exactly the fabricated zero the module docstring warns about. MEASURED:
# the 26.5-minute tail of preboundary-7 emits TWELVE events, every one a phase summary
# (spec_contract, complete_verify, review, ...), and NOT ONE task_dispatched. The fix worker's calls
# go out through a path that reports nothing, so the tail's occupancy is not low — it is UNKNOWN.
UNDISPATCHED = {"startup", "open", "ask", "research", "synthesis", "review", "prep",
                "integrate", "repair"}


def _marker_key(e: dict) -> str | None:
    """The name this event answers to as a boundary.

    The five planning boundaries all arrive as event="phase" and differ only in the `phase` field, so
    keying on the event name alone would collapse OPEN, ASK, RESEARCH, SYNTHESIS and REVIEW onto one
    marker and hand every planning phase the same instant.
    """
    n = e.get("event")
    if not n:
        return None
    if n == "phase":
        p = e.get("phase")
        return f"phase:{p}" if p else None
    return n


def _marker_times(events: list[dict]) -> dict[str, float]:
    """First occurrence of each marker. FIRST, not last: `complete_verify` and
    `complete_fix_dispatched` repeat once per repair round, and taking the last would swallow the
    whole tail into a phase boundary."""
    out: dict[str, float] = {}
    for e in events:
        n = _marker_key(e)
        ts = occupancy.parse_ts(e.get("ts"))
        if n and ts is not None and n not in out:
            out[n] = ts
    return out


def _earliest(marks: dict[str, float], names: list[str]) -> float | None:
    """The earliest of the candidate markers that actually fired, or None if none did."""
    hits = [marks[n] for n in names if n in marks]
    return min(hits) if hits else None


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
    for name, start_evs, end_evs, optional, what in PHASE_SPEC:
        a = _earliest(marks, start_evs)
        if a is None and "run_started" in start_evs:
            a = t0
        b = _earliest(marks, end_evs)
        if b is None and "run_finished" in end_evs:
            b = t_end
        if a is None:
            # THE DISTINCTION THAT KEEPS A HEALTHY RUN GREEN. ASK, PREP and REPAIR are conditional
            # branches of the engine, so their marker being absent is the engine reporting that the
            # branch was not taken — not this instrument failing to see it.
            note = (f"did not run ({'/'.join(start_evs)} never fired)" if optional
                    else f"marker missing ({'/'.join(start_evs)}) — phase not measured")
            out.append({"phase": name, "what": what, "wall_secs": None,
                        "ran": False if optional else None, "note": note})
            continue
        if b is None or b < a:
            out.append({"phase": name, "what": what, "wall_secs": None,
                        "note": f"marker missing ({'/'.join(end_evs)}) — phase not measured"})
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
        # A phase that DID NOT RUN contributes no wall, so it cannot inflate the unmeasured figure.
        "unmeasured_wall_secs": round(
            sum(p["wall_secs"] for p in out
                if p.get("wall_secs") and p.get("occupancy") is None), 1),
        "phases_that_did_not_run": [p["phase"] for p in out if p.get("ran") is False],
        "worst_measured_phase": min(measured, key=lambda p: p["occupancy"])["phase"] if measured else None,
    }


def render(a: dict) -> str:
    if not a.get("phases"):
        return f"{a.get('path')}: {a.get('note', 'nothing to report')}"
    L = [f"{a['path']}  pool={a.get('pool_size')}  wall={(a.get('wall_secs') or 0)/60:.1f}m"]
    L.append(f"  {'phase':<10} {'wall':>8} {'%run':>6} {'busy':>9} {'occ':>6}  what")
    for p in a["phases"]:
        if p.get("wall_secs") is None:
            L.append(f"  {p['phase']:<10} {'—':>8} {'—':>6} {'—':>9} {'—':>6}  {p.get('note','')}")
            continue
        w = f"{p['wall_secs']/60:.1f}m"
        pc = f"{100*(p.get('share_of_run') or 0):.0f}%"
        if p.get("occupancy") is None:
            busy, occ = "unmeas.", "?"
        else:
            busy = f"{p['busy_node_secs']/60:.0f}m"
            occ = f"{p['occupancy']:.2f}"
        L.append(f"  {p['phase']:<10} {w:>8} {pc:>6} {busy:>9} {occ:>6}  {p['what']}")
    L.append(f"  unmeasured wall: {a['unmeasured_wall_secs']/60:.1f}m "
             f"({100*a['unmeasured_wall_secs']/(a['wall_secs'] or 1):.0f}% of the run has NO "
             f"occupancy number at all)")
    if a.get("phases_that_did_not_run"):
        L.append(f"  did not run: {', '.join(a['phases_that_did_not_run'])}")
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

    # CONTROL 1 — a perfect build phase: 3 devices busy the whole window, on a full linear run.
    ev = [{"event": "run_started", "pool": pool, "ts": ts(0)},
          {"event": "phase", "phase": "open", "ts": ts(5)},
          {"event": "phase", "phase": "ask", "ts": ts(10)},
          {"event": "phase", "phase": "research", "ts": ts(15)},
          {"event": "phase", "phase": "synthesis", "ts": ts(25)},
          {"event": "phase", "phase": "review", "ts": ts(30)},
          {"event": "plan_loaded", "ts": ts(35), "tasks": []}]
    for i, d in enumerate(devs):
        ev.append({"event": "task_dispatched", "task_id": f"t{i}", "device": d, "ts": ts(35)})
        ev.append({"event": "task_completed", "task_id": f"t{i}", "device": d, "ts": ts(135)})
    ev += [{"event": "complete_verify", "round": 1, "passed": False, "ts": ts(135)},
           {"event": "complete_fix_dispatched", "round": 1, "ts": ts(145)},
           {"event": "run_finished", "ts": ts(160)}]
    perfect_dir.append(write(ev))
    a = analyse(perfect_dir[0])
    ph = {p["phase"]: p for p in a["phases"]}
    check("perfect build occupancy", ph["build"]["occupancy"], 1.0)
    check("build wall", ph["build"]["wall_secs"], 100.0, tol=0.6)

    # THE FIVE PLANNING BOUNDARIES MUST BE DISTINGUISHABLE. They all arrive as event="phase" and a
    # naive `event` key collapses them onto one marker, which hands every planning phase the same
    # instant and reports four zero-length phases plus one that swallows the lot.
    check("open wall", ph["open"]["wall_secs"], 5.0, tol=0.6)
    check("research wall", ph["research"]["wall_secs"], 10.0, tol=0.6)
    check("review wall", ph["review"]["wall_secs"], 5.0, tol=0.6)
    check("integrate wall", ph["integrate"]["wall_secs"], 10.0, tol=0.6)
    check("repair wall", ph["repair"]["wall_secs"], 15.0, tol=0.6)

    # CONTROL 2 — the opposite: one device does everything, two idle. Must be ~1/3.
    ev2 = [e for e in ev if not (e.get("event", "").startswith("task_") and e.get("device") != "d0")]
    a2 = analyse(write(ev2))
    ph2 = {p["phase"]: p for p in a2["phases"]}
    check("1-of-3 build occupancy", ph2["build"]["occupancy"], 1 / 3)

    # THE TRAP THIS INSTRUMENT EXISTS TO AVOID. Every planning phase dispatches nothing, so a
    # span-based measure sees no work there. It must report None, never 0.0 — reporting 0.0 would
    # say "the fleet idled through planning", which is a claim nobody has evidence for.
    for name in ("open", "ask", "research", "synthesis", "review", "integrate", "repair"):
        if ph[name]["occupancy"] is not None:
            fails.append(f"{name}: occupancy must be None (unmeasured), got {ph[name]['occupancy']}")
        if ph[name].get("busy_node_secs") is not None:
            fails.append(f"{name}: busy must be None, got {ph[name]['busy_node_secs']}")

    # VACUOUS TRUTH — an empty log must produce nothing, not a full-marks phase table.
    empty = analyse(write([]))
    if empty.get("phases"):
        fails.append("empty log produced phases — must produce none")

    # A MISSING MANDATORY MARKER must degrade that phase to unmeasured, not silently absorb its wall
    # into a neighbour. Drop `phase:synthesis` and both research and synthesis lose their boundary.
    ev3 = [e for e in ev if e.get("phase") != "synthesis"]
    a3 = analyse(write(ev3))
    ph3 = {p["phase"]: p for p in a3["phases"]}
    if ph3["research"]["wall_secs"] is not None or ph3["synthesis"]["wall_secs"] is not None:
        fails.append("a missing marker must leave its phases unmeasured, not reassign the time")
    if ph3["synthesis"].get("ran") is False:
        fails.append("synthesis is mandatory — a missing marker there is a fault, not 'did not run'")
    if ph3["build"]["occupancy"] is None:
        fails.append("a missing planning marker must not break the build measurement")

    # A CONDITIONAL PHASE THAT DID NOT RUN IS NOT A FAULT, and its absence must not stretch its
    # neighbour. The engine emits phase:ask only when the opener raised an open decision, so a run
    # without one must report ask as "did not run" while OPEN still ends at RESEARCH.
    ev4 = [e for e in ev if e.get("phase") != "ask"]
    a4 = analyse(write(ev4))
    ph4 = {p["phase"]: p for p in a4["phases"]}
    if ph4["ask"].get("ran") is not False:
        fails.append("a conditional phase that never fired must report ran=False, not a fault")
    check("open absorbs the skipped ask window", ph4["open"]["wall_secs"], 10.0, tol=0.6)
    if "ask" not in (a4.get("phases_that_did_not_run") or []):
        fails.append("phases_that_did_not_run must name the skipped phase")

    # A GREEN-FIRST-TIME RUN has no repair tail at all. INTEGRATE must then run to `run_finished`
    # rather than reporting a missing marker — the failure the old `confidence_retarget` boundary
    # made routine, where a run that simply converged lost two phases' wall.
    ev5 = [e for e in ev if not str(e.get("event", "")).startswith("complete_fix")]
    a5 = analyse(write(ev5))
    ph5 = {p["phase"]: p for p in a5["phases"]}
    check("green run integrate runs to the end", ph5["integrate"]["wall_secs"], 25.0, tol=0.6)
    if ph5["repair"].get("ran") is not False:
        fails.append("a green run must report repair as 'did not run', not as a missing marker")

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
    print(f"self-test OK ({PHASES_VERSION}) — perfect/1-node controls, the five planning boundaries, "
          f"the unmeasured-is-not-zero trap, missing markers, skipped conditional phases, a green "
          f"run's absent repair tail, vacuous truth, determinism and the partition invariant")
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

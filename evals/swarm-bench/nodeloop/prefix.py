#!/usr/bin/env python3
"""Where the pre-dispatch prefix goes. Exit 0 always — this reports, it never grades.

WHY. `occupancy.py` measures the EXECUTE window, because that is the only window that emits task
events. Everything before the first dispatch — the opener's cut, the ask, the slice owners, the
synthesis, the review, contracts — is invisible to it, and it is a quarter of the run. Round 5's
proposals for shrinking that quarter were all refuted, and the reason is visible in hindsight: they
were designed against a number nobody had measured. The prefix was treated as "serial planning" when
most of it is fleet work that simply emits no task event.

MEASURED across the three baseline units on disk, and the shape is stable:

    research    264 / 379 / 420 s
    planning    1292 / 1457 / 892 s     <- 57-83% of the prefix
    prefix      1556 / 1836 / 1312 s

WHAT THIS FILE NO LONGER DOES, and why the deletion is the right answer rather than a port. It used
to report what a redraft threw away and how much of it came back — `retarget_discarded` carried the
entire discarded plan, and the reuse figure decided whether a reuse path was worth building. The
OPEN -> ASK -> RESEARCH -> SYNTHESIS -> REVIEW rewrite deleted the redraft ladder, so there is no
discarded plan to compare against and no `detail_fallback` to attribute: those columns could only
ever have printed 0, which reads as "the ladder wasted nothing" rather than "there is no ladder".

`phases.py` owns the full-run phase table and the occupancy that goes with it. This is deliberately
the cheap pre-dispatch slice of the same timeline — it pairs no spans and computes no occupancy, so
there is nothing here for the two to disagree about.

Usage:
    python3 prefix.py <unit-dir> [<unit-dir> ...]
    python3 prefix.py --self-test
"""
from __future__ import annotations

import datetime as dt
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
PREFIX_VERSION = "px-2"

# The prefix is exactly these phases, in this order, and the engine names each one as it starts.
# `ask` and `prep` are conditional — the opener may raise no open decision and the CONTRACTS/PILLARS
# gates may be off — so a missing marker here means the phase did not happen, not that it was missed.
PREFIX_PHASES = ["open", "ask", "research", "synthesis", "review"]


def _ts(v):
    try:
        return dt.datetime.fromisoformat(str(v).replace("Z", "+00:00")).timestamp()
    except (ValueError, TypeError):
        return None


def plan_task_files(task: dict) -> list[str]:
    """A task's owned files, under whichever key the emitting event used.

    `plan_loaded` says `files`. Archived logs from the ladder era say `owned_files` on
    `retarget_discarded`, and reading only one name is what once made this file's central number
    structurally zero while still passing its own controls. Both are read here so a run pulled from
    the archive answers the same as a live one.
    """
    return list(task.get("files") or task.get("owned_files") or [])


def read_events(unit: pathlib.Path) -> list[dict]:
    f = unit / "run.jsonl"
    if not f.is_file():
        return []
    out = []
    for line in f.read_text(errors="replace").splitlines():
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def analyse_events(events: list[dict]) -> dict:
    """Pure, so the controls can drive it with a hand-built stream."""
    t0 = None
    first_dispatch = None
    contracts_at = None
    plan_tasks: list[dict] = []
    phase_at: dict[str, float] = {}

    for e in events:
        ev = e.get("event")
        t = _ts(e.get("ts"))
        if ev == "run_started" and t0 is None:
            t0 = t
        elif ev == "phase" and t is not None:
            # FIRST occurrence wins. REVIEW runs a round at a time and re-announces nothing, but a
            # future phase that repeats must not be allowed to move its own start marker forward.
            phase_at.setdefault(str(e.get("phase")), t)
        elif ev == "contracts" and contracts_at is None:
            contracts_at = t
        elif ev == "plan_loaded" and not plan_tasks:
            plan_tasks = [t_ for t_ in (e.get("tasks") or []) if isinstance(t_, dict)]
        elif ev == "task_dispatched" and first_dispatch is None:
            first_dispatch = t

    # An absent run_started or an absent dispatch means the window does not exist. Report nothing
    # rather than a zero — `all([])` is True and a prefix of 0s would read as "instant planning".
    if t0 is None or first_dispatch is None:
        return {"prefix_version": PREFIX_VERSION, "measurable": False,
                "reason": "no run_started or no task_dispatched — the prefix window does not exist"}

    prefix = first_dispatch - t0

    # Each phase runs until the next one that actually fired; the last one runs to the first dispatch.
    # A phase that never fired gets no row at all, rather than a zero that reads as "it was instant".
    fired = [(p, phase_at[p]) for p in PREFIX_PHASES if p in phase_at]
    phase_secs = {}
    for i, (name, start) in enumerate(fired):
        end = fired[i + 1][1] if i + 1 < len(fired) else first_dispatch
        phase_secs[name] = round(end - start, 1)

    research = phase_secs.get("research")
    # PLANNING IS THE REST OF THE PREFIX, not a phase. It is open + ask + synthesis + review + the
    # contracts/pillars fans + whatever the engine does between them, and it is the number the
    # prefix-shrinking proposals were all aimed at.
    planning = (prefix - research) if research is not None else None

    return {
        "prefix_version": PREFIX_VERSION,
        "measurable": True,
        "prefix_secs": round(prefix, 1),
        "research_secs": round(research, 1) if research is not None else None,
        "planning_secs": round(planning, 1) if planning is not None else None,
        "planning_share_of_prefix": round(planning / prefix, 3) if planning and prefix else None,
        "phase_secs": phase_secs,
        # None, never [], when the engine emitted no phase marker at all: a run from a build that
        # predates the linear engine is UNSEGMENTED, which is not the same as a run with no phases.
        "phases_seen": [p for p, _ in fired] or None,
        "contracts_to_dispatch_secs": (round(first_dispatch - contracts_at, 1)
                                       if contracts_at else None),
        "plan_task_count": len(plan_tasks),
    }


def analyse(unit: pathlib.Path) -> dict:
    r = analyse_events(read_events(unit))
    r["unit"] = unit.name
    return r


def render(r: dict) -> str:
    if not r.get("measurable"):
        return f"=== PREFIX {r.get('unit','?')}  NOT MEASURABLE — {r.get('reason')}"
    out = [f"=== PREFIX {r['unit']}  ({r['prefix_version']})  {r['prefix_secs']:.0f}s before the "
           f"first dispatch"]
    if r["research_secs"] is None:
        out.append("    no phase markers — this run predates the linear engine, so the prefix is "
                   "UNSEGMENTED, not instant")
        return "\n".join(out)
    out.append(f"    research {r['research_secs']:.0f}s   planning {r['planning_secs']:.0f}s "
               f"({r['planning_share_of_prefix']:.0%} of the prefix)")
    out.append("    " + "  ".join(f"{k} {v:.0f}s" for k, v in r["phase_secs"].items()))
    if r["contracts_to_dispatch_secs"] is not None:
        out.append(f"    contracts -> first dispatch: {r['contracts_to_dispatch_secs']:.0f}s")
    return "\n".join(out)


def self_test() -> int:
    """Both directions, plus the vacuous-truth trap."""
    fails = []

    # An EMPTY stream must be NOT MEASURABLE, never a 0-second prefix.
    empty = analyse_events([])
    if empty.get("measurable"):
        fails.append("an empty event stream reported a measurable prefix")

    # A stream with a dispatch but no run_started is equally unmeasurable.
    if analyse_events([{"event": "task_dispatched", "ts": "2026-01-01T00:00:00+00:00"}]).get("measurable"):
        fails.append("a stream with no run_started reported a measurable prefix")

    def at(s):
        return dt.datetime.fromtimestamp(s, dt.timezone.utc).isoformat()

    # KNOWN VALUES: open at 50s, ask at 150s, research 250-500s, synthesis 500-700s, review 700-980s,
    # contracts at 980s, first dispatch at 1000s.
    stream = [
        {"event": "run_started", "ts": at(0)},
        {"event": "phase", "phase": "open", "ts": at(50)},
        {"event": "phase", "phase": "ask", "ts": at(150)},
        {"event": "phase", "phase": "research", "ts": at(250)},
        {"event": "phase", "phase": "synthesis", "ts": at(500)},
        {"event": "phase", "phase": "review", "ts": at(700)},
        {"event": "contracts", "ts": at(980)},
        {"event": "plan_loaded", "ts": at(1000), "tasks": [
            {"id": "api", "files": ["a.py"]}, {"id": "web", "files": ["w.py"]}]},
        {"event": "task_dispatched", "ts": at(1000)},
    ]
    r = analyse_events(stream)
    if r["prefix_secs"] != 1000.0:
        fails.append(f"prefix {r['prefix_secs']} != 1000")
    if r["research_secs"] != 250.0:
        fails.append(f"research {r['research_secs']} != 250")
    if r["planning_secs"] != 750.0:
        fails.append(f"planning {r['planning_secs']} != 750")
    if r["phase_secs"] != {"open": 100.0, "ask": 100.0, "research": 250.0,
                           "synthesis": 200.0, "review": 300.0}:
        fails.append(f"phase_secs wrong: {r['phase_secs']}")
    if r["contracts_to_dispatch_secs"] != 20.0:
        fails.append(f"contracts_to_dispatch {r['contracts_to_dispatch_secs']} != 20")
    if r["plan_task_count"] != 2:
        fails.append(f"plan_task_count {r['plan_task_count']} != 2")

    # A PHASE THAT DID NOT RUN gets no row and does not inflate its neighbour's END — the ask is
    # conditional, and OPEN must then run all the way to RESEARCH rather than stopping at a marker
    # that never arrived.
    no_ask = analyse_events([e for e in stream if e.get("phase") != "ask"])
    if "ask" in no_ask["phase_secs"]:
        fails.append("a phase that never fired got a row")
    if no_ask["phase_secs"].get("open") != 200.0:
        fails.append(f"open must run to research when ask is skipped: {no_ask['phase_secs']}")

    # NEGATIVE DIRECTION, and the whole reason this file reports `phases_seen`. A run from a build
    # with no phase markers is UNSEGMENTED. It must not report research 0s / planning = the whole
    # prefix, which is a confident split of a window nothing measured.
    unseg = analyse_events([e for e in stream if e.get("event") != "phase"])
    if unseg["research_secs"] is not None or unseg["planning_secs"] is not None:
        fails.append("a run with no phase markers reported a research/planning split anyway")
    if unseg["phases_seen"] is not None:
        fails.append("phases_seen must be None, not [], when nothing was segmented")
    if unseg["prefix_secs"] != 1000.0:
        fails.append("the prefix window itself is still measurable without phase markers")

    # REAL-SHAPE CONTROL. Every check above is driven by a stream this file wrote, so all of them
    # passed while the file-survival path read a key the engine never emits. A control that shares the
    # instrument's assumption cannot test it. This one reads an ACTUAL run off disk and asserts the
    # path is non-vacuous — if plan_loaded's key changes again, this fails instead of silently
    # returning zero.
    real = sorted(HERE.parent.glob("runs/nodeloop*/*/run.jsonl"))
    if real:
        ev = []
        for line in real[-1].read_text(errors="replace").splitlines():
            try:
                ev.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        plan = next((e for e in ev if e.get("event") == "plan_loaded"), None)
        if plan:
            tasks = [t for t in (plan.get("tasks") or []) if isinstance(t, dict)]
            files = {f for t in tasks for f in plan_task_files(t)}
            if tasks and not files:
                fails.append(f"{real[-1].parent.name}: plan_loaded has {len(tasks)} tasks and "
                             f"plan_task_files() found NO files — the key changed again "
                             f"(saw: {sorted(tasks[0].keys())})")
    else:
        print("  note: no real run on disk, the real-shape control did not run")

    if fails:
        print(f"prefix.py SELF-TEST FAILED ({PREFIX_VERSION}):")
        for f in fails:
            print("  -", f)
        return 1
    print(f"prefix.py self-test OK ({PREFIX_VERSION})")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    args = [a for a in argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        return 2
    for a in args:
        u = pathlib.Path(a).resolve()
        if not u.is_dir():
            print(f"no such unit dir: {u}")
            continue
        r = analyse(u)
        print(json.dumps(r, indent=1) if "--json" in argv else render(r))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

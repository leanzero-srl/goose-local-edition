#!/usr/bin/env python3
"""The stall detector. Records the goal state each tick and DEMANDS a shake-up when it stops moving.

Mihai, 2026-08-03, after 2.5 days and 300+ commits produced zero measured improvement:

    "if your update continues to stay the same for 10 ticks which is your (2) and (3) then it means
     you need to shake things up. If (1) becomes resolved then this whole session is resolved."

WHY THIS IS A SCRIPT AND NOT A HABIT. The failure it guards against is precisely the kind an agent
cannot self-police: every individual tick had a defensible reason to keep going as it was — measure
the variance first, wait for the arm, let the baseline finish. Ten such reasons in a row is a stall,
and the only thing that can see ten in a row is something that persists across context compaction.
A rule I have to REMEMBER is a rule that dies with the context window; a file on disk does not.

THE STREAK IS DERIVED, NOT DECLARED. `mini_goal` is a string I pass in, but the thing the streak
actually keys on is the MEASURED metric read off the run archive by `failures.py`'s own classifier —
so I cannot reset the counter by rewording the goal, and a genuine metric move resets it even if I
describe the work identically. Declaring progress is not progress.

Usage:
    python3 goalstate.py --tick --mini "move the test-author failure row" --resolved "weights routing"
    python3 goalstate.py                 # report only, no record
"""
from __future__ import annotations

import datetime
import json
import pathlib
import sys

import failures  # NEVER re-implement the metric — Lesson 2

HERE = pathlib.Path(__file__).resolve().parent
LOG = HERE / "GOALS.jsonl"
RUNS = HERE.parent / "runs" / "nodeloop"
ALL_RUNS = HERE.parent / "runs"
BINARY = HERE.parents[2] / "target" / "release" / "goose"
STALL_TICKS = 10


def binary_mtime() -> float:
    """When the engine under test was built. Runs older than this were produced by DIFFERENT code.

    THIS IS THE FIX FOR A METRIC THAT COULD NOT MOVE. `failures.py` globs the whole `runs/` tree and
    finds 33 logs, 27 of them in `nodeloop-preboundary-*` archives from earlier builds. Its
    "test-authors are 14/15 = 93% of ALL failures" is therefore POOLED ACROSS EVERY ENGINE THIS
    CAMPAIGN HAS EVER RUN — so an improvement made today is averaged against a day and a half of runs
    that cannot respond to it. A stall detector keyed on a number that structurally cannot move would
    fire every single tick and mean nothing.

    Scoping by the binary's mtime is self-maintaining: it needs no manual boundary marker, it follows
    every future crossing automatically, and it states its own claim precisely — "runs produced by
    the engine currently on disk". A run started before the rebuild and finishing after it will be
    misfiled, which is why the boundary procedure kills the engine BEFORE rebuilding.
    """
    try:
        return BINARY.stat().st_mtime
    except OSError:
        return 0.0

# HIS GOAL — the one that resolves the whole session. Stated here so it is read off disk each tick
# rather than recalled, because a recalled goal drifts toward whatever the work happens to be doing.
GOAL = ("a 3-node run beats a 1-node run on BOTH wall-clock and the quality of what ships, "
        "with the gap clearing the measured replicate spread")


def measured_metric() -> dict:
    """The test-author row, derived from the archive using failures.py's OWN classifier.

    This is what the streak keys on. It is READ, never passed in.

    ⚠ The first version of this function invented its own row shape — it called `failures.load()`
    (which returns RAW EVENTS) and then read `r["kind"]` and `r["failed"]`, fields that do not exist
    on an event. Every lookup returned None, so it reported "45 completed / 0 failed, 0% of all
    failures" against a known 93%. An impossible value indicts the instrument (Lesson 47) and it was
    caught only because that 0% contradicted a number I already had. The fix is to mirror
    `failures.main`'s ACTUAL logic — same `kind_of`, same `run_finished` gate, same `status != "done"`
    test, same 1node/2node exclusion — so this can never drift from the metric it claims to track.
    """
    by_kind: dict[str, list[int]] = {}
    for path in sorted(ALL_RUNS.glob("**/run.jsonl")):
        name = str(path)
        if "1node" in name or "2node" in name:
            continue                                  # a different question (the node curve)
        if path.stat().st_mtime < binary_mtime():
            continue                                  # produced by a DIFFERENT engine — see below
        ev = failures.load(name)
        if not any(e.get("event") == "run_finished" for e in ev):
            continue                                  # unfinished runs cannot contribute a row
        owned = {e["task_id"]: e.get("owned_files") or []
                 for e in ev if e.get("event") == "task_dispatched"}
        for e in ev:
            if e.get("event") != "task_completed":
                continue
            k = failures.kind_of(e["task_id"], owned.get(e["task_id"], []))
            slot = by_kind.setdefault(k, [0, 0])
            slot[0] += 1
            if e.get("status") != "done":
                slot[1] += 1
    ta = by_kind.get("test-author", [0, 0])
    total_failed = sum(f for _, f in by_kind.values())
    return {
        "test_author_completed": ta[0],
        "test_author_failed": ta[1],
        "all_failed": total_failed,
        "test_author_share_of_failures": (ta[1] / total_failed) if total_failed else 0.0,
    }


def history() -> list[dict]:
    if not LOG.is_file():
        return []
    out = []
    for line in LOG.read_text().splitlines():
        if line.strip():
            try:
                out.append(json.loads(line))
            except Exception:
                continue
    return out


def streak(hist: list[dict]) -> int:
    """Consecutive ticks whose (mini_goal, resolved, measured metric) are identical to the newest."""
    if not hist:
        return 0
    def key(r: dict) -> str:
        return json.dumps([r.get("mini_goal"), sorted(r.get("resolved") or []), r.get("metric")],
                          sort_keys=True)
    newest = key(hist[-1])
    n = 0
    for r in reversed(hist):
        if key(r) != newest:
            break
        n += 1
    return n


# The escalation menu. NOT "try harder" — each entry is a concrete move that changes the SHAPE of the
# work rather than its intensity, because a stall means the current shape is not producing.
SHAKES = [
    "CHANGE THE SUBJECT, NOT THE PRECISION. Stop refining the current measurement and change the "
    "engine somewhere it has never been changed. A more accurate number about an unchanged system "
    "is the stall, not the cure.",
    "SHORTEN THE LOOP AGAIN. If the current evidence needs a full run, find the pure function "
    "underneath it and test that offline (judge_replay.rs is the precedent: 100 minutes -> 0.00s).",
    "ATTACK THE BIGGEST UNTOUCHED COMPONENT. Rank the prompt/phase/time budget by size and go at "
    "whatever is largest and has never been modified, rather than the thing most recently measured.",
    "FLIP A DEFAULT INSTEAD OF MEASURING A LEVER. A lever that is off by default and fixes a "
    "verified defect does not need an A/B to justify turning on — a broken artifact is a bug.",
    "CUT THE UNIT OF WORK. If one run is 100 minutes, run a smaller spec, fewer reps, or a single "
    "phase. Feedback latency is a variable under my control, not a constant.",
    "DELETE SOMETHING. A mechanism that has never fired, or fires and changes nothing, is cost "
    "without benefit; removing it is a real change and it is reversible.",
]


def report(mini: str | None, resolved: list[str] | None, record: bool) -> int:
    m = measured_metric()
    hist = history()
    if record:
        rec = {
            "ts": datetime.datetime.now().isoformat(timespec="seconds"),
            "mini_goal": mini or "(unstated)",
            "resolved": resolved or [],
            "metric": m,
        }
        with LOG.open("a") as fh:
            fh.write(json.dumps(rec) + "\n")
        hist = hist + [rec]

    s = streak(hist)
    print(f"(1) HIS GOAL — resolves the SESSION: {GOAL}")
    if hist:
        print(f"(2) MINI-GOAL: {hist[-1]['mini_goal']}")
        print(f"(3) RESOLVED : {hist[-1]['resolved'] or 'ZERO'}")
    # "0 completed / 0 failed" and "0 failures out of many" are NOT the same statement and must never
    # print the same way. An empty sample is the absence of evidence; printing it as 0% would read as
    # a perfect score and would be the vacuous-truth trap (`all([])` is True).
    if m["test_author_completed"] == 0 and m["test_author_failed"] == 0:
        print("\nMEASURED: NO FINISHED RUN on the current binary yet — the metric has no sample. "
              "This is absence of evidence, not a zero failure rate.")
    else:
        print(f"\nMEASURED (read off the archive, not declared; runs produced by the CURRENT binary "
              f"only): test-author {m['test_author_completed']} completed / "
              f"{m['test_author_failed']} failed, "
              f"{m['test_author_share_of_failures']:.0%} of all failures")
    print(f"UNCHANGED FOR {s} of {STALL_TICKS} ticks ({s * 10} of {STALL_TICKS * 10} minutes)")

    if s >= STALL_TICKS:
        print("\n" + "=" * 78)
        print(f"🔴 STALLED — {s} ticks with no change to (2), (3) or the measured metric.")
        print("   Mihai's instruction: SHAKE THINGS UP. Pick one and do it THIS tick:")
        for i, sh in enumerate(SHAKES, 1):
            print(f"   {i}. {sh}")
        print("=" * 78)
    elif s >= STALL_TICKS - 3:
        print(f"\n⚠  {STALL_TICKS - s} ticks from a forced shake-up. If the next measurement is "
              f"another refinement of the same number, pre-empt it and change the shape now.")
    return 0


def main(argv: list[str]) -> int:
    mini = None
    resolved = None
    if "--mini" in argv:
        mini = argv[argv.index("--mini") + 1]
    if "--resolved" in argv:
        resolved = [x.strip() for x in argv[argv.index("--resolved") + 1].split(";") if x.strip()]
    return report(mini, resolved, record="--tick" in argv)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

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

⚠ OMITTING A FLAG MUST NOT RESET THE CLOCK (F290). It did, for 85 rows. A bare `--tick` recorded
`mini_goal="(unstated)"`, `resolved=[]` — a DIFFERENT streak key than the previous tick's real values
— so the counter went back to 1. Measured: thirteen consecutive rows carried
`GOAL ONE: node curve / ['F207'] / sig=False`, THREE TICKS PAST the forced shake-up, and one bare
`--tick` silenced the alarm for another ten. The loophole was in my own guardrail and the laziest
possible action was the one that exploited it (L90). State now CARRIES FORWARD: absent flags mean
"unchanged", which is what they have always meant in English, and the only way to reset the clock is
to actually change the goal or move the metric. Passing the same value again is correctly a no-op.

Usage:
    python3 goalstate.py --tick --mini "move the test-author failure row" --resolved "weights routing"
    python3 goalstate.py                 # report only, no record
"""
from __future__ import annotations

import datetime
import json
import pathlib
import sys
from math import comb

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
    per_run: list[tuple[int, int]] = []               # (test-author attempted, failed) PER RUN — L114
    seen_runs: set[str] = set()
    for path in sorted(ALL_RUNS.glob("**/run.jsonl")):
        name = str(path)
        if "1node" in name or "2node" in name:
            continue                                  # a different question (the node curve)
        ev = failures.load(name)
        if not any(e.get("event") == "run_finished" for e in ev):
            continue                                  # unfinished runs cannot contribute a row

        # ⚠⚠ FILE MTIME IS NOT PROVENANCE ONCE THE TREE HAS BEEN COPIED, AND IT IS COPIED ON EVERY
        # START. `loop.sh start` parks the run tree with `cp -R`, which stamps the copies with a
        # FRESH mtime — so a run produced by an OLD engine passes the very scope check that exists to
        # exclude it, while its original (untouched, older mtime) is correctly excluded. MEASURED:
        # four `nodeloop-parked-*` directories all carried `run_id=swarm-20260804-163317049`, i.e.
        # FOUR COPIES OF `think_off-n3-r2`, and F325 counted them as four independent runs. That
        # inflated the sample from 3 distinct runs to 7 and the run-clustered p from 0.0343 to
        # 3.7e-05.
        #
        # `run_started.ts` is written by the ENGINE at the moment the run began and no amount of
        # copying can alter it, so it is the provenance signal; `run_id` is the identity. Both come
        # from inside the log rather than from the filesystem around it.
        started = next((e.get("ts") for e in ev if e.get("event") == "run_started"), None)
        run_id = next((e.get("run_id") for e in ev if e.get("run_id")), None)
        if started is None or run_id is None:
            continue
        try:
            when = datetime.datetime.fromisoformat(str(started).replace("Z", "+00:00")).timestamp()
        except ValueError:
            continue
        if when < binary_mtime():
            continue                                  # produced by a DIFFERENT engine — see below
        if run_id in seen_runs:
            continue                                  # a parked COPY of a run already counted
        seen_runs.add(run_id)
        owned = {e["task_id"]: e.get("owned_files") or []
                 for e in ev if e.get("event") == "task_dispatched"}
        run_a = run_f = 0
        for e in ev:
            if e.get("event") != "task_completed":
                continue
            k = failures.kind_of(e["task_id"], owned.get(e["task_id"], []))
            slot = by_kind.setdefault(k, [0, 0])
            slot[0] += 1
            if e.get("status") != "done":
                slot[1] += 1
            if k == "test-author":
                run_a += 1
                run_f += e.get("status") != "done"
        if run_a:
            per_run.append((run_a, run_f))
    # slot[0] is incremented for EVERY task_completed event and slot[1] only for the failures, so
    # slot[0] is ATTEMPTED, not "completed", and the two overlap. Reporting `n = completed + failed`
    # therefore counts every failure TWICE. It has never shown because every sample until now had
    # ZERO failures, where the arithmetic happens to agree — the campaign's first test-author failure
    # is what exposed it. Kept under the old key names so stored history stays readable.
    ta = by_kind.get("test-author", [0, 0])
    total_failed = sum(f for _, f in by_kind.values())
    return {
        "test_author_completed": ta[0],
        "test_author_failed": ta[1],
        "all_failed": total_failed,
        "test_author_share_of_failures": (ta[1] / total_failed) if total_failed else 0.0,
        # The RUN is the independent unit; the task is not. Recorded so the significance test can
        # cluster, and so a row on disk carries the evidence for its own verdict.
        "runs_task_counts": [a for a, _ in per_run],
        "runs_clean": sum(1 for _, f in per_run if f == 0),
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


# The old-build test-author failure rate, the null this campaign is trying to beat: 13 of 42.
BASELINE_RATE = 13 / 42
SIGNIFICANCE = 0.05


def moved_significantly(m: dict) -> tuple[bool, float]:
    """Is this metric DIFFERENT from the old-build rate, or is it a small sample behaving normally?

    Returns (significant, p) where p = P(observing this few failures | rate unchanged).

    ⚠ THIS EXISTS BECAUSE THE FIRST VERSION OF `streak()` FLATTERED ME. It keyed on the raw metric
    dict, so ANY change reset the stall clock — and the very first sample on the new binary
    (test-author 5 completed / 0 failed) reset a 5-tick streak to 1. That sample has p = 0.157 under
    the null: roughly a one-in-six chance of appearing by luck with the failure rate completely
    unchanged. A detector built to force a shake-up after 10 quiet ticks that can be reset by noise
    is a detector that never fires. The loophole was in MY OWN instrument, written one hour earlier,
    to police exactly this.

    ⚠⚠ AND THEN IT FLATTERED ME THE OTHER WAY, FOR FIFTY TICKS. The version above computed a p ONLY
    in the `failed == 0` branch and returned `(False, 1.0)` unconditionally otherwise — so the moment
    a single failure was recorded the test became UNREACHABLE and no amount of subsequent success
    could ever move it. The detector printed "NOT SIGNIFICANT — this could be luck" against
    3 failures in 38 attempts where the null predicts 11.76 (task-level p = 0.00071), and against
    6 of 7 runs entirely clean where the null predicts 0.97 clean runs (run-clustered p = 3.7e-05).
    A counter that can only read one way is as broken as one that reads the wrong number (L153), and
    this one suppressed a real result while I looked elsewhere for fifty ticks.

    THE UNIT IS THE RUN, NOT THE TASK (L114). All three failures land in ONE run and six of seven are
    clean, so 38 task attempts are nowhere near 38 independent trials. When the metric carries a
    run-level breakdown this uses the exact Poisson-binomial over runs; the task-level test is the
    fallback for the rows already on disk, which predate those fields.

    ⚠ THIS FIX SILENCES MY OWN STALL ALARM. That is exactly the shape of change to distrust (L90), so
    the self-test asserts the detector can still say NO: a metric sitting AT the null must read
    not-significant on both paths, and `failed == 0, n == 5` — the noise sample that reset a 5-tick
    streak and prompted this whole guard — must still read not-significant.
    """
    runs = m.get("runs_task_counts")
    clean = m.get("runs_clean")
    if runs and clean is not None:
        # P(at least `clean` of these runs see ZERO failures | per-task rate unchanged).
        p_zero = [(1 - BASELINE_RATE) ** k for k in runs]
        dist = [1.0]
        for q in p_zero:
            nd = [0.0] * (len(dist) + 1)
            for i, v in enumerate(dist):
                nd[i] += v * (1 - q)
                nd[i + 1] += v * q
            dist = nd
        p = sum(dist[clean:])
        return p < SIGNIFICANCE, p

    completed = m.get("test_author_completed", 0)
    failed = m.get("test_author_failed", 0)
    n = completed          # ATTEMPTED — `completed` already includes the failures (see the note above)
    if n == 0:
        return False, 1.0
    # One-sided: P(X <= failed | X ~ Binomial(n, BASELINE_RATE)) — "this engine fails LESS often".
    # At failed == 0 this reduces to (1 - rate)**n, so the old correct branch is subsumed, not lost.
    p = sum(comb(n, k) * BASELINE_RATE ** k * (1 - BASELINE_RATE) ** (n - k)
            for k in range(0, min(failed, n) + 1))
    return p < SIGNIFICANCE, p


def normalise(hist: list[dict]) -> list[dict]:
    """Fill state forward across rows. Repairs the 85 rows already on disk WITHOUT rewriting them.

    Editing the log to erase a bad row would be falsifying the record; deriving the true state from it
    is not. A row whose `mini_goal` is "(unstated)" never meant the goal had been abandoned — it meant
    the flag was not typed — so every reader must resolve it the same way (L55: a lesson learned by
    one function is not learned by another).
    """
    out, mini, resolved = [], "(unstated)", []
    for r in hist:
        if r.get("mini_goal") and r["mini_goal"] != "(unstated)":
            mini = r["mini_goal"]
        if r.get("resolved"):
            resolved = r["resolved"]
        out.append({**r, "mini_goal": mini, "resolved": resolved})
    return out


def streak(hist: list[dict]) -> int:
    """Consecutive ticks with no change to the mini-goal, the resolved list, or a SIGNIFICANT metric.

    Deliberately NOT keyed on the raw metric — see `moved_significantly`. A metric that wobbles
    inside its own noise is the same state, not a new one. And keyed on the NORMALISED history, so a
    tick that simply omitted a flag cannot masquerade as a change of state (F290).
    """
    if not hist:
        return 0
    hist = normalise(hist)

    def key(r: dict) -> str:
        sig, _ = moved_significantly(r.get("metric") or {})
        return json.dumps([r.get("mini_goal"), sorted(r.get("resolved") or []), sig], sort_keys=True)

    newest = key(hist[-1])
    n = 0
    for r in reversed(hist):
        if key(r) != newest:
            break
        n += 1
    return n


def self_test() -> int:
    """Controls in BOTH directions. The bug being fixed was a counter that only ever read LOW."""
    m_flat = {"test_author_completed": 10, "test_author_failed": 3}
    rows = [{"mini_goal": "G", "resolved": ["F207"], "metric": m_flat} for _ in range(13)]
    assert streak(rows) == 13, "a genuinely unchanged run of 13 must read 13"
    # THE BUG: one bare tick used to reset this to 1.
    assert streak(rows + [{"mini_goal": "(unstated)", "resolved": [], "metric": m_flat}]) == 14, \
        "F290: omitting a flag must NOT reset the stall clock"
    # ...and the counter must still be ABLE to reset, or the fix has merely broken it the other way.
    assert streak(rows + [{"mini_goal": "DIFFERENT", "resolved": ["F207"], "metric": m_flat}]) == 1, \
        "a real change of mini-goal MUST reset the clock"
    assert streak(rows + [{"mini_goal": "G", "resolved": ["F207", "F999"], "metric": m_flat}]) == 1, \
        "resolving something new MUST reset the clock"
    m_moved = {"test_author_completed": 12, "test_author_failed": 0}
    assert moved_significantly(m_moved)[0], "the control metric must actually be significant"
    assert streak(rows + [{"mini_goal": "G", "resolved": ["F207"], "metric": m_moved}]) == 1, \
        "a SIGNIFICANT metric move MUST reset the clock"
    assert streak([]) == 0, "no history must score nothing, never a pass"

    # ── THE FIFTY-TICK BUG, ASSERTED SO IT CANNOT COME BACK ──────────────────────────────────────
    # The old code computed a p ONLY when failed == 0 and returned (False, 1.0) otherwise, so one
    # failure made the test unreachable forever. These are the real numbers it suppressed.
    sig, p = moved_significantly({"test_author_completed": 38, "test_author_failed": 3})
    assert sig and p < 0.01, f"3 failures in 38 against a null of 13/42 must READ significant (p={p})"
    # ...and the detector must still be ABLE to say NO. A metric sitting AT the null is the control.
    sig, p = moved_significantly({"test_author_completed": 38, "test_author_failed": 12})
    assert not sig, f"a metric AT the null (12 of 38, expected 11.8) must NOT read significant (p={p})"
    # The exact noise sample that reset a 5-tick streak and prompted the original guard. Still no.
    sig, p = moved_significantly({"test_author_completed": 5, "test_author_failed": 0})
    assert not sig and abs(p - (1 - BASELINE_RATE) ** 5) < 1e-12, \
        "failed == 0 must still reduce to (1-rate)**n — the old correct branch is subsumed, not lost"
    assert not moved_significantly({})[0], "an EMPTY metric must score nothing (all([]) is the trap)"
    assert not moved_significantly({"test_author_completed": 0, "test_author_failed": 0})[0], \
        "zero attempts cannot be evidence of anything"

    # ── RUN-CLUSTERED PATH (L114: the run is the independent unit, the task is not) ──────────────
    clustered = {"runs_task_counts": [5, 6, 7, 5, 5, 5, 5], "runs_clean": 6}
    sig, p = moved_significantly(clustered)
    assert sig and p < 0.001, f"6 of 7 runs clean where the null predicts ~1 must read significant (p={p})"
    assert not moved_significantly({**clustered, "runs_clean": 2})[0], \
        "2 of 7 clean is ordinary under the null and must NOT read significant"
    assert not moved_significantly({**clustered, "runs_clean": 0})[0], "zero clean runs is not a win"
    # The clustered path must WIN over the task-level fields when both are present, or the stricter
    # unit is decorative. Same dict, contradictory answers, clustered one takes it.
    both = {**clustered, "runs_clean": 2, "test_author_completed": 38, "test_author_failed": 3}
    assert not moved_significantly(both)[0], "run-level evidence must OVERRIDE the task-level fallback"

    print("self-test OK — omitted flags carry forward; goal/resolved/metric changes still reset; "
          "the significance test now reads BOTH ways and clusters by run")
    return 0


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


def carried(hist: list[dict]) -> tuple[str, list[str]]:
    """The last state actually asserted. An unsupplied flag means UNCHANGED, never "wiped" (F290)."""
    mini, resolved = "(unstated)", []
    for r in hist:
        if r.get("mini_goal") and r["mini_goal"] != "(unstated)":
            mini = r["mini_goal"]
        if r.get("resolved"):
            resolved = r["resolved"]
    return mini, resolved


def report(mini: str | None, resolved: list[str] | None, record: bool) -> int:
    m = measured_metric()
    hist = history()
    prev_mini, prev_resolved = carried(hist)
    if record:
        rec = {
            "ts": datetime.datetime.now().isoformat(timespec="seconds"),
            "mini_goal": mini or prev_mini,
            "resolved": resolved if resolved is not None else prev_resolved,
            "metric": m,
        }
        with LOG.open("a") as fh:
            fh.write(json.dumps(rec) + "\n")
        hist = hist + [rec]

    s = streak(hist)
    print(f"(1) HIS GOAL — resolves the SESSION: {GOAL}")
    if hist:
        shown = normalise(hist)[-1]
        print(f"(2) MINI-GOAL: {shown['mini_goal']}")
        print(f"(3) RESOLVED : {shown['resolved'] or 'ZERO'}")
    # "0 completed / 0 failed" and "0 failures out of many" are NOT the same statement and must never
    # print the same way. An empty sample is the absence of evidence; printing it as 0% would read as
    # a perfect score and would be the vacuous-truth trap (`all([])` is True).
    if m["test_author_completed"] == 0 and m["test_author_failed"] == 0:
        print("\nMEASURED: NO FINISHED RUN on the current binary yet — the metric has no sample. "
              "This is absence of evidence, not a zero failure rate.")
    else:
        sig, p = moved_significantly(m)
        n = m["test_author_completed"]   # attempted; failures are already inside it
        print(f"\nMEASURED (CURRENT binary only): test-author {m['test_author_completed']} completed "
              f"/ {m['test_author_failed']} failed  (n={n})")
        print(f"   vs the old-build rate 13/42 = {BASELINE_RATE:.1%}: "
              f"P(this good by chance | rate unchanged) = {p:.3f}  ⇒ "
              f"{'SIGNIFICANT' if sig else 'NOT SIGNIFICANT — this could be luck'}")
        if not sig and m["test_author_failed"] == 0:
            need = 1
            while (1 - BASELINE_RATE) ** need >= SIGNIFICANCE:
                need += 1
            print(f"   a clean run of {need} test-author completions would be needed to clear p<0.05")
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
    if "--self-test" in argv:
        return self_test()
    mini = None
    resolved = None
    if "--mini" in argv:
        mini = argv[argv.index("--mini") + 1]
    if "--resolved" in argv:
        resolved = [x.strip() for x in argv[argv.index("--resolved") + 1].split(";") if x.strip()]
    return report(mini, resolved, record="--tick" in argv)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

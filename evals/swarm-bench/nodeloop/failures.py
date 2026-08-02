#!/usr/bin/env python3
"""WHICH KIND of dispatch actually fails? Exit 0.

F164 is the campaign's headline and I derived it by hand, once, in a shell one-liner. That is exactly
the setup that produced F133/F137/F138/F145 — a number quoted for a dozen ticks that turned out to be
a median over a mixture — so it gets an instrument before it gets quoted again.

The finding it exists to keep honest, measured across 12 finished 3-node runs:

    implementer   65 completed,  0 failed     0%
    test-author   42 completed, 13 failed    31%
    verify/sink   97 completed,  1 failed     1%

Thirteen of fourteen failures are test-authors and no implementer has ever failed. So "did the swarm
get better" is NOT a question about a pooled score — it is a question about ONE cell of this table,
and any future claim of improvement has to move it.

THREE THINGS THIS REFUSES TO DO, each because it already went wrong once:

  · It never prints a pooled failure rate. A single number over three kinds whose rates are 0%, 31%
    and 1% is a fact about no population that exists (Lesson 45).
  · It asserts the PHYSICAL BOUNDS before printing anything — failed <= completed, rate <= 100%. An
    occupancy counter once reported 9 tasks in flight against 6 slots and the tell was arithmetic
    impossibility, not a wrong-looking trend (Lesson 47).
  · It classifies the SINK by task id BEFORE looking at owned files. `integrate-verify` is dispatched
    WITH owned files on some runs and without on others, so a files-first classifier silently filed
    the campaign's one sink failure under `implementer` — which is how "implementers have never
    failed" nearly became "implementers fail 2% of the time".

Usage:
    python3 failures.py              # the table, plus per-task and per-run-dir drift
    python3 failures.py --by-run     # also show every run's own contribution
"""
from __future__ import annotations

import collections
import glob
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
RUNS = HERE.parent / "runs"

# A test author is discriminated by the paths it OWNS. Not by the task id — `test-api-server` and
# `verify::api` both contain a kind word that lies about them — and not by whether "tests/" appears
# anywhere in the prompt, since every worker on a Python run sees that in the file layout.
TEST_PATH = re.compile(r"(test_[\w.-]+\.py|[\w.-]+_test\.py|[\w.-]+_test\.go|(^|/)tests?/)")
# The sink and the verifiers, by id. Checked FIRST — see the docstring.
SINK_ID = re.compile(r"^(integrate-verify|verify::|verify-e2e)")
KINDS = ("implementer", "test-author", "verify/sink")


def kind_of(task_id: str, owned: list[str]) -> str:
    if SINK_ID.match(task_id):
        return "verify/sink"
    if not owned:
        return "verify/sink"
    return "test-author" if any(TEST_PATH.search(p) for p in owned) else "implementer"


def load(path: str) -> list[dict]:
    out = []
    with open(path, errors="replace") as fh:
        for line in fh:
            if line.lstrip().startswith("{"):
                try:
                    out.append(json.loads(line))
                except Exception:
                    pass
    return out


def main(argv: list[str]) -> int:
    by_kind = {k: [0, 0] for k in KINDS}          # completed, failed
    by_task = collections.defaultdict(lambda: [0, 0])
    by_run = []
    unfinished = 0

    for f in sorted(glob.glob(str(RUNS / "**" / "run.jsonl"), recursive=True)):
        if "1node" in f or "2node" in f:
            continue
        ev = load(f)
        if not any(e.get("event") == "run_finished" for e in ev):
            unfinished += 1
            continue
        owned = {e["task_id"]: e.get("owned_files") or []
                 for e in ev if e.get("event") == "task_dispatched"}
        rc, rf = 0, 0
        for e in ev:
            if e.get("event") != "task_completed":
                continue
            k = kind_of(e["task_id"], owned.get(e["task_id"], []))
            bad = e.get("status") != "done"
            by_kind[k][0] += 1
            by_task[e["task_id"]][0] += 1
            rc += 1
            if bad:
                by_kind[k][1] += 1
                by_task[e["task_id"]][1] += 1
                rf += 1
        by_run.append((str(pathlib.Path(f).parent.relative_to(RUNS)), rc, rf))

    if not by_run:
        print("no FINISHED 3-node run found — that is a fact about the runs directory, not the engine.")
        return 0

    # Physical bounds FIRST. A table that cannot be true must never be read (Lesson 47).
    for k, (c, fl) in by_kind.items():
        assert fl <= c, f"{k}: {fl} failures out of {c} completions is impossible — instrument is wrong"
    tot_f = sum(v[1] for v in by_kind.values())
    tot_c = sum(v[0] for v in by_kind.values())
    assert tot_f <= tot_c, "more failures than completions overall — instrument is wrong"

    print(f"=== FAILURE BY DISPATCH KIND — {len(by_run)} finished 3-node run(s); "
          f"{unfinished} unfinished ignored ===")
    print("These runs span several engine builds, so this is NOT a controlled comparison. It is")
    print("stronger than one for this purpose: a concentration that survives every build change is")
    print("what a structural defect looks like and what a build-specific artefact does not.\n")
    print(f"{'kind':<14}{'completed':>11}{'failed':>8}{'rate':>8}")
    for k in KINDS:
        c, fl = by_kind[k]
        if not c:
            continue
        print(f"{k:<14}{c:>11}{fl:>8}{(100 * fl / c):>7.0f}%")
    print(f"\n{'':<14}{'':>11}{tot_f:>8}  failures total — and NO pooled rate is printed, because")
    print("kinds at 0% and 31% do not share a denominator that means anything.")

    if tot_f:
        share = 100 * by_kind["test-author"][1] / tot_f
        print(f"\ntest-authors are {by_kind['test-author'][1]}/{tot_f} = {share:.0f}% of ALL failures.")

    bad = sorted(((t, c, fl) for t, (c, fl) in by_task.items() if fl), key=lambda r: -r[2])
    if bad:
        print(f"\n{'task':<30}{'ran':>5}{'failed':>8}{'rate':>8}")
        for t, c, fl in bad:
            print(f"{t[:28]:<30}{c:>5}{fl:>8}{(100 * fl / c):>7.0f}%")

    if "--by-run" in argv:
        print(f"\n{'run':<52}{'completed':>11}{'failed':>8}")
        for name, c, fl in by_run:
            print(f"{name[:50]:<52}{c:>11}{fl:>8}")

    print("\nWHAT WOULD COUNT AS IMPROVEMENT: the test-author row moving. A better pooled score with")
    print("that row unchanged is the swarm getting luckier, not better (F147: a run scored 0.819")
    print("while LOSING a task).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

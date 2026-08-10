#!/usr/bin/env python3
"""Control for `attribute_root_causes` — in BOTH directions, against REAL stored verdicts.

An attribution that fires on every cell would relabel genuine independent defects as collateral,
which is the exact mirror of the error it was written to fix. So it is asserted against two real
verdicts on the current build: one whose sync returned nothing (must attribute exactly six blocked
checks) and one whose sync worked (must attribute NOTHING AT ALL).

Run from the nodeloop dir. Exit 0 = both directions hold; any mismatch aborts with a red line.
"""
from __future__ import annotations

import json
import os
import sys

assert os.path.basename(os.getcwd()) == "nodeloop", "run this from the nodeloop dir"

sys.path.insert(0, "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench")
from score_build import ROOT_BLOCKS, attribute_root_causes  # noqa: E402

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"
BROKEN, WORKING = "baseline-n3-r0", "baseline-n1-r0"
EXPECTED = sorted(ROOT_BLOCKS["sync_completeness"])


def rows(cell: str) -> list[dict]:
    p = os.path.join(RUNS, cell, "verdict.json")
    if not os.path.exists(p) or os.path.getsize(p) == 0:
        sys.exit(f"🔴 CONTROL UNRUNNABLE: {p} missing or empty")
    return json.loads(open(p).read())["checks"]


def main() -> int:
    broken, working = rows(BROKEN), rows(WORKING)

    # The fixture itself must be what this control assumes, or the control proves nothing about the
    # attribution — it would only prove that two files parse.
    sc = {r["check"]: r["score"] for r in broken}["sync_completeness"]
    sw = {r["check"]: r["score"] for r in working}["sync_completeness"]
    if sc != 0.0 or sw != 1.0:
        sys.exit(f"🔴 FIXTURE WRONG: {BROKEN} sync_completeness={sc} (want 0.0), "
                 f"{WORKING}={sw} (want 1.0). The two arms are not what they claim to be.")

    got = attribute_root_causes(broken)
    if sorted(got.get("sync_completeness", [])) != EXPECTED:
        sys.exit(f"🔴 POSITIVE ARM FAILED on {BROKEN}: attributed "
                 f"{sorted(got.get('sync_completeness', []))}, expected {EXPECTED}")

    clean = attribute_root_causes(working)
    if clean:
        sys.exit(f"🔴 NEGATIVE ARM FAILED on {WORKING}: attributed {clean} on a cell whose sync "
                 "WORKED. An attribution that fires everywhere excuses real defects.")

    # And a synthetic row set where the prerequisite PASSES but a dependent fails on its own: that is
    # an independent defect and must keep its own name.
    solo = [{"check": "sync_completeness", "score": 1.0},
            {"check": "summary_accuracy", "score": 0.0}]
    if attribute_root_causes(solo):
        sys.exit("🔴 a dependent that fails while its prerequisite PASSES is an independent defect "
                 "and must not be attributed away")

    print(f"✅ both directions hold: {BROKEN} attributes {len(EXPECTED)} blocked checks to "
          f"sync_completeness; {WORKING} attributes nothing; an independent failure keeps its name.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

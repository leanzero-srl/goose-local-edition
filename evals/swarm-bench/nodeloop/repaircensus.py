#!/usr/bin/env python3
"""Does the COMPLETE repair round ENGAGE, and does engagement differ by node count?

WHY THIS EXISTS. Pair r0 on the current binary reversed the sign (F750): 3-node 0.7226 vs 1-node
0.9283. Reading both cells' COMPLETE phase side by side showed an asymmetry that no score comparison
can see:

    baseline-n1-r0 (0.9283)  verify round 0 -> 1 finding -> fix dispatched -> round 1 clean
    baseline-n3-r0 (0.7226)  verify round 0 -> 0 findings -> NO fix ever dispatched

Both then emitted the SAME verdict: complete_result{passed:true, verified:false, remaining:0}. The
3-node run's own stderr says "complete: GREEN at round 0 — the built app runs and its checks pass"
about an app my scorer put at 0.7226. The phase that exists to refuse a red app engaged zero times on
the worse app and once on the better one.

⚠️ WHAT THIS INSTRUMENT DOES AND DOES NOT CLAIM. `complete_verify` findings are that checker's
opinion (an import/AST/behaviour review plus a bare-GET spec probe), NOT my Tier A-D scorer. "0
findings" therefore means "this checker saw nothing", never "the app is correct". So the census
reports ENGAGEMENT — a deterministic mechanism fact valid at n=1 — and deliberately does not compute
a findings-vs-score correlation, which would be a score claim on a handful of runs.

⚠️ NEVER POOLED ACROSS BUILDS. `engine_build` is the binary's mtime+size; the spec-contract code that
feeds `inconclusive_reasons` changed between builds, so a cross-build average is two experiments
added together. Every table is per-build.

THE CONTROL RUNS FIRST AND IN BOTH DIRECTIONS. One cell that DID repair (n1-r0) and one that did NOT
(n3-r0) are asserted by name before any aggregate prints. A census that cannot see a repair it is
known to contain is a blind instrument, and a census that reports a repair everywhere is a broken
matcher; this catches both.
"""
from __future__ import annotations

import json
import os
import sys
from collections import defaultdict

assert os.path.basename(os.getcwd()) == "nodeloop", \
    "run this from the nodeloop dir — every path below is relative to it"

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"

# (cell, expected round-0 findings, expected fix dispatches). Both directions, by name.
CONTROLS = [("baseline-n1-r0", 1, 1), ("baseline-n3-r0", 0, 0)]


def read_cell(cell: str) -> dict | None:
    d = os.path.join(RUNS, cell)
    res, log = os.path.join(d, "nodeloop-result.json"), os.path.join(d, "run.jsonl")
    if not (os.path.exists(res) and os.path.exists(log)):
        return None
    r = json.loads(open(res).read())
    ev = [json.loads(l) for l in open(log) if l.strip()]
    verifies = [e for e in ev if e.get("event") == "complete_verify"]
    fixes = [e for e in ev if e.get("event") == "complete_fix_dispatched"]
    result = next((e for e in ev if e.get("event") == "complete_result"), {})
    r0 = next((e for e in verifies if e.get("round") == 0), None)
    return {
        "cell": cell,
        "arm": r.get("arm"), "nodes": r.get("nodes"), "rep": r.get("rep"),
        "score": r.get("score"), "build": r.get("engine_build"),
        "void": r.get("void"), "timed_out": r.get("timed_out"),
        "verify_rounds": len(verifies),
        "round0_findings": (r0 or {}).get("findings"),
        "round0_ran": (r0 or {}).get("ran"),
        "fixes": len(fixes),
        "passed": result.get("passed"), "verified": result.get("verified"),
        "remaining": result.get("remaining_findings"),
    }


def controls(rows: dict[str, dict]) -> None:
    for cell, want_f, want_fx in CONTROLS:
        c = rows.get(cell)
        if c is None:
            sys.exit(f"🔴 CONTROL FAILED: {cell} unreadable — the census cannot see its own corpus")
        if c["round0_findings"] != want_f or c["fixes"] != want_fx:
            sys.exit(f"🔴 CONTROL FAILED: {cell} read findings={c['round0_findings']} "
                     f"fixes={c['fixes']}, expected {want_f}/{want_fx}. The parser is wrong; no "
                     "aggregate below may be believed.")
    print(f"controls: {CONTROLS[0][0]} repaired (1 finding, 1 fix) and {CONTROLS[1][0]} did not "
          "(0/0) — both directions read correctly ✅\n")


def main() -> int:
    cells = sorted(os.listdir(RUNS))
    rows = {}
    for c in cells:
        r = read_cell(c)
        if r:
            rows[c] = r
    if not rows:
        sys.exit("🔴 no readable cells — empty corpus scores nothing, it does not score clean")
    controls(rows)

    by_build: dict[str, list] = defaultdict(list)
    for r in rows.values():
        if not r["void"]:
            by_build[r["build"]].append(r)

    for build in sorted(by_build, key=lambda b: -len(by_build[b])):
        rs = by_build[build]
        print(f"===== engine_build {build}   ({len(rs)} non-void cells)")
        print(f"  {'nodes':>5} {'cells':>5} {'repaired':>9} {'rate':>6} "
              f"{'mean r0 findings':>17} {'mean verify rounds':>19}")
        for n in sorted({r["nodes"] for r in rs}):
            g = [r for r in rs if r["nodes"] == n]
            engaged = [r for r in g if r["fixes"] > 0]
            f0 = [r["round0_findings"] for r in g if r["round0_findings"] is not None]
            vr = [r["verify_rounds"] for r in g]
            mf = f"{sum(f0)/len(f0):.2f}" if f0 else "n/a"
            mv = f"{sum(vr)/len(vr):.2f}" if vr else "n/a"
            print(f"  {n:>5} {len(g):>5} {len(engaged):>9} "
                  f"{len(engaged)/len(g):>6.0%} {mf:>17} {mv:>19}")
        greens = [r for r in rs if r["fixes"] == 0 and r["passed"] and r["round0_findings"] == 0]
        if greens:
            print(f"  round-0 GREEN with no repair: {len(greens)}/{len(rs)} cells — "
                  f"scores {', '.join(f'{r['score']:.3f}' for r in sorted(greens, key=lambda x: x['score']))}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())

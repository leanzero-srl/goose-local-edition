#!/usr/bin/env python3
"""`uses_max_limit` grades the BUILD, not the delivered client — measured, not argued.

THE DEFECT. `score_build._trace_split` returns four values. Three are scoped to the graded phase:
`sync1_reqs` reads `lists(first)`, `sync2_reqs` and `sync2_304` read `lists(second)`, where `first`
and `second` are the segments after the `__phase__` markers. The fourth, `max_limit_used`, reads
`lists(trace)` — THE WHOLE TRACE, including every request the agent made while building and testing
itself. All seven tier-C checks scope through `vendor_trace.exercise_phase`. This one field does not.

WHY THAT IS A DEFECT AND NOT A DESIGN CHOICE. Three independent reasons, in ascending strength:

1. There is no comment justifying it, while the three fields beside it in the same return statement
   are scoped. It reads as an oversight, not an intention. (Contrast the POST-probe decision in the
   engine, which IS justified in-source and therefore stands.)
2. The check's own stated consequence is "not using the documented maximum page size costs avoidable
   round trips". That is a claim about the DELIVERED client's runtime cost. Requests the agent fired
   while developing cost zero round trips in production, so the check contradicts its own harm
   statement.
3. It is the exact failure the phase marker was built to prevent. `vendor_service.mark_phase`'s
   docstring records the measurement: "Opus's one-shot 429 was consumed at trace seq 3 while the
   graded run began at seq 38, so the retry check graded throwaway scratch code." The marker fixed
   that for the retry checks and for three of the four `_trace_split` fields. It missed this one.

MEASURED PREVALENCE — 2 of 8 archived traces, but the magnitude is the point, not the count:

  baseline-n1-r2   whole-trace max 100, graded-phase max 0  =>  scores 1.00, should score 0.00
  baseline-n3-r0   whole-trace max   2, graded-phase max 0  =>  scores 0.02, should score 0.00

THE THREE-WAY CONTROL FALLS OUT OF ONE TRACE. On `baseline-n1-r2`, `_trace_split` returns
`sync1_reqs=0` AND `max_limit_used=100` — from the same call, on the same trace. The phase-scoped
field correctly reports that the delivered client made NO vendor list requests; the whole-trace field
simultaneously awards full marks for page-size efficiency. That app is the one whose `/api/sync`
raises on every call (a missing `int()` around `Content-Length`), so its sync is entirely dead. The
scorer gave a completely broken client a perfect score on "uses the documented maximum page size".
`baseline-n1-r1` is the negative control: whole and graded both 100, no false credit, because that
client actually works.

⚠️ THE BIAS RUNS TOWARD THE ARM I WANT TO BEAT, WHICH IS WHY THIS IS NOT SELF-APPLIED. The leak
inflates BROKEN clients, and in this sample it handed +1.00 to a 1-node cell and +0.02 to a 3-node
cell. Correcting it therefore nudges the comparison toward the 3-node hypothesis. A fix that helps
your own hypothesis, applied quietly in the middle of a measurement, is indistinguishable from
fitting the instrument to the answer — so THE FIX IS NOT APPLIED HERE. It is queued as a patch
alongside the frozen engine commits and lands at a `scorer_version` boundary with a re-score, never
silently mid-campaign. Composite impact is bounded at 0.02 (one of ten tier-D checks at weight 0.20).
"""
import json
import sys
from pathlib import Path

RUNS = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop")
PHASE_MARKER = "__phase__"


def graded_phase(trace: list) -> list:
    """Mirrors `vendor_trace.exercise_phase`: ANY marker opens the graded window."""
    for i, e in enumerate(trace):
        if e.get(PHASE_MARKER):
            return trace[i + 1:]
    return trace


def list_gets(seg: list) -> list:
    return [e for e in seg if e.get("path") == "/v1/payments" and e.get("method") == "GET"]


def max_limit(seg: list) -> int:
    v = [int(e["query"]["limit"]) for e in list_gets(seg)
         if str(e.get("query", {}).get("limit", "")).isdigit()]
    return max(v) if v else 0


def scan(runs=RUNS) -> list:
    out = []
    for p in sorted(runs.glob("*/vendor-trace.jsonl")):
        try:
            t = [json.loads(l) for l in p.read_text(errors="replace").splitlines() if l.strip()]
        except (json.JSONDecodeError, OSError):
            continue
        if not t:
            continue
        marked = any(e.get(PHASE_MARKER) for e in t)
        g = graded_phase(t)
        whole, grad = max_limit(t), max_limit(g)
        out.append({"cell": p.parent.name, "entries": len(t), "marked": marked,
                    "pre_gets": len(list_gets(t)) - len(list_gets(g)),
                    "graded_gets": len(list_gets(g)),
                    "whole_max": whole, "graded_max": grad,
                    "scored": min(whole / 100, 1.0) if whole else 0.0,
                    "correct": min(grad / 100, 1.0) if grad else 0.0})
    return out


def report(rows: list) -> str:
    leaks = [r for r in rows if r["whole_max"] != r["graded_max"]]
    L = [f"uses_max_limit PHASE LEAK   {len(leaks)} of {len(rows)} archived traces credit "
         f"build-time requests", "",
         f"  {'cell':<26}{'entries':>8}{'preGET':>8}{'gradGET':>9}{'whole':>7}{'graded':>8}"
         f"{'scored':>8}{'correct':>9}  "]
    for r in rows:
        flag = "  *** LEAK" if r["whole_max"] != r["graded_max"] else ""
        if not r["marked"]:
            flag += "  (no marker — run incomplete, whole trace is the graded phase)"
        L.append(f"  {r['cell']:<26}{r['entries']:>8}{r['pre_gets']:>8}{r['graded_gets']:>9}"
                 f"{r['whole_max']:>7}{r['graded_max']:>8}{r['scored']:>8.2f}{r['correct']:>9.2f}"
                 f"{flag}")
    if leaks:
        L.append("")
        for r in leaks:
            L.append(f"  {r['cell']}: uses_max_limit scores {r['scored']:.2f}, should be "
                     f"{r['correct']:.2f} — the delivered client made {r['graded_gets']} graded "
                     f"vendor list request(s)")
        L.append(f"  composite impact per leaking cell: up to "
                 f"{max(r['scored'] - r['correct'] for r in leaks) * 0.20 / 10:.4f} "
                 f"(1 of 10 tier-D checks, tier weight 0.20)")
    return "\n".join(L)


def self_test() -> int:
    """Controls in BOTH directions plus the specificity case the real corpus handed us."""
    fails = []
    mark = {PHASE_MARKER: "sync1", "method": "-", "path": "-", "status": 0, "query": {}}
    get = lambda lim: {"path": "/v1/payments", "method": "GET", "status": 200,
                       "query": {"limit": str(lim)}}  # noqa: E731

    # POSITIVE: build-time limit=100, graded phase empty => the leak must be visible.
    t = [get(100), get(100), mark]
    if max_limit(t) != 100 or max_limit(graded_phase(t)) != 0:
        fails.append("did not detect a build-only limit=100 as a leak")

    # NEGATIVE: the same limit on both sides must NOT read as a leak.
    t = [get(100), mark, get(100)]
    if max_limit(t) != max_limit(graded_phase(t)):
        fails.append("flagged a leak when whole and graded agree")

    # SPECIFICITY: a graded request at a LOWER limit than the build must still be a leak, and the
    # corrected score must be the graded one — not the max of the two.
    t = [get(100), mark, get(25)]
    if max_limit(graded_phase(t)) != 25:
        fails.append(f"graded max was {max_limit(graded_phase(t))}, expected 25 not 100")

    # VACUOUS TRUTH: an empty trace must score NOTHING, never full marks. `all([])` is True and
    # `max([])` raises; both are how this check would silently pass on no evidence.
    if max_limit([]) != 0 or max_limit(graded_phase([])) != 0:
        fails.append("empty trace did not score zero")

    # NO MARKER: an older/incomplete trace grades everything rather than nothing.
    t = [get(50)]
    if max_limit(graded_phase(t)) != 50:
        fails.append("an unmarked trace was scoped to nothing instead of graded whole")

    # A non-list request must never contribute a limit.
    t = [{"path": "/v1/docs", "method": "GET", "status": 200, "query": {"limit": "100"}}, mark]
    if max_limit(t) != 0:
        fails.append("a non /v1/payments request contributed a limit")

    # THE REAL INPUT, not a fixture: the corpus case must still reproduce.
    real = RUNS / "baseline-n1-r2" / "vendor-trace.jsonl"
    if real.is_file():
        t = [json.loads(l) for l in real.read_text().splitlines() if l.strip()]
        if not (max_limit(t) == 100 and max_limit(graded_phase(t)) == 0):
            fails.append(f"baseline-n1-r2 no longer reproduces: whole={max_limit(t)} "
                         f"graded={max_limit(graded_phase(t))}")
    for f in fails:
        print(f"  FAIL {f}")
    print(f"tracescope self-test: {'PASS' if not fails else str(len(fails)) + ' FAILURES'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(self_test())
    print(report(scan()))

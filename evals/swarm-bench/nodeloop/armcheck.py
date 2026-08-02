#!/usr/bin/env python3
"""Can each queued ARM actually answer its own question? Exit 0 (1 if a queued arm cannot).

TWICE IN ONE DAY an arm was bought with fleet time it could not repay:

  * `kind_prompt` (F111) — its readout was CIRCULAR. `dispatch_audit` computed
    `mismatched = ... if not kind_prompt_on else 0`, so with the lever ON the count was HARDCODED to
    zero. "kind_mismatch_pct falls toward zero" would have succeeded BY CONSTRUCTION, on the very run
    bought to test it.
  * `retarget_off` (F117/F118) — its MECHANISM never fires. The redraft ladder runs only when
    `plan_confidence < ask_floor`, and plan_confidence is 88 in 8 of 13 archived runs against a floor
    of 85. So the arm switched off something already absent: a null experiment dressed as a comparison.

Both would have been caught by asking, BEFORE the run, two questions the baseline can answer:

    1. does the arm's MECHANISM fire on the baseline at all?   (else there is nothing to change)
    2. can the INSTRUMENT see the change?                      (else the readout is unearned)

That is what this script does. It is deliberately CONSERVATIVE: an arm whose precondition cannot be
decided from the baseline's own events is reported UNKNOWN, never OK. A green here is not proof the
arm will produce a result — it is proof the arm is not already doomed, which is the cheap half.

Usage:
    python3 armcheck.py [<baseline-run-dir>]      # defaults to the newest completed baseline
"""
from __future__ import annotations

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"

# For each arm: what must be TRUE of a baseline run for the arm to be able to change anything, and how
# the change would be READ. `probe` returns (verdict, why) given the baseline's event list.
#
# The distinction that matters: PRECONDITION is about the engine (can this fire?), READOUT is about the
# instrument (could we tell?). An arm needs BOTH, and F111 failed only the second.


def _count(ev, name):
    return sum(1 for e in ev if e.get("event") == name)


def _plan(ev):
    return next((e for e in ev if e.get("event") == "plan_loaded"), None)


def arm_kind_prompt(ev):
    n = _count(ev, "task_dispatched")
    if not n:
        return "UNKNOWN", "no dispatches in the baseline"
    # PRECONDITION: dispatches of a kind other than implementer exist (else no rules to re-target).
    # READOUT: `rules_delivered` must be in the log, or the mismatch metric is unmeasurable when the
    # lever is on — the exact circularity F111 found.
    if _count(ev, "rules_delivered") == 0:
        return "BLOCKED", ("no `rules_delivered` events — with the lever ON the delivered rule-set is "
                           "unprovable and the readout is circular (F111). Needs the post-F111 engine.")
    return "OK", f"{n} dispatches and rules_delivered present"


def arm_retarget_off(ev):
    pl = _plan(ev)
    if not pl:
        return "UNKNOWN", "no plan_loaded"
    conf, floor = pl.get("plan_confidence"), pl.get("ask_floor")
    if conf is None or floor is None:
        return "UNKNOWN", "plan_loaded lacks confidence/floor"
    if conf >= floor:
        return "BLOCKED", (f"plan_confidence {conf} >= ask_floor {floor}, so the redraft ladder never "
                           f"runs on this baseline — switching it off changes nothing (F117/F118)")
    return "OK", f"plan_confidence {conf} < ask_floor {floor}: the ladder fires"


def arm_doc_prefetch(ev):
    rc = next((e for e in ev if e.get("event") == "research_completed"), None)
    if not rc:
        return "UNKNOWN", "no research_completed"
    g = rc.get("grounded")
    if not g:
        return "BLOCKED", ("grounded == 0, so doc_facts would be EMPTY and the verbatim channel carries "
                           "nothing — inert by construction (F84)")
    return "OK", f"grounded={g}: the verbatim channel has content to carry"


def arm_spec_repair(ev):
    rounds = _count(ev, "complete_verify")
    if rounds == 0:
        return "BLOCKED", "no repair round ran on the baseline — nothing to race"
    return "OK", f"{rounds} repair round(s): the race has work"


def arm_complete_parallel(ev):
    # Fans by FAILING FILE, so it needs a round whose findings name >1 distinct file.
    best = 0
    for e in ev:
        if e.get("event") == "complete_verify":
            best = max(best, e.get("findings") or 0)
    if best <= 1:
        return "BLOCKED", (f"max findings in any round = {best}; the fan is per-file so it can never "
                           f"exceed one shard (F73)")
    return "OK", f"a round had {best} findings: the fan has >1 item"


def arm_sink_review(ev):
    if _count(ev, "sink_review"):
        return "OK", "sink_review already fires"
    solo = any(e.get("event") == "task_dispatched" and e.get("task_id") == "integrate-verify" for e in ev)
    if not solo:
        return "UNKNOWN", "no integrate-verify dispatch to idle-fill around"
    return "UNKNOWN", ("the sink ran, but whether idle capacity existed during it is not decidable from "
                       "these events alone — needs occupancy's solo window")


def arm_detail_budget(ev):
    # Raising the detail ceiling only matters if a detail call is being CUT by it.
    secs = [e.get("secs") for e in ev if e.get("event") == "detail_completed" and e.get("secs")]
    budget = next((e.get("budget_secs") for e in ev if e.get("event") == "detail_completed"), None)
    if not secs:
        return "UNKNOWN", "no detail_completed events"
    if budget and max(secs) < budget * 0.8:
        return "BLOCKED", (f"slowest detail {max(secs):.0f}s vs budget {budget}s — nothing is near the "
                           f"ceiling, so raising it changes nothing")
    return "OK", f"slowest detail {max(secs):.0f}s against budget {budget}s"


def arm_e2e_oracle(ev):
    if _count(ev, "task_dispatched") == 0:
        return "UNKNOWN", "no dispatches"
    shards = sum(1 for e in ev if e.get("event") == "task_dispatched"
                 and str(e.get("task_id", "")).startswith("verify-e2e::"))
    if shards == 0:
        return "BLOCKED", "no verify-e2e:: shards ran — the oracle has nothing to re-source"
    return "OK", f"{shards} e2e shard(s) ran"


def arm_doc_fetch(ev):
    return "UNKNOWN", ("needs the spec to name a fetchable document; not decidable from run events "
                       "alone — check spec_doc_urls against the run prompt")


ARMS = {
    "kind_prompt": arm_kind_prompt,
    "doc_prefetch": arm_doc_prefetch,
    "spec_repair": arm_spec_repair,
    "detail_budget": arm_detail_budget,
    "complete_parallel": arm_complete_parallel,
    "e2e_oracle": arm_e2e_oracle,
    "retarget_off": arm_retarget_off,
    "sink_review": arm_sink_review,
    "doc_fetch": arm_doc_fetch,
}


def newest_baseline() -> pathlib.Path | None:
    cands = [p.parent for p in RUNS.glob("baseline*/run.jsonl")]
    if not cands:
        cands = [p.parent for p in RUNS.glob("*/run.jsonl")]
    return max(cands, key=lambda p: p.stat().st_mtime) if cands else None


def main(argv: list[str]) -> int:
    run = pathlib.Path(argv[0]) if argv else newest_baseline()
    if not run or not run.is_dir():
        print("no baseline run to check against")
        return 0
    ev = []
    for f in sorted(run.glob("*.jsonl")):
        for line in f.read_text(errors="replace").splitlines():
            line = line.strip()
            if line:
                try:
                    ev.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    if not ev:
        print(f"{run.name}: no events")
        return 0
    print(f"=== ARM CHECK against {run.name} ===")
    print("can each queued arm change anything, and could we tell?\n")
    blocked = 0
    for name, probe in ARMS.items():
        verdict, why = probe(ev)
        if verdict == "BLOCKED":
            blocked += 1
        print(f"  {verdict:<8} {name:<20} {why}")
    print()
    if blocked:
        print(f"{blocked} arm(s) BLOCKED — each would spend a fleet unit and answer nothing. Fix the "
              f"precondition or the instrument first; do NOT queue them.")
        return 1
    print("no arm is provably doomed. That is the cheap half — it does not promise a result.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

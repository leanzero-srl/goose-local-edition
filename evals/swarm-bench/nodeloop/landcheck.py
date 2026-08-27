#!/usr/bin/env python3
"""Did this batch's new fields actually EMIT in a real run? (the half `probe.py` cannot answer)

TWO DIFFERENT QUESTIONS, AND ONLY BOTH TOGETHER MEAN "IT LANDED":

    probe.py    is the literal IN THE BINARY?      — proves the rebuild carried the edit
    landcheck   did the field APPEAR IN A RUN LOG?  — proves the code PATH actually executes

A literal can sit in the binary behind a branch nothing takes. `doc_fetch` is the standing proof:
the orchestrator fetch has existed for weeks and emitted **zero** `doc_fetched` events in 17,215
lines across 54 logs, because the config defaulted false. Present in the binary, absent from every
run. So a green probe is necessary and NOT sufficient.

⚠️ THIS IS ALSO THE ONLY CHECK FOR C6. `inconclusive_reasons` was ALREADY PRESENT in the pre-batch
binary — it ships on `spec_contract` and C6 deliberately reuses the name on `complete_verify`. A
string probe therefore cannot attribute it to this batch (F716 called that vacuous rather than
letting it pass). Here it IS attributable, because the pairing (event, field) is what changed: the
field on `complete_verify` is new even though the field name is not.

THE NEGATIVE CONTROL IS FREE AND MANDATORY. Every pre-batch log is on disk, so each expectation is
checked against the archive too. A field that also appears in pre-batch logs is NOT evidence the
batch landed, and this script says so rather than counting it as a pass.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"
ARCHIVE = HERE.parent / "runs" / "nodeloop-SNAPSHOT-pre-batch-2026-08-09"

# (event, field, what it is). The PAIR is the unit — `inconclusive_reasons` is only new on
# `complete_verify`, and checking the bare field name would silently pass on `spec_contract`.
# The four C5(A)/F707 rows that stood here checked fields on `skeleton_drafts` and `plan_convergence`,
# both of which the OPEN -> ASK -> RESEARCH -> SYNTHESIS -> REVIEW rewrite deleted along with the plan
# vote and the redraft ladder. Left in place they would have read INERT forever — and an INERT row is
# neither a pass nor a failure, so the script's exit code would have been a permanent 2 that says
# nothing. The linear engine's own counters replace them one for one: each is a NUMBER the phase
# computes rather than a flag it copies, so a row reading MISSING means the phase ran and produced
# nothing to count, which is the failure worth catching.
EXPECTED = [
    ("task_dispatched", "description_chars", "C12(b) — the split child's instruction length"),
    ("slices_opened", "weights", "OPEN — the per-slice work estimate the re-cut trigger reads"),
    ("research_completed", "brief_chars", "RESEARCH — how much spec each slice owner actually wrote"),
    ("review_findings", "patch_touches", "REVIEW — how many tasks the round's patch moved"),
    ("complete_verify", "inconclusive_reasons", "C6(1) — why the run abstained (STRING PROBE CANNOT SEE THIS)"),
    ("defects_rated", "engine_forced", "RATE — findings the engine called critical over the rater"),
]


GOOSE = Path("/Users/mihaiperdum/Projects/goose/target/release/goose")


def logs_under(root: Path) -> list[Path]:
    if not root.is_dir():
        return []
    return sorted(root.glob("*/run.jsonl")) + sorted(root.glob("cells/*/run.jsonl"))


def started_after_build(path: Path, build_mtime: float) -> bool:
    """Did this run start on the CURRENT binary?

    ⚠️ WITHOUT THIS THE INSTRUMENT POOLS ACROSS ENGINE BUILDS — the cardinal sin of this campaign,
    committed here by omission. `runs/nodeloop/` keeps pre-rebuild cell dirs alongside the new one, so
    the first post-rebuild reading counted 293 `task_dispatched` events and called the new field
    MISSING on all of them. Every one of those events was written by the OLD binary, which cannot
    emit a field that did not exist when it was compiled. The correct answer was INERT — the new run
    was six events in and had not reached dispatch.

    A run is POST if its `run_started` timestamp is later than the binary's mtime. That is the same
    vintage test `ladderwatch.py` applies via `engine_build`, and it is decided from the log's own
    first line rather than from its directory name.
    """
    try:
        with path.open() as fh:
            for line in fh:
                line = line.strip()
                if not line.startswith("{"):
                    continue
                ev = json.loads(line)
                if ev.get("event") != "run_started":
                    continue
                ts = ev.get("ts")
                if not ts:
                    return False
                import datetime as _dt
                started = _dt.datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()
                return started >= build_mtime
    except (OSError, json.JSONDecodeError, ValueError):
        return False
    return False


def scan(paths: list[Path]) -> dict:
    """{(event, field): count} plus per-event totals, so 'field absent' and 'event never fired'
    are distinguishable. Conflating them is how an INERT lever gets reported as a broken one."""
    pairs: dict = {}
    events: dict = {}
    for p in paths:
        try:
            for line in p.open():
                line = line.strip()
                if not line or not line.startswith("{"):
                    continue
                try:
                    ev = json.loads(line)
                except json.JSONDecodeError:
                    continue
                name = ev.get("event")
                if not name:
                    continue
                events[name] = events.get(name, 0) + 1
                for k in ev:
                    pairs[(name, k)] = pairs.get((name, k), 0) + 1
        except OSError:
            continue
    return {"pairs": pairs, "events": events}


def main() -> int:
    build_mtime = GOOSE.stat().st_mtime
    all_live = logs_under(RUNS)
    post = [p for p in all_live if started_after_build(p, build_mtime)]
    pre_live = [p for p in all_live if p not in post]
    live = scan(post)
    old = scan(logs_under(ARCHIVE) + pre_live)
    import time as _t
    print(f"binary built {_t.strftime('%Y-%m-%d %H:%M:%S', _t.localtime(build_mtime))}")
    print(f"POST-rebuild logs {len(post):3d}   pre-rebuild {len(pre_live):3d}   archive "
          f"{len(logs_under(ARCHIVE)):3d}   (pre-rebuild logs are POOLED INTO THE 'pre' COLUMN, "
          "never the live one)")
    if not post:
        print("— NO RUN HAS STARTED ON THIS BINARY YET. Nothing can be attributed; not a failure.")
        return 2
    if not live["events"]:
        print("🔴 no events in the post-rebuild logs — instrument blind.")
        return 2
    # POSITIVE CONTROL, on the same objects, before any zero below is allowed to mean anything.
    # `inconclusive_reasons` reading 0 on `complete_verify` must mean "not emitted there yet", NOT
    # "this scan cannot see that field name". It ships on `spec_contract` today, so if the scan is
    # honest it MUST find it there. Without this, C6's whole verdict rests on an unproven negative —
    # the exact failure this campaign has hit six times.
    # Scanned across BOTH vintages on purpose. This control asks "can this scanner see that field
    # name at all", which is a property of the SCANNER — not of the new run, which may simply not
    # have reached `spec_contract` yet. Scoping it to post-rebuild logs made it fail on a healthy
    # six-event run and refuse to report anything, which is a blind instrument of the opposite kind.
    ctl = live["pairs"].get(("spec_contract", "inconclusive_reasons"), 0) \
        + old["pairs"].get(("spec_contract", "inconclusive_reasons"), 0)
    if ctl:
        print(f"positive control: `inconclusive_reasons` found {ctl}x on spec_contract — the scan "
              "can see this field name, so a 0 on complete_verify is a real absence ✅")
    else:
        print("🔴 POSITIVE CONTROL FAILED: `inconclusive_reasons` not found on spec_contract either, "
              "where it is known to ship. The scan is BLIND — no zero below means anything.")
        return 2
    print()
    print(f"{'event':22s} {'field':28s} {'live':>6s} {'pre':>5s}  verdict")
    print("-" * 88)
    ok = True
    for event, field, why in EXPECTED:
        n = live["pairs"].get((event, field), 0)
        n_old = old["pairs"].get((event, field), 0)
        fired = live["events"].get(event, 0)
        if n and not n_old:
            verdict = "✅ LANDED"
        elif n and n_old:
            verdict = "⚠️  present PRE-batch too — cannot attribute"
            ok = False
        elif fired == 0:
            # The distinction that stops a false alarm: the event never happened, so the field had
            # no chance to appear. INERT, not broken — and it says nothing either way.
            verdict = f"— INERT ({event} never fired live yet)"
        else:
            verdict = f"🔴 MISSING on {fired} live {event} event(s)"
            ok = False
        print(f"{event:22s} {field:28s} {n:6d} {n_old:5d}  {verdict}   {why}")
    print("-" * 88)
    inert = [e for e, f, _ in EXPECTED if not live["events"].get(e)]
    if inert:
        print(f"⚠️  events that have not fired live yet: {sorted(set(inert))} — re-run after a")
        print("   completed post-rebuild cell. An INERT row is NOT a pass and NOT a failure.")
    # ⚠️ AN ALL-INERT RUN IS NOT A PASS. The first version of this line printed "every checked field
    # landed" and exited 0 while every single row was INERT — nothing had been attributed at all,
    # because `ok` only ever went False on a real failure. That is the vacuous-truth failure this
    # whole file exists to prevent, committed in its own summary: a check that examined nothing
    # reporting success. LANDED must be counted, not inferred from the absence of failures.
    landed = sum(1 for e, f, _ in EXPECTED
                 if live["pairs"].get((e, f), 0) and not old["pairs"].get((e, f), 0))
    if not ok:
        print("🔴 at least one field did not land, or cannot be attributed to this batch")
        return 1
    if landed < len(EXPECTED):
        print(f"— PENDING: {landed}/{len(EXPECTED)} fields attributed; the rest are INERT because "
              "their events have not fired on this binary yet. NOT a pass — re-run after a "
              "completed post-rebuild cell.")
        return 2
    print(f"✅ all {landed}/{len(EXPECTED)} fields landed and are attributable to this batch")
    return 0


if __name__ == "__main__":
    sys.exit(main())

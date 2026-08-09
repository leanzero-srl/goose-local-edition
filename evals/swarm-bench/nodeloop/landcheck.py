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
EXPECTED = [
    ("task_dispatched", "description_chars", "C12(b) — the split child's instruction length"),
    ("skeleton_drafts", "requested_best_of_n", "C5(A) — the PRE-clamp ask"),
    ("skeleton_drafts", "distinct_draft_models", "C5(A) — what actually caps the pool"),
    ("skeleton_drafts", "clamped", "C5(A) — whether the ask was cut down"),
    ("complete_verify", "inconclusive_reasons", "C6(1) — why the run abstained (STRING PROBE CANNOT SEE THIS)"),
    ("plan_convergence", "would_skip_ladder_prelift", "F707 — the pre-lift shadow"),
]


def logs_under(root: Path) -> list[Path]:
    if not root.is_dir():
        return []
    return sorted(root.glob("*/run.jsonl")) + sorted(root.glob("cells/*/run.jsonl"))


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
    live = scan(logs_under(RUNS))
    old = scan(logs_under(ARCHIVE))
    print(f"live logs   {len(logs_under(RUNS)):3d}   archive logs {len(logs_under(ARCHIVE)):3d}")
    if not live["events"]:
        print("🔴 no live events at all — instrument blind or no run has completed since the rebuild.")
        return 2
    # POSITIVE CONTROL, on the same objects, before any zero below is allowed to mean anything.
    # `inconclusive_reasons` reading 0 on `complete_verify` must mean "not emitted there yet", NOT
    # "this scan cannot see that field name". It ships on `spec_contract` today, so if the scan is
    # honest it MUST find it there. Without this, C6's whole verdict rests on an unproven negative —
    # the exact failure this campaign has hit six times.
    ctl = live["pairs"].get(("spec_contract", "inconclusive_reasons"), 0)
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
    print("✅ every checked field landed and is attributable" if ok
          else "🔴 at least one field did not land, or cannot be attributed to this batch")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

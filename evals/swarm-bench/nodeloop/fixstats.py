"""F785: aggregate fix_attempt_progress across every trace so the early-cut rule is designed
from DISTRIBUTIONS, not anecdotes.

Joins each fix_attempt_progress to its complete_fix_completed (same trace, round + twin/shard)
and answers the design question directly: does a long still-window PREDICT a worthless attempt
(safe to cut), or do healthy attempts also stall (a cut would destroy salvage — F785's twin 0)?

Reads runs/nodeloop/*/run.jsonl (live) + runs/nodeloop/trace-*.jsonl (rotated). Attempts are
keyed loosely — a trace may hold several rounds; twin/shard disambiguates within one.
"""

from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"


def iter_traces():
    yield from RUNS.glob("*/run.jsonl")
    yield from RUNS.glob("trace-*.jsonl")


def attempt_key(e: dict):
    which = e.get("twin", e.get("shard"))
    return (e.get("round"), which)


def collect():
    rows = []
    for tr in iter_traces():
        progress: dict = {}
        for line in tr.read_text(errors="replace").splitlines():
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            ev = e.get("event")
            if ev == "fix_attempt_progress":
                progress[attempt_key(e)] = e
            elif ev == "complete_fix_completed" and attempt_key(e) in progress:
                p = progress.pop(attempt_key(e))
                verified = e.get("verified_findings")
                baseline = e.get("baseline_findings")
                rows.append({
                    "trace": tr.name if tr.name.startswith("trace-") else tr.parent.name,
                    "round": e.get("round"),
                    "who": e.get("twin", e.get("shard")),
                    "secs": e.get("secs"),
                    "first_change_secs": p.get("first_change_secs"),
                    "longest_still_secs": p.get("longest_still_secs"),
                    "samples": p.get("samples"),
                    "changed": p.get("changed_samples"),
                    "verified": verified,
                    "baseline": baseline,
                    "improved": (verified is not None and baseline is not None
                                 and verified < baseline),
                })
    return rows


def main():
    rows = collect()
    if not rows:
        print("no joined fix-attempt data yet")
        return
    rows.sort(key=lambda r: (r["longest_still_secs"] or 0), reverse=True)
    print(f"{len(rows)} joined attempt(s)\n")
    print(f"{'trace':<28}{'rnd':>4}{'who':>5}{'secs':>6}{'first_wr':>9}"
          f"{'max_still':>10}{'verified':>9}{'improved':>9}")
    for r in rows:
        print(f"{r['trace']:<28}{r['round']:>4}{str(r['who']):>5}{r['secs']:>6}"
              f"{str(r['first_change_secs']):>9}{str(r['longest_still_secs']):>10}"
              f"{str(r['verified']):>9}{str(r['improved']):>9}")
    # The design question, answered as a 2x2: long-still (>=300s) vs improved.
    still = [r for r in rows if (r["longest_still_secs"] or 0) >= 300]
    improved_still = sum(1 for r in still if r["improved"])
    print(f"\nattempts with a >=300s still window: {len(still)}; "
          f"of those, IMPROVED the tree: {improved_still}")
    print("every improved long-still attempt is one a naive stall-cut would have destroyed")


if __name__ == "__main__":
    main()

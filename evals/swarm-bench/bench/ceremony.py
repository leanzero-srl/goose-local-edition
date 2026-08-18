#!/usr/bin/env python3
"""Where did the fleet's node-seconds actually GO, and what did each family produce?

F881 follow-up. Run 8 spent 1,873 of 7,850 node-seconds (24%) inside `verify::*` tasks — a fan
that asks a 27B model to run an import statement — while all three findings the gate reported came
from deterministic scans. That is the ceremony class Mihai keeps pointing at ("why are there still
such generic tasks?"), and it deserves a NUMBER per run rather than a suspicion per run.

This is deliberately an OFFLINE reader of run.jsonl. Nothing here changes the engine, so it can be
pointed at any archived run — including every run that has already happened — and it cannot
destabilise the run it is measuring.

I am NOT cutting the verify fan on this evidence. One run showing zero yield is not proof that a
gate never catches anything; a negative that authorises deletion has to be proven across runs, on
the same object. This script is how that proof gets accumulated.

Usage:  python3 ceremony.py <run.jsonl> [<run.jsonl> ...]
"""
from __future__ import annotations

import json
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path


def family(task_id: str) -> str:
    """The task's family — the unit a decision would be made about, not the individual task."""
    for prefix in ("verify-e2e::", "verify::", "fix::", "complete-fix", "test-"):
        if task_id.startswith(prefix):
            return prefix.rstrip(":-") or prefix
    if task_id in ("integrate-verify", "readme"):
        return task_id
    return "module"


def parse_ts(event: dict):
    try:
        return datetime.fromisoformat(event["ts"])
    except Exception:
        return None


def read(path: Path) -> dict:
    dispatched: dict[str, datetime] = {}
    secs: dict[str, float] = defaultdict(float)
    count: dict[str, int] = defaultdict(int)
    failed: dict[str, int] = defaultdict(int)
    findings: list[str] = []

    for line in path.open(errors="replace"):
        try:
            event = json.loads(line)
        except Exception:
            continue
        name = event.get("event")
        if name == "task_dispatched":
            when = parse_ts(event)
            if when:
                dispatched[event.get("task_id", "?")] = when
        elif name == "task_completed":
            task_id = event.get("task_id", "?")
            fam = family(task_id)
            count[fam] += 1
            if event.get("status") not in (None, "ok", "Completed", "completed"):
                failed[fam] += 1
            started, ended = dispatched.get(task_id), parse_ts(event)
            if started and ended:
                secs[fam] += (ended - started).total_seconds()
        elif name == "complete_verify":
            findings = event.get("finding_texts") or findings

    return {"secs": secs, "count": count, "failed": failed, "findings": findings}


def report(path: Path) -> None:
    data = read(path)
    secs, count, failed = data["secs"], data["count"], data["failed"]
    total = sum(secs.values())
    print(f"\n=== {path} ===")
    print(f"{'family':16s} {'tasks':>6s} {'failed':>7s} {'node-sec':>10s} {'share':>7s}")
    for fam in sorted(secs, key=lambda f: -secs[f]):
        share = 100 * secs[fam] / total if total else 0
        print(f"{fam:16s} {count[fam]:6d} {failed[fam]:7d} {secs[fam]:10.0f} {share:6.1f}%")
    print(f"{'TOTAL':16s} {sum(count.values()):6d} {sum(failed.values()):7d} {total:10.0f}")

    # THE QUESTION THIS SCRIPT EXISTS TO ANSWER: did the read-only LLM verification fan pay for
    # itself? Attribution is deliberately crude — a finding is credited to the fan only when it
    # names an import/syntax failure, which is the ONLY thing that spec asks the fan to look for.
    # Anything else the gate found, a deterministic scan found first.
    fan = secs.get("verify", 0.0)
    import_shaped = [
        f for f in data["findings"]
        if any(w in f.lower() for w in ("importerror", "syntaxerror", "modulenotfound", "cannot import"))
    ]
    print(f"\nread-only verify fan: {fan:.0f} node-sec ({100 * fan / total if total else 0:.0f}% of the run)")
    print(f"gate findings of the shape that fan is asked to catch: {len(import_shaped)}")
    for f in import_shaped:
        print(f"  - {f[:160]}")
    if fan > 0 and not import_shaped:
        print("  VERDICT (this run): the fan cost real node-time and produced nothing of its own shape.")


def main() -> int:
    paths = [Path(a) for a in sys.argv[1:]]
    if not paths:
        print(__doc__)
        return 2
    for path in paths:
        if path.exists():
            report(path)
        else:
            print(f"missing: {path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

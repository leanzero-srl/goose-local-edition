"""Re-grade stored episodes after a probe is fixed, without re-running the agent.

A probe bug should not cost a night of fleet time, but silently overwriting recorded results is how
a benchmark stops being auditable. So a regrade keeps the superseded verdict inline, stamps why, and
never touches the workspace the score came from.

The honest constraint: this can only fix GRADING. If the run itself was wrong — wrong prompt, wrong
model, a contended fleet — the episode has to be re-run, and this will not pretend otherwise.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path
from typing import Dict

BOARD = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BOARD / "probes"))
sys.path.insert(0, str(BOARD / "runner"))

import build  # noqa: E402
import repair  # noqa: E402
import testwrite  # noqa: E402

PROBES = {"repair": repair, "testwrite": testwrite, "build": build}


def regrade_one(record: Dict, episode_dir: Path, reason: str) -> Dict:
    vertical = record["vertical"]
    fixture = BOARD / "verticals" / vertical / "fixtures" / record["fixture"]
    graded_root = Path(record.get("graded_root") or (episode_dir / "workspace"))
    if not graded_root.is_dir():
        graded_root = episode_dir / "workspace"
    probe = PROBES[vertical].grade(fixture, graded_root)

    superseded = {"probe": record["probe"], "score": record["score"],
                  "artifact_score": record.get("artifact_score")}
    record.setdefault("supersedes", []).append(
        {"at": dt.datetime.now(dt.timezone.utc).isoformat(), "reason": reason, **superseded})
    record["probe"] = probe
    record["artifact_score"] = probe["score"]
    # The delivery rule is unchanged by a regrade: a run that never finished still scores zero.
    record["score"] = 0.0 if record.get("scored_zero_for") else probe["score"]
    record["regraded"] = True
    return record


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=Path, default=BOARD / "runs")
    ap.add_argument("--vertical", help="only this vertical")
    ap.add_argument("--reason", required=True, help="why these results are being replaced")
    ap.add_argument("--apply", action="store_true", help="write the changes (default is dry run)")
    args = ap.parse_args()

    changed = 0
    for path in sorted(args.runs.glob("*/episode.json")):
        record = json.loads(path.read_text())
        if not record.get("complete"):
            continue
        if args.vertical and record["vertical"] != args.vertical:
            continue
        before = record["score"], record["probe"].get("passed"), record["probe"].get("total")
        updated = regrade_one(dict(record), path.parent, args.reason)
        after = updated["score"], updated["probe"].get("passed"), updated["probe"].get("total")
        if before != after:
            changed += 1
            print(f"{path.parent.name}: {before} -> {after}")
            if args.apply:
                path.write_text(json.dumps(updated, indent=2))
    verb = "rewrote" if args.apply else "would change"
    print(f"{verb} {changed} episode(s)" + ("" if args.apply else "  (pass --apply to write)"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

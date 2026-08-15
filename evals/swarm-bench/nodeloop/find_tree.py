"""Pick the _sb4trees archive for a completed unit — mechanically, not by eyeball.

F819 broke the old rule: two archives can share a score for the same cell name (one pre-freeze,
one current), so score+cell alone no longer disambiguates. The rule is now score+cell+mtime:
among score matches for the cell's node-shape, take the one whose verdict mtime is closest to
the unit's end, and REFUSE if the closest is further than the window (a wrong tree silently
feeding bridge_calc is worse than no answer).

Usage: python3 find_tree.py <score> <nodes> [<unit-end-epoch>]  (end defaults to now)
Prints the archive dir path on success; exits 2 with a reason on refusal.
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

WINDOW_SECS = 6 * 3600

def main() -> int:
    if len(sys.argv) < 3:
        sys.stderr.write(__doc__)
        return 2
    score = float(sys.argv[1])
    nodes = int(sys.argv[2])
    end = float(sys.argv[3]) if len(sys.argv) > 3 else time.time()
    root = Path(__file__).resolve().parent.parent / "runs" / "nodeloop" / "_sb4trees"
    matches = []
    for d in root.glob(f"swarm-{nodes}node-*"):
        v = d / "sb4-verdict.json"
        if not v.is_file():
            continue
        try:
            s = json.load(open(v)).get("score")
        except Exception:
            continue
        if s is not None and abs(s - score) < 0.002:
            matches.append((abs(v.stat().st_mtime - end), d))
    if not matches:
        sys.stderr.write(f"REFUSING: no {nodes}-node archive scores {score}\n")
        return 2
    matches.sort()
    dist, best = matches[0]
    if dist > WINDOW_SECS:
        sys.stderr.write(
            f"REFUSING: closest score-match {best.name} is {dist/3600:.1f}h from the unit end "
            f"(window {WINDOW_SECS//3600}h) — likely a different era's tree; "
            f"pass the unit-end epoch explicitly\n")
        return 2
    if len(matches) > 1 and matches[1][0] < WINDOW_SECS:
        sys.stderr.write(f"note: {len(matches)-1} other in-window match(es): "
                         + ", ".join(m[1].name for m in matches[1:3]) + "\n")
    print(best)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

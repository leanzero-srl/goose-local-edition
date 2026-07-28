"""Characterise the instrument before trusting it to measure anything.

The gate before scoring exists: how far does a score move on runs that should be identical, and how
many reps does a vertical need before a difference means anything? Until that number exists, a
composite is a number wearing an error bar it never earned.

Two figures come out.

  REPLICATE SPREAD — the same entrant on the same fixture, k times. Binary outcomes, so the spread
  IS the uncertainty; the Wilson width at that n is the honest bar.

  MINIMUM DETECTABLE EFFECT — the smallest true difference two entrants could have that this many
  reps would reliably reveal. Standard two-proportion power at alpha 0.05, power 0.80:

      delta = (z_alpha/2 + z_beta) * sqrt(2 * p * (1 - p) / n)

  Reported as a sentence, in points, because "n=5" means nothing to a reader and "anything closer
  than 55 points is noise at n=5" means everything.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from pathlib import Path
from typing import Dict, List

BOARD = Path(__file__).resolve().parents[1]
Z_ALPHA, Z_BETA = 1.96, 0.8416


def wilson_half_width(passes: int, n: int) -> float:
    if n == 0:
        return 1.0
    p = passes / n
    denom = 1 + Z_ALPHA * Z_ALPHA / n
    return Z_ALPHA / denom * math.sqrt(p * (1 - p) / n + Z_ALPHA * Z_ALPHA / (4 * n * n))


def mde(p: float, n: int) -> float:
    """Smallest difference in pass rate this n can reliably detect, in percentage points."""
    if n == 0:
        return 100.0
    # A rate pinned at 0 or 1 has no observed variance; use the worst case rather than claim
    # infinite resolution from a sample that has simply not seen the other outcome yet.
    p = min(max(p, 0.0), 1.0)
    variance = p * (1 - p)
    if variance == 0:
        variance = 0.25
    return min(100.0, 100.0 * (Z_ALPHA + Z_BETA) * math.sqrt(2 * variance / n))


def reps_needed(p: float, target_points: float) -> int:
    variance = p * (1 - p) or 0.25
    n = 2 * variance * ((Z_ALPHA + Z_BETA) / (target_points / 100.0)) ** 2
    return max(1, math.ceil(n))


def load(runs: Path) -> List[Dict]:
    out = []
    for path in sorted(runs.glob("*/episode.json")):
        try:
            record = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if record.get("complete"):
            out.append(record)
    return out


def report(episodes: List[Dict], target_points: float) -> str:
    groups: Dict[tuple, List[Dict]] = defaultdict(list)
    for ep in episodes:
        groups[(ep["fixture"], ep["label"])].append(ep)

    lines = ["DRIFT CALIBRATION", ""]
    lines.append(f"{'fixture':<18}{'entrant':<20}{'n':>3}{'pass':>6}{'rate':>8}{'±wilson':>9}"
                 f"{'MDE':>8}   outcomes")
    for (fixture, label), eps in sorted(groups.items()):
        eps.sort(key=lambda e: e["rep"])
        n = len(eps)
        passes = sum(1 for e in eps if e["score"] == 1.0)
        p = passes / n
        seq = "".join("1" if e["score"] == 1.0 else "0" for e in eps)
        lines.append(f"{fixture:<18}{label:<20}{n:>3}{passes:>6}{100 * p:>7.1f}%"
                     f"{100 * wilson_half_width(passes, n):>8.1f}{mde(p, n):>8.1f}   {seq}")

    lines += ["", "WHAT THIS INSTRUMENT CAN RESOLVE"]
    for (fixture, label), eps in sorted(groups.items()):
        n = len(eps)
        p = sum(1 for e in eps if e["score"] == 1.0) / n
        lines.append(f"  {fixture} / {label}: at n={n} this cannot separate entrants closer than "
                     f"{mde(p, n):.0f} points.")
        lines.append(f"      reaching ±{target_points:.0f} points needs about "
                     f"{reps_needed(p, target_points)} reps per prompt.")

    flat = [e for e in episodes]
    if flat:
        walls = sorted(e["wall_secs"] for e in flat)
        lines += ["", f"wall-clock per episode: median {walls[len(walls) // 2]:.0f}s, "
                      f"min {walls[0]:.0f}s, max {walls[-1]:.0f}s (no trimming)"]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=Path, default=BOARD / "runs")
    ap.add_argument("--target-points", type=float, default=15.0,
                    help="the band width you want a card to hold")
    args = ap.parse_args()
    print(report(load(args.runs), args.target_points))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

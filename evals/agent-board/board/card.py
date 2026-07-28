"""Render one card from the episodes on disk.

This REPORTS, it does not yet score. The scoring model is deliberately not designed until drift
calibration says how many reps a vertical needs — a normalised composite built on an
uncharacterised instrument is a number with no error bar wearing one.

What it will not do: rank two entrants whose intervals overlap. They print as TIED, because at the
sample sizes a person will actually sit through, most differences are not differences.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Tuple

BOARD = Path(__file__).resolve().parents[1]
Z = 1.96
BAR_WIDTH = 20

TITLES = {
    "repair": ("REPAIR", "Can the agent fix a real defect without breaking anything?"),
}


def wilson(passes: int, n: int) -> Tuple[float, float, float]:
    """Point estimate and 95% Wilson interval. Wilson, not normal: at n=5 the normal approximation
    puts the bound outside [0,1] and would print an interval that cannot occur."""
    if n == 0:
        return 0.0, 0.0, 1.0
    p = passes / n
    denom = 1 + Z * Z / n
    centre = (p + Z * Z / (2 * n)) / denom
    half = Z / denom * math.sqrt(p * (1 - p) / n + Z * Z / (4 * n * n))
    # Clamped to contain p: at n=100, 100/100 lands the upper bound on 0.9999999999999999 and a
    # band that excludes its own point estimate is not a band.
    return p, min(max(0.0, centre - half), p), max(min(1.0, centre + half), p)


def load_episodes(runs: Path) -> List[Dict]:
    out = []
    for path in sorted(runs.glob("*/episode.json")):
        try:
            record = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if record.get("complete"):
            out.append(record)
    return out


def summarise(episodes: List[Dict]) -> List[Dict]:
    groups: Dict[str, List[Dict]] = defaultdict(list)
    for ep in episodes:
        groups[ep["label"]].append(ep)

    rows = []
    for label, eps in groups.items():
        n = len(eps)
        passes = sum(1 for e in eps if e["score"] == 1.0)
        p, lo, hi = wilson(passes, n)
        walls = sorted(e["wall_secs"] for e in eps)
        rows.append({
            "label": label, "n": n, "passes": passes,
            "pct": 100 * p, "lo": 100 * lo, "hi": 100 * hi,
            "median_secs": walls[len(walls) // 2],
            "tampered": sum(1 for e in eps if e["probe"]["tampered"]),
            "crashed": sum(1 for e in eps if e.get("crashed") or e.get("timed_out")),
            "baseline": e_is_baseline(eps[0]),
        })
    rows.sort(key=lambda r: -r["pct"])

    # Overlapping intervals are not an ordering. Equal rank numbers say so on the page.
    rank = 0
    for i, row in enumerate(rows):
        if i == 0 or row["hi"] < rows[i - 1]["lo"]:
            rank = i + 1
        row["rank"] = rank
    return rows


def e_is_baseline(ep: Dict) -> bool:
    return (ep.get("provider") or "").startswith("aws_") or (ep.get("provider") == "anthropic")


def render(vertical: str, rows: List[Dict]) -> str:
    title, question = TITLES.get(vertical, (vertical.upper(), ""))
    total_n = min((r["n"] for r in rows), default=0)
    lines = [f"{title}", question, ""]
    if not rows:
        return "\n".join(lines + ["  no episodes yet"])

    for row in rows:
        filled = int(round(BAR_WIDTH * row["pct"] / 100))
        bar = "█" * filled + "░" * (BAR_WIDTH - filled)
        half = (row["hi"] - row["lo"]) / 2
        tag = "baseline" if row["baseline"] else "← your fleet"
        note = ""
        if row["tampered"]:
            note += f"  TAMPERED {row['tampered']}"
        if row["crashed"]:
            note += f"  crashed/timeout {row['crashed']}"
        lines.append(
            f" {row['rank']}  {row['label']:<20s} {row['passes']}/{row['n']}  "
            f"{row['pct']:5.1f} ±{half:4.1f}  {bar}  {row['median_secs']:6.1f}s  {tag}{note}")

    tied = [r for r in rows if sum(1 for x in rows if x["rank"] == r["rank"]) > 1]
    lines += ["", f"n={total_n} per entrant · reporting only, no composite — drift not yet calibrated"]
    if tied:
        names = ", ".join(sorted({r["label"] for r in tied}))
        lines.append(f"TIED (intervals overlap, refusing to order): {names}")
    lines.append("cost/episode: not measured — goose does not surface token counts to the harness")
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=Path, default=BOARD / "runs")
    ap.add_argument("--vertical", default="repair")
    ap.add_argument("--json", type=Path, help="also write the card as JSON for the website")
    args = ap.parse_args()

    episodes = [e for e in load_episodes(args.runs) if e["vertical"] == args.vertical]
    rows = summarise(episodes)
    print(render(args.vertical, rows))
    if args.json:
        args.json.write_text(json.dumps(
            {"vertical": args.vertical, "rows": rows, "episodes": len(episodes)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

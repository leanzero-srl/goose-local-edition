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
from typing import Dict, List, Optional, Tuple

BOARD = Path(__file__).resolve().parents[1]
Z = 1.96
BAR_WIDTH = 20

TITLES = {
    "repair": ("REPAIR", "Can the agent fix a real defect without breaking anything?"),
    "testwrite": ("TEST WRITING",
                  "Would these tests actually catch a bug? (mutation score: mutants killed / K)"),
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


def time_noise_threshold(episodes: List[Dict]) -> float:
    """Two worst-case replicate CVs, in percent. Below this, a time gap is jitter."""
    by_label: Dict[str, List[float]] = defaultdict(list)
    for ep in episodes:
        by_label[ep["label"]].append(ep["wall_secs"])
    worst = 0.0
    for secs in by_label.values():
        if len(secs) < 2:
            continue
        mean = sum(secs) / len(secs)
        if not mean:
            continue
        sd = math.sqrt(sum((s - mean) ** 2 for s in secs) / len(secs))
        worst = max(worst, 100 * sd / mean)
    return 2 * worst


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
        # A continuous vertical reports killed/K per episode. Counting only score==1.0 would throw
        # the resolution away and turn mutation score back into the binary that already saturated.
        # Every mutant is its own Bernoulli trial, so the pooled kills carry the interval.
        mutant_totals = [(e["probe"].get("killed", 0), e["probe"].get("mutants", 0))
                         for e in eps if e["probe"].get("mutants")]
        if mutant_totals:
            passes = sum(k for k, _ in mutant_totals)
            trials = sum(m for _, m in mutant_totals)
            p, lo, hi = wilson(passes, trials)
            denom = trials
        else:
            passes = sum(1 for e in eps if e["score"] == 1.0)
            denom = n
            p, lo, hi = wilson(passes, n)
        walls = sorted(e["wall_secs"] for e in eps)
        rows.append({
            "label": label, "n": n, "passes": passes, "denom": denom,
            "pct": 100 * p, "lo": 100 * lo, "hi": 100 * hi,
            "median_secs": walls[len(walls) // 2],
            "tampered": sum(1 for e in eps if e["probe"]["tampered"]),
            "crashed": sum(1 for e in eps if e.get("crashed") or e.get("timed_out")),
            "baseline": e_is_baseline(eps[0]),
            "claims": sum(1 for e in eps if (e.get("claim") or {}).get("available")),
            "false_greens": sum(1 for e in eps if e.get("false_green")),
        })
    # Correctness first, then time. Measured on this corpus: every cloud baseline clears every
    # repair rung, and the local 27b clears them too — 876.8s against haiku's 31.3s. When pass
    # rates saturate, time is the only thing left that separates entrants, so it is a ranked column
    # and not a footnote. The two are never merged into one score: a fast wrong answer and a slow
    # right one are different failures and must stay legible as such.
    rows.sort(key=lambda r: (-r["pct"], r["median_secs"]))

    # Overlapping intervals are not an ordering. Equal rank numbers say so on the page.
    rank = 0
    for i, row in enumerate(rows):
        if i == 0 or row["hi"] < rows[i - 1]["lo"]:
            rank = i + 1
        row["rank"] = rank
    return rows


def e_is_baseline(ep: Dict) -> bool:
    return (ep.get("provider") or "").startswith("aws_") or (ep.get("provider") == "anthropic")


def render(vertical: str, rows: List[Dict], episodes: Optional[List[Dict]] = None) -> str:
    episodes = episodes or []
    title, question = TITLES.get(vertical, (vertical.upper(), ""))
    ns = [r["n"] for r in rows] or [0]
    total_n = f"{min(ns)}" if min(ns) == max(ns) else f"{min(ns)}-{max(ns)}"
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
            f" {row['rank']}  {row['label']:<20s} {row['passes']}/{row['denom']}  "
            f"{row['pct']:5.1f} ±{half:4.1f}  {bar}  {row['median_secs']:6.1f}s  {tag}{note}")

    tied = [r for r in rows if sum(1 for x in rows if x["rank"] == r["rank"]) > 1]
    lines += ["", f"n={total_n} per entrant · reporting only, no composite — drift not yet calibrated"]
    if tied:
        names = ", ".join(r["label"] for r in tied)
        lines.append(f"TIED on correctness (intervals overlap, refusing to order): {names}")
        fastest, slowest = min(tied, key=lambda r: r["median_secs"]), max(tied, key=lambda r: r["median_secs"])
        if fastest["median_secs"] > 0 and slowest is not fastest:
            ratio = slowest["median_secs"] / fastest["median_secs"]
            lines.append(f"  ...but {fastest['label']} is {ratio:.0f}x quicker than "
                         f"{slowest['label']} ({fastest['median_secs']:.0f}s vs "
                         f"{slowest['median_secs']:.0f}s). Same answer, different price.")
        # Time is not exempt from the noise floor just because correctness saturated. Measured
        # replicate CV is 23-29%, so entrants inside ~2 CV are the SAME speed and ordering them by
        # median would invent a ranking out of jitter — the exact failure the TIED rule exists for.
        threshold = time_noise_threshold(episodes)
        if threshold:
            same_speed = [r["label"] for r in rows
                          if r["median_secs"] <= fastest["median_secs"] * (1 + threshold / 100)]
            if len(same_speed) > 1:
                lines.append(f"  Time differences under {threshold:.0f}% are replicate noise "
                             f"(worst CV {threshold / 2:.0f}%), so these are the SAME speed and the "
                             f"order between them is arbitrary: {', '.join(same_speed)}.")
    lines.append("cost/episode: not measured — goose does not surface token counts to the harness")

    claimed = sum(r["claims"] for r in rows)
    total = sum(r["n"] for r in rows)
    lines += ["", "HONESTY — did the run tell the truth about finishing?"]
    if claimed == 0:
        lines.append(f"  NOT COMPUTABLE on any of these {total} episodes. Only the swarm emits a "
                     f"structured claim (complete_result); reading success out of a single agent's "
                     f"closing prose is the self-report this board refuses to treat as evidence.")
    else:
        false_greens = sum(r["false_greens"] for r in rows)
        lines.append(f"  {claimed}/{total} episodes made a structured claim "
                     f"({100 * claimed / total:.0f}% coverage) · "
                     f"false-green {false_greens}/{claimed}")
        for row in rows:
            if row["false_greens"]:
                lines.append(f"    {row['label']}: claimed success on {row['false_greens']} "
                             f"episode(s) the probe found broken")
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=Path, default=BOARD / "runs")
    ap.add_argument("--vertical", default="repair")
    ap.add_argument("--json", type=Path, help="also write the card as JSON for the website")
    args = ap.parse_args()

    episodes = [e for e in load_episodes(args.runs) if e["vertical"] == args.vertical]
    rows = summarise(episodes)
    print(render(args.vertical, rows, episodes))
    if args.json:
        args.json.write_text(json.dumps(
            {"vertical": args.vertical, "rows": rows, "episodes": len(episodes)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

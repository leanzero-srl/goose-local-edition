"""Emit the whole board as JSON for the website, with a profile hash that can invalidate it.

The site recomputes from evidence rather than trusting a submitted score, so what leaves here is
cards plus the manifest a sceptic needs to re-derive them: which fixtures, which mutants, which
build, and a hash over the bytes that define the task.

The profile hash is the load-bearing part. It covers every prompt, probe, mutant and seed file, so
editing a fixture to flatter a result changes the hash and detaches the run from every baseline
recorded under the old one. A benchmark whose tasks can drift silently is not a benchmark.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Dict, List

import card

BOARD = Path(__file__).resolve().parents[1]
HASHED_TREES = ("verticals", "probes")
SKIP_SUFFIXES = (".pyc",)
SKIP_DIRS = {"__pycache__", ".pytest_cache"}


def profile_hash() -> Dict[str, object]:
    """SHA-256 over every byte that defines the tasks and the grading, in sorted path order."""
    digest = hashlib.sha256()
    counted = 0
    for tree in HASHED_TREES:
        root = BOARD / tree
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file() or path.suffix in SKIP_SUFFIXES:
                continue
            if SKIP_DIRS & set(path.parts):
                continue
            digest.update(str(path.relative_to(BOARD)).encode())
            digest.update(path.read_bytes())
            counted += 1
    return {"sha256": digest.hexdigest(), "files": counted, "trees": list(HASHED_TREES)}


def build_sha() -> str:
    try:
        return subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=BOARD,
                              capture_output=True, text=True, timeout=15).stdout.strip() or "unknown"
    except (subprocess.SubprocessError, OSError):
        return "unknown"


def export(runs: Path) -> Dict:
    episodes = card.load_episodes(runs)
    verticals = sorted({e["vertical"] for e in episodes})
    cards: List[Dict] = []
    for vertical in verticals:
        subset = [e for e in episodes if e["vertical"] == vertical]
        rows = card.summarise(subset)
        title, question = card.TITLES.get(vertical, (vertical.upper(), ""))
        tied = {r["rank"] for r in rows if sum(1 for x in rows if x["rank"] == r["rank"]) > 1}
        cards.append({
            "vertical": vertical,
            "title": title,
            "question": question,
            "episodes": len(subset),
            "fixtures": sorted({e["fixture"] for e in subset}),
            "rows": rows,
            "tied_ranks": sorted(tied),
            "time_noise_threshold_pct": round(card.time_noise_threshold(subset), 1),
            # Stated on the card, never inferred by the reader.
            "not_measured": ["cost per episode — goose does not surface token counts to the harness"],
        })

    tampered = sum(1 for e in episodes if e["probe"].get("tampered"))
    zeroed = [e["episode_id"] for e in episodes if e.get("scored_zero_for")]
    return {
        "board_version": json.loads((BOARD / "board.json").read_text())["board_version"],
        "build_sha": build_sha(),
        "profile": profile_hash(),
        "cards": cards,
        "integrity": {
            "episodes": len(episodes),
            "tampered": tampered,
            "scored_zero_for_not_finishing": zeroed,
            "crashes_and_timeouts_stay_in_denominator": True,
        },
        "refusals": [
            "No composite is emitted — the scoring model stays undesigned until drift calibration "
            "says what the instrument can support.",
            "Entrants whose intervals overlap are TIED and are never ordered.",
            "A saturated fixture is reported as saturated rather than as needing more reps.",
            "Self-reported success is never evidence; it is only the claim side of the honesty card.",
        ],
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=Path, default=BOARD / "runs")
    ap.add_argument("--out", type=Path, default=BOARD / "runs/board-export.json")
    args = ap.parse_args()

    payload = export(args.runs)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(payload, indent=2))
    print(f"wrote {args.out}")
    print(f"  board {payload['board_version']} @ build {payload['build_sha']}")
    print(f"  profile {payload['profile']['sha256'][:16]}… over {payload['profile']['files']} files")
    for c in payload["cards"]:
        print(f"  {c['title']:<14} {len(c['rows'])} entrants, {c['episodes']} episodes, "
              f"fixtures {len(c['fixtures'])}")
    print(f"  integrity: {payload['integrity']['tampered']} tampered, "
          f"{len(payload['integrity']['scored_zero_for_not_finishing'])} zeroed for not finishing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

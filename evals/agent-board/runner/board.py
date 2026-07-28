"""Schedule ticks. The tick budget is the dial — it decides how long a board run takes.

Ticks are emitted in ROUNDS: one round holds exactly one tick per (fixture x entrant), shuffled
within the round. Two properties follow, and both matter more than they look.

  * Aborting after round k leaves exactly k reps of EVERYTHING. A half-finished board is still a
    balanced board, so a partial result is reportable instead of discarded.
  * Within a round the order is shuffled from a fixed seed, so no entrant systematically occupies
    the cold start or the thermally-throttled tail of a long run — while the schedule stays exactly
    reproducible.

Resumability is per episode, not per board: a finished tick is skipped, so an interrupted 24-hour
run resumes where it stopped rather than starting over.
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path
from typing import Dict, List

sys.path.insert(0, str(Path(__file__).resolve().parent))
from episode import run_episode  # noqa: E402

BOARD = Path(__file__).resolve().parents[1]


def load_board(path: Path) -> Dict:
    return json.loads(path.read_text())


def plan_ticks(cfg: Dict, only_vertical: str | None, only_entrant: str | None) -> List[Dict]:
    pairs = []
    for vname, vertical in cfg["verticals"].items():
        if only_vertical and vname != only_vertical:
            continue
        for fixture in vertical["fixtures"]:
            for entrant in cfg["entrants"]:
                if only_entrant and entrant["name"] != only_entrant:
                    continue
                pairs.append({"vertical": vname, "fixture": fixture, **entrant})

    rng = random.Random(cfg.get("shuffle_seed", 0))
    ticks: List[Dict] = []
    for rep in range(cfg["reps"]):
        round_ticks = [dict(p, rep=rep) for p in pairs]
        rng.shuffle(round_ticks)
        ticks.extend(round_ticks)
    return ticks


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--config", type=Path, default=BOARD / "board.json")
    ap.add_argument("--ticks", type=int, help="stop after this many ticks (the time dial)")
    ap.add_argument("--vertical")
    ap.add_argument("--entrant", help="run only this entrant, e.g. local-single")
    ap.add_argument("--out", type=Path, default=BOARD / "runs")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--allow-busy", action="store_true")
    args = ap.parse_args()

    cfg = load_board(args.config)
    ticks = plan_ticks(cfg, args.vertical, args.entrant)
    if args.ticks:
        ticks = ticks[: args.ticks]

    rounds = cfg["reps"] if not args.ticks else "partial"
    print(f"board {cfg['board_version']}: {len(ticks)} ticks "
          f"({len(ticks) // max(cfg['reps'], 1)} per round, {rounds} rounds planned)")
    if args.dry_run:
        for i, t in enumerate(ticks, 1):
            print(f"  {i:3d}  {t['vertical']}/{t['fixture']}  {t['name']}  rep{t['rep']}")
        return 0

    results = []
    for i, tick in enumerate(ticks, 1):
        fixture = BOARD / "verticals" / tick["vertical"] / "fixtures" / tick["fixture"]
        print(f"--- tick {i}/{len(ticks)}: {tick['name']} rep{tick['rep']} ---", flush=True)
        try:
            results.append(run_episode(
                fixture, tick["target"], tick["rep"], args.out,
                provider=tick.get("provider"), model=tick.get("model"), label=tick["name"],
                env_file=tick.get("env_file"), allow_busy=args.allow_busy))
        except Exception as exc:  # a bad tick must never kill the board
            print(f"[tick FAILED] {tick['name']} rep{tick['rep']}: {exc}", flush=True)

    scored = [r for r in results if r.get("complete")]
    print(f"\n{len(scored)}/{len(ticks)} ticks complete")
    for name in sorted({r["label"] for r in scored}):
        rows = [r for r in scored if r["label"] == name]
        wins = sum(1 for r in rows if r["score"] == 1.0)
        tampered = sum(1 for r in rows if r["probe"]["tampered"])
        print(f"  {name:22s} {wins}/{len(rows)} passed"
              + (f"   TAMPERED {tampered}" if tampered else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

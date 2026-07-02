"""swarm-gym entrypoints: `once` (one vibing session), `loop` (N sessions), `report` (trend)."""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Optional

import yaml

from . import bench, orchestrator
from .ledger import Ledger

ARCHES = ["heavy-spec", "minimal-spec", "continue-existing"]


def _root() -> Path:
    return Path(__file__).resolve().parent.parent


def _cfg(root: Path) -> dict:
    return yaml.safe_load((root / "config.yaml").read_text())


def main(argv: Optional[list] = None) -> None:
    root = _root()
    cfg = _cfg(root)
    ap = argparse.ArgumentParser("swarm-gym")
    sub = ap.add_subparsers(dest="cmd", required=True)

    o = sub.add_parser("once", help="run one vibing session")
    o.add_argument("--archetype", default="heavy-spec", choices=ARCHES)
    o.add_argument("--persona", default=None)
    o.add_argument("--seed", type=int, default=1)
    o.add_argument("--turns", type=int, default=None)
    o.add_argument("--no-judge", action="store_true")
    o.add_argument("--tweak", action="store_true")

    lp = sub.add_parser("loop", help="run N sessions cycling the archetypes")
    lp.add_argument("--n", type=int, default=3)
    lp.add_argument("--turns", type=int, default=None)
    lp.add_argument("--no-judge", action="store_true")
    lp.add_argument("--tweak", action="store_true")

    sub.add_parser("report", help="print the session ledger summary")

    bp = sub.add_parser("bench", help="run a benchmark tier tagged with a model variant (mlx/gguf)")
    bp.add_argument("--tier", required=True, choices=list(bench.TIERS))
    bp.add_argument("--variant", required=True, help="model variant label, e.g. mlx or gguf")
    bp.add_argument("--turns", type=int, default=None)
    bp.add_argument("--no-judge", action="store_true")
    bp.add_argument("--tweak", action="store_true")

    br = sub.add_parser("bench-report", help="print/write the head-to-head benchmark report")
    br.add_argument("--variant", default=None, help="filter to one variant (optional)")

    sub.add_parser("bench-csv", help="export benchmark-runs.csv + benchmark-summary.csv from the ledger")

    args = ap.parse_args(argv)

    if args.cmd == "bench-report":
        print(bench.bench_report(_root(), cfg, args.variant))
        return

    if args.cmd == "bench-csv":
        print(bench.bench_csv(_root(), cfg))
        return

    if args.cmd == "report":
        led = Ledger((root / cfg["paths"]["ledger"]).resolve())
        sessions = led.all_sessions()
        print(f"{len(sessions)} sessions")
        for s in sessions[-30:]:
            print(f"  {s.get('ts','')[:19]}  {s.get('archetype',''):17} {s.get('app_slug',''):24} "
                  f"turns={s.get('turns',0)}  -> {s.get('overall')}")
        return

    turns = args.turns or cfg["session"]["default_turns"]
    if args.cmd == "once":
        s = orchestrator.run_session(
            cfg, root, args.archetype, args.persona, args.seed, turns, not args.no_judge, args.tweak
        )
        print(f"\nsession {s['session_id']} -> {s['overall']} ({s['turns']} turns)")
        print(f"report: {s['run_dir']}/report.html")
    elif args.cmd == "loop":
        for i in range(args.n):
            a = ARCHES[i % len(ARCHES)]
            try:
                s = orchestrator.run_session(
                    cfg, root, a, None, 1000 + i, turns, not args.no_judge, args.tweak
                )
                print(f"[{i + 1}/{args.n}] {s['session_id']} -> {s['overall']} ({s['turns']} turns)")
            except Exception as e:
                print(f"[{i + 1}/{args.n}] {a} session FAILED: {e}")
    elif args.cmd == "bench":
        bench.run_suite(cfg, root, args.tier, args.variant)
        print("\n=== benchmark report ===")
        print(bench.bench_report(root, cfg))


if __name__ == "__main__":
    main()

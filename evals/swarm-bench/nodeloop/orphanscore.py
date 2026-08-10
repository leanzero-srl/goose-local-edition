#!/usr/bin/env python3
"""Score a tree the sweep never got to score — INFORMATIONALLY, never as a corpus row.

WHY THIS EXISTS. On 2026-08-10 the sweep supervisor died at 18:35 leaving its engine child running.
The engine finished its build; the sweep, which scores in-process, was not there to score it. So a
completed ~80-minute unit sits on disk with no `nodeloop-result.json`.

⚠️ IT DELIBERATELY DOES NOT WRITE A CORPUS ROW. The sweep's row carries `audit`, `harness_ok`,
`contended`, `actual_pool`, `void`/`void_reason` and the abandon-watchdog fields, and `summarise()`
and `is_done()` filter on several of them. A hand-assembled row missing those would be a
second-class row in a corpus whose whole discipline is that every number came off the same
instrument — the exact "comparing across instrument versions" trap that once published a cheaper
model beating a stronger one. The sweep will re-run this unit on restart and produce the real row;
this only recovers the NUMBER so ~80 minutes of fleet time still tells us something.

THE CONTROL RUNS FIRST AND IS A REPRODUCTION, not a smoke test: re-score a cell that already has a
stored score and require the recomputation to land near it. If the scoring path cannot reproduce a
known row, the number it produces for the orphan means nothing either. Tolerance is generous
(±0.05) because the scorer RUNS the app — ports, timing and the vendor's rate limiter are live.
"""
from __future__ import annotations

import json
import os
import socket
import sys
from pathlib import Path

assert os.path.basename(os.getcwd()) == "nodeloop", "run this from the nodeloop dir"

BENCH = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench")
RUNS = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop")
SCRATCH = Path("/private/tmp/claude-501/-Users-mihaiperdum-Projects-goose/"
               "124573f3-de2d-4c0d-a30d-b877e482d4b1/scratchpad")
sys.path.insert(0, str(BENCH))

import score_build  # noqa: E402
import vendor_service  # noqa: E402

CONTROL_CELL = "baseline-n1-r0"
TOLERANCE = 0.05


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def score_tree(tree: Path, tag: str) -> dict:
    """Exactly the path run_build.py uses: a live vendor, gather(), evaluate()."""
    port = free_port()
    trace = SCRATCH / f"orphan-trace-{tag}.jsonl"
    db = SCRATCH / f"orphan-{tag}.db"
    for p in (trace, db):
        if p.exists():
            p.unlink()
    srv = vendor_service.serve(port, trace)
    try:
        ctx = score_build.gather(tree, port, db, trace)
        return score_build.evaluate(ctx)
    finally:
        srv.shutdown()


def main() -> int:
    if len(sys.argv) < 2:
        sys.exit("usage: orphanscore.py <cell-or-entrant-dir-name>")
    target = sys.argv[1]

    stored_path = RUNS / CONTROL_CELL / "nodeloop-result.json"
    if not stored_path.exists():
        sys.exit(f"🔴 CONTROL UNRUNNABLE: {stored_path} missing")
    stored = json.loads(stored_path.read_text())["score"]
    print(f"control: re-scoring {CONTROL_CELL} (stored {stored:.4f}) …")
    got = score_tree(RUNS / CONTROL_CELL, "control")["score"]
    if abs(got - stored) > TOLERANCE:
        sys.exit(f"🔴 CONTROL FAILED: recomputed {got:.4f} vs stored {stored:.4f} "
                 f"(> {TOLERANCE}). The scoring path does not reproduce a known row, so the "
                 "orphan's number would not be comparable to anything. Do NOT report it.")
    print(f"control: recomputed {got:.4f} vs stored {stored:.4f} — reproduces ✅\n")

    tree = RUNS / target
    if not (tree / "vendorsync").is_dir():
        sys.exit(f"🔴 {tree} has no vendorsync/ — nothing was built there")
    r = score_tree(tree, "target")
    print(score_build.format_report(r, title=f"{target} (ORPHAN, informational)"))
    print(f"\nscore {r['score']:.4f}   tiers "
          f"{ {k: round(v['mean'], 4) for k, v in r['tiers'].items()} }")
    print("\n⚠️ INFORMATIONAL ONLY — no nodeloop-result.json written. This number has no `audit`, "
          "no `harness_ok` and no pool verification, so it must never be pooled with sweep rows or "
          "quoted as a pair. The sweep re-runs this unit on restart for the real row.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

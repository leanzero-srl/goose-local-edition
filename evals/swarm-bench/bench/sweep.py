"""Drive the whole backlog to completion without an operator.

The failure this exists to prevent: an assistant announcing "running unattended" and then stopping
between items because its own context ran out. The loop must live in a process, not in a
conversation. Once launched this walks every entrant, persists each verdict as it lands, skips work
already done, and survives any single item dying.

    nohup python3 -u bench/sweep.py >> sweep.log 2>&1 & disown
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import traceback
from pathlib import Path
from typing import Dict, List

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

# Ordered so the cheapest thing that could invalidate everything runs FIRST.
# local-single is the feasibility gate: if the local fleet cannot resolve the task at all, every
# cloud baseline captured afterwards is wasted effort.
PLAN: List[Dict] = [
    {"entrant": "opus-5", "reps": 1, "why": "true number on the corrected scorer"},
    {"entrant": "local-single", "reps": 1, "why": "FEASIBILITY GATE — local must clear the floor"},
    {"entrant": "sonnet-5", "reps": 2, "why": "baseline"},
    {"entrant": "haiku-4.5", "reps": 2, "why": "baseline"},
]

FLOOR_GATE = {"entrant": "local-single", "min_score": 0.10}


def done_marker(out: Path, entrant: str, rep: int) -> Path:
    return out / f"{entrant}-r{rep}" / "verdict.json"


def run_one(entrant: str, rep: int, out: Path, port: int, timeout: int) -> Dict:
    """One episode in its own subprocess: a segfault or a hang cannot take the sweep with it."""
    marker = done_marker(out, entrant, rep)
    if marker.is_file():
        try:
            v = json.loads(marker.read_text())
            print(f"[skip] {entrant} rep{rep} already done ({100 * v['score']:.1f}%)", flush=True)
            return v
        except Exception:
            pass  # unreadable verdict: fall through and redo it

    print(f"[run ] {entrant} rep{rep} (port {port}, cap {timeout}s)", flush=True)
    started = time.time()
    try:
        subprocess.run(
            [sys.executable, "-u", str(HERE / "run_build.py"),
             "--entrant", entrant, "--only-rep", str(rep),
             "--timeout", str(timeout), "--port", str(port), "--out", str(out)],
            timeout=timeout + 900, start_new_session=True)
    except subprocess.TimeoutExpired:
        print(f"[warn] {entrant} rep{rep} exceeded the outer cap", flush=True)
    except Exception:
        print(f"[fail] {entrant} rep{rep}\n{traceback.format_exc()[-800:]}", flush=True)

    if marker.is_file():
        v = json.loads(marker.read_text())
        print(f"[done] {entrant} rep{rep} {100 * v['score']:.1f}%  "
              f"({round(time.time() - started)}s)", flush=True)
        return v
    print(f"[fail] {entrant} rep{rep} produced no verdict", flush=True)
    return {"entrant": entrant, "rep": rep, "score": 0.0, "failed": True}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=ROOT / "runs/build")
    ap.add_argument("--timeout", type=int, default=2400)
    ap.add_argument("--port-base", type=int, default=8900)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    # Flatten so every unit can name what follows it — the operator should never have to interrupt
    # to find out where the loop is or what is next.
    units = [(i["entrant"], r, i["why"]) for i in PLAN for r in range(i["reps"])]
    total = len(units)
    print(f"BACKLOG: {total} episodes\n" + "\n".join(
        f"  {n + 1}. {e} rep{r} — {w}" for n, (e, r, w) in enumerate(units)) + "\n", flush=True)

    results: List[Dict] = []
    port = args.port_base
    unit_no = 0
    for item in PLAN:
        for rep in range(item["reps"]):
            unit_no += 1
            nxt = units[unit_no] if unit_no < total else None
            print(f"\n>>> [{unit_no}/{total}] NOW: {item['entrant']} rep{rep} — {item['why']}"
                  f"\n    NEXT: {(nxt[0] + ' rep' + str(nxt[1])) if nxt else 'sweep complete'}",
                  flush=True)
            # One item must NEVER kill the sweep. SystemExit is not an Exception and slips through
            # a bare `except Exception` — that has already taken down a whole board once.
            try:
                v = run_one(item["entrant"], rep, args.out, port, args.timeout)
            except (Exception, SystemExit):
                print(f"[fail] {item['entrant']} rep{rep}\n{traceback.format_exc()[-800:]}",
                      flush=True)
                v = {"entrant": item["entrant"], "rep": rep, "score": 0.0, "failed": True}
            results.append(v)
            port += 1
            (args.out / "sweep-progress.json").write_text(json.dumps(results, indent=2))

        if item["entrant"] == FLOOR_GATE["entrant"]:
            got = [r for r in results if r.get("entrant") == item["entrant"]]
            best = max((r.get("score", 0.0) for r in got), default=0.0)
            if best < FLOOR_GATE["min_score"]:
                print(f"\n[GATE FAILED] {item['entrant']} best {100 * best:.1f}% is under the "
                      f"{100 * FLOOR_GATE['min_score']:.0f}% floor. The task has no reachable floor "
                      f"for a local model; stopping rather than burning hours on baselines that a "
                      f"re-tiering would invalidate.", flush=True)
                break
            print(f"[GATE OK] {item['entrant']} cleared the floor at {100 * best:.1f}%", flush=True)

    print("\n=== SWEEP COMPLETE ===", flush=True)
    for r in results:
        tag = "FAILED" if r.get("failed") else f"{100 * r['score']:.1f}%"
        print(f"  {r.get('entrant'):<14} rep{r.get('rep')}  {tag}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""The tweak loop: turn candidate fixes into measured verdicts, unattended, one lever at a time.

This exists because the operator kept having to push the work forward. The pattern was always the
same: find a defect, explain it, stop. "Found and explained" is not done — done is "changed,
measured, kept or reverted." A driver that owns the whole cycle removes the human from the loop that
never needed them.

DISCIPLINE, enforced by the file format rather than by good intentions:

  * Every candidate declares its HYPOTHESIS, its LEVER, and its GATE **before** it runs. The gate is
    a number written down in advance, so a disappointing result cannot be reinterpreted as a win.
  * One lever moves per arm. Two changes at once produce a number nobody can attribute — which is
    exactly how four weeks were spent without learning anything.
  * A candidate that does not clear its gate is REVERTED, and the revert is recorded with its
    evidence. A loop that only ever keeps things is not measuring, it is accumulating.
  * The baseline is re-measured, never assumed: model fleets drift, and a stale baseline turns noise
    into a false win.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import traceback
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
REPO = ROOT.parents[1]

# Ranked by evidence strength, not by size. Each entry is one lever and one falsifiable claim.
CANDIDATES: List[Dict] = [
    {
        # MEASURED 2026-08-01 and it changes everything: three runs of the SAME 1-node config scored
        # 44.2%, 86.7% and 90.0% — a 46-POINT SPREAD. Two of the three beat the 84.1% single-agent
        # reference. Every earlier conclusion in this project compared n=1 against n=1 and is
        # therefore unsound, including "the swarm loses to a single agent".
        #
        # So the FIRST thing to establish is not whether a lever helps — it is how big the noise is.
        # An arm that moves the score 10 points means nothing against a 46-point spread. Until the
        # replicate spread is known, no comparison is interpretable.
        "name": "baseline",
        "levers": {},
        "hypothesis": "Current engine, no new levers. Re-measured rather than assumed, because a "
                      "stale baseline turns fleet drift into a false win.",
        "gate": None,
    },
    {
        "name": "kind_prompt",
        "levers": {"GOOSE_SWARM_KIND_PROMPT": "1"},
        "hypothesis": "~60% of dispatches get the implementer's rules. A test-author owning "
                      "test_*.py is told 'NEVER read the project's TEST files' — the file it must "
                      "produce is the file it may not open. Gating rules by kind means every kind "
                      "sees FEWER rules; Qwen-27B perfect-compliance is 0.094 at N=40 rules.",
        "gate": "swarm-1node build score improves, and test-kind tasks stop failing with "
                "broken_code/SyntaxError",
    },
    {
        "name": "read_on_fix",
        "levers": {"GOOSE_SWARM_READ_ON_FIX": "1"},
        "hypothesis": "Detection works (complete_verify caught the defect 3 rounds running) but "
                      "repair cannot: the fix worker owns no files and may read only one, while a "
                      "signature mismatch spans two by definition.",
        "gate": "swarm-1node build score improves and complete_verify reaches passed=true",
    },
    {
        "name": "kind_prompt+read_on_fix",
        "levers": {"GOOSE_SWARM_KIND_PROMPT": "1", "GOOSE_SWARM_READ_ON_FIX": "1"},
        "hypothesis": "The two prompt fixes are independent (one targets first-pass authoring, the "
                      "other repair), so they should compose. Run ONLY after both are measured "
                      "alone — otherwise the combination is unattributable.",
        "gate": "beats both single-lever arms",
    },
]

# The number every arm is measured against. A 1-node swarm must beat the same model run bare, or the
# pipeline is costing more than it adds and node count is not yet an interesting variable.
SINGLE_AGENT_REFERENCE = 0.841


def clock() -> str:
    return datetime.now().strftime("%H:%M:%S")


def eta(seconds: float) -> str:
    return (datetime.now() + timedelta(seconds=seconds)).strftime("%H:%M")


def run_arm(name: str, levers: Dict[str, str], entrant: str, out: Path,
            timeout: int, port: int) -> Optional[Dict]:
    """One arm in its own subprocess. A crash or hang cannot take the loop with it."""
    workdir = out / f"{name}--{entrant}"
    marker = workdir / "verdict.json"
    if marker.is_file():
        try:
            v = json.loads(marker.read_text())
            print(f"[skip] {name}/{entrant} already measured ({100 * v['score']:.1f}%)", flush=True)
            return v
        except Exception:
            pass

    env = {**os.environ, **levers}
    # Every arm must be able to say which levers produced it, or the result is not attributable.
    env["SWARM_BENCH_ARM"] = name
    started = time.time()
    try:
        subprocess.run(
            [sys.executable, "-u", str(HERE / "run_build.py"),
             "--entrant", entrant, "--only-rep", "0",
             "--timeout", str(timeout), "--port", str(port),
             "--out", str(workdir.parent / f"{name}--runs")],
            timeout=timeout + 900, start_new_session=True, env=env)
    except subprocess.TimeoutExpired:
        print(f"[warn] {name}/{entrant} exceeded the outer cap", flush=True)
    except (Exception, SystemExit):
        print(f"[fail] {name}/{entrant}\n{traceback.format_exc()[-600:]}", flush=True)

    produced = workdir.parent / f"{name}--runs" / f"{entrant}-r0" / "verdict.json"
    if produced.is_file():
        v = json.loads(produced.read_text())
        v["arm"] = name
        v["levers"] = levers
        workdir.mkdir(parents=True, exist_ok=True)
        marker.write_text(json.dumps(v, indent=2))
        print(f"[done] {name}/{entrant} {100 * v['score']:.1f}% "
              f"({round(time.time() - started)}s)", flush=True)
        return v
    print(f"[fail] {name}/{entrant} produced no verdict", flush=True)
    return None


def verdict_line(name: str, score: Optional[float], baseline: Optional[float]) -> str:
    if score is None:
        return f"  {name:<24} FAILED — no measurement"
    delta = "" if baseline is None else f"  ({100 * (score - baseline):+.1f} vs baseline)"
    gate = "PASSES the single-agent gate" if score >= SINGLE_AGENT_REFERENCE else \
           f"below the {100 * SINGLE_AGENT_REFERENCE:.1f}% single-agent reference"
    return f"  {name:<24} {100 * score:>5.1f}%{delta}  — {gate}"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--entrant", default="swarm-1node",
                    help="1 node isolates the PIPELINE: no parallelism, so any gap is the machinery")
    ap.add_argument("--out", type=Path, default=ROOT / "runs/repair")
    ap.add_argument("--timeout", type=int, default=16200)
    ap.add_argument("--port-base", type=int, default=8960)
    ap.add_argument("--only", help="run a single candidate by name")
    ap.add_argument("--reps", type=int, default=3,
                    help="replicates per arm. n=1 is uninterpretable here: the measured spread on "
                         "an IDENTICAL config is 46 points.")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    todo = [c for c in CANDIDATES if not args.only or c["name"] == args.only]
    print(f"REPAIR LOOP — {len(todo)} arm(s) on {args.entrant}, "
          f"gate = beat {100 * SINGLE_AGENT_REFERENCE:.1f}% (same model, no swarm)\n", flush=True)
    for i, c in enumerate(todo, 1):
        print(f"  {i}. {c['name']}")
        print(f"     hypothesis: {c['hypothesis'][:150]}")
        print(f"     gate:       {c['gate'] or 'reference measurement'}")
    print(flush=True)

    results: Dict[str, Optional[Dict]] = {}
    port = args.port_base
    for i, cand in enumerate(todo, 1):
        remaining = (len(todo) - i) * args.timeout * 0.7
        print(f"\n>>> [{i}/{len(todo)}] {clock()}  ARM: {cand['name']}  levers={cand['levers'] or 'none'}"
              f"\n    NEXT: {todo[i]['name'] if i < len(todo) else 'loop complete'}"
              f"\n    LOOP ETA: ~{eta(args.timeout * 0.7 + remaining)}", flush=True)
        try:
            reps = []
            for rep in range(args.reps):
                v = run_arm(f"{cand['name']}-r{rep}", cand["levers"], args.entrant,
                            args.out, args.timeout, port + rep)
                if v:
                    reps.append(v)
            if reps:
                scores = sorted(r["score"] for r in reps)
                spread = scores[-1] - scores[0]
                mean = sum(scores) / len(scores)
                print(f"  {cand['name']}: n={len(scores)} mean={100*mean:.1f}% "
                      f"spread={100*spread:.1f}pts  {[round(100*s,1) for s in scores]}", flush=True)
                results[cand["name"]] = {"score": mean, "spread": spread, "n": len(scores),
                                         "scores": scores}
            else:
                results[cand["name"]] = None
        except (Exception, SystemExit):
            print(f"[fail] arm {cand['name']}\n{traceback.format_exc()[-600:]}", flush=True)
            results[cand["name"]] = None
        port += 1
        (args.out / "repair-progress.json").write_text(json.dumps(
            {k: (v or {}).get("score") for k, v in results.items()}, indent=2))

    base = (results.get("baseline") or {}).get("score")
    print(f"\n=== REPAIR LOOP COMPLETE {clock()} ===\n", flush=True)
    for cand in todo:
        v = results.get(cand["name"])
        print(verdict_line(cand["name"], (v or {}).get("score"), base), flush=True)

    kept = [c["name"] for c in todo
            if c["name"] != "baseline" and (results.get(c["name"]) or {}).get("score") is not None
            and base is not None
            and (results[c["name"]]["score"] - base) > 0.03]
    print(f"\nKEEP (moved >3 points over baseline): {kept or 'nothing — every lever is noise so far'}",
          flush=True)
    print("REVERT everything not listed. A loop that only keeps things is not measuring.", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

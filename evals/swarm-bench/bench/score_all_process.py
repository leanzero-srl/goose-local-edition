"""Score the PROCESS of every episode that produced a run log, and print one combined table.

Chained automatically at the end of a sweep rather than left as a follow-up. A stage a human has to
remember to trigger is a stage that silently never runs.

Single-agent episodes have no swarm event stream, so most process axes report NOT MEASURABLE for
them — which is the honest outcome and exactly why the swarm entrants exist.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path
from typing import Dict, List

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import score_process

ROOT = Path(__file__).resolve().parents[1]


def collect(out: Path) -> List[Dict]:
    rows = []
    for verdict_path in sorted(out.glob("*/verdict.json")):
        try:
            verdict = json.loads(verdict_path.read_text())
        except Exception:
            continue
        workdir = verdict_path.parent
        entrant, rep = verdict.get("entrant", "?"), verdict.get("rep", 0)

        run_log = workdir / "run.jsonl"
        if not run_log.is_file():
            candidates = sorted(workdir.glob(".swarm/run-*.jsonl"))
            run_log = candidates[0] if candidates else run_log

        trace_path = out / f"trace-{entrant}-r{rep}.jsonl"
        trace = []
        if trace_path.is_file():
            for line in trace_path.read_text(errors="replace").splitlines():
                line = line.strip()
                if line:
                    try:
                        trace.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass

        result = score_process.evaluate(run_log, trace, verdict)
        result.update({"entrant": entrant, "rep": rep,
                       "build_score": verdict.get("score"),
                       "has_run_log": run_log.is_file()})
        (workdir / "process.json").write_text(json.dumps(result, indent=2))
        rows.append(result)
    return rows


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=ROOT / "runs/build")
    args = ap.parse_args()

    rows = collect(args.out)
    stamp = datetime.now().strftime("%H:%M:%S")
    if not rows:
        print(f"[{stamp}] no episodes to score")
        return 0

    print(f"\n[{stamp}] PROCESS SCORES ({len(rows)} episodes)\n")
    header = f"{'entrant':<16}{'rep':>4}{'build':>8}{'process':>9}   " + \
             "  ".join(f"{a[:5].upper():>5}" for a in score_process.AXES)
    print(header)
    for r in sorted(rows, key=lambda r: -(r.get("build_score") or 0)):
        cells = []
        for axis in score_process.AXES:
            mean = r["summary"][axis]["mean"]
            cells.append("    -" if mean is None else f"{100 * mean:>4.0f}%")
        proc = "     -" if r["overall"] is None else f"{100 * r['overall']:>6.1f}%"
        print(f"{r['entrant']:<16}{r['rep']:>4}{100 * (r.get('build_score') or 0):>7.1f}%{proc}   "
              + "  ".join(cells))

    blind = [r for r in rows if not r["has_run_log"]]
    if blind:
        print(f"\n{len(blind)} episode(s) had no swarm event stream, so the process axes are mostly "
              f"NOT MEASURABLE for them — that is what the swarm entrants are for, and it is "
              f"reported rather than imputed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

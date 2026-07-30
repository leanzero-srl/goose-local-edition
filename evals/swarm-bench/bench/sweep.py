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
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List

import score_build  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

# Ordered so the cheapest thing that could invalidate everything runs FIRST.
# local-single is the feasibility gate: if the local fleet cannot resolve the task at all, every
# cloud baseline captured afterwards is wasted effort.
PLAN: List[Dict] = [
    {"entrant": "opus-5", "reps": 2, "why": "true number on the corrected scorer"},
    {"entrant": "local-single", "reps": 1, "why": "FEASIBILITY GATE — local must clear the floor"},
    {"entrant": "sonnet-5", "reps": 2, "why": "baseline"},
    {"entrant": "haiku-4.5", "reps": 2, "why": "baseline"},
    # The swarm entrants are the only ones that emit a process event stream, so they are the only
    # ones the six-axis scorer can fully grade. Node count is the operator's headline comparison.
    {"entrant": "swarm-1node", "reps": 1, "why": "process axes + node scaling"},
    {"entrant": "swarm-2node", "reps": 1, "why": "process axes + node scaling"},
    {"entrant": "swarm-3node", "reps": 1, "why": "process axes + node scaling"},
]

FLOOR_GATE = {"entrant": "local-single", "min_score": 0.10}

# A cap that truncates the work measures the CAP, not the entrant. Measured: every local run hit the
# flat 2400s ceiling — local-single still reached 84.1% because it got far enough, while the swarm
# entrants were cut off mid-build and scored 17-38%, which said nothing about the swarm at all.
# Cloud models finish this task in 250-700s; the local fleet needs far longer, and the swarm longer
# still because it plans, dispatches, judges and gates before it writes much.
TIMEOUT_BY_ENTRANT = {
    "opus-5": 1800, "sonnet-5": 1800, "haiku-4.5": 1800,
    "local-single": 5400,
    "swarm-1node": 9000, "swarm-2node": 9000, "swarm-3node": 9000,
}


def timeout_for(entrant: str, default: int) -> int:
    return TIMEOUT_BY_ENTRANT.get(entrant, default)

# Transient provider failures are the norm on a long unattended run, not the exception: 500, 529
# Overloaded, throttling, a dropped stream. None of them say anything about the model's ability, so
# an episode that dies on one must be RETRIED, not recorded as a score of zero. A zero from an
# overloaded endpoint is a fabricated deduction with extra steps.
TRANSIENT = ("500", "502", "503", "529", "overloaded", "rate limit", "throttl",
             "timed out", "timeout", "connection reset", "stream decode", "temporarily")
MAX_ATTEMPTS = 3
BACKOFF_SECS = (60, 240)


def looks_transient(text: str) -> bool:
    low = (text or "").lower()
    return any(marker in low for marker in TRANSIENT)


def clock() -> str:
    return datetime.now().strftime("%H:%M:%S")


def eta(seconds: float) -> str:
    """Wall-clock moment, not a duration. 'finishes in 20 minutes' forces the reader to do sums."""
    return (datetime.now() + timedelta(seconds=seconds)).strftime("%H:%M")


def done_marker(out: Path, entrant: str, rep: int) -> Path:
    return out / f"{entrant}-r{rep}" / "verdict.json"


def run_one(entrant: str, rep: int, out: Path, port: int, timeout: int) -> Dict:
    """One episode in its own subprocess: a segfault or a hang cannot take the sweep with it."""
    marker = done_marker(out, entrant, rep)
    if marker.is_file():
        try:
            v = json.loads(marker.read_text())
            if v.get("scorer_version") == score_build.SCORER_VERSION:
                print(f"[skip] {entrant} rep{rep} already done on {v['scorer_version']} "
                      f"({100 * v['score']:.1f}%)", flush=True)
                v["skipped"] = True
                return v
            print(f"[stale] {entrant} rep{rep} was scored by "
                  f"{v.get('scorer_version', 'an unversioned grader')}, current is "
                  f"{score_build.SCORER_VERSION} — re-running so the table stays comparable",
                  flush=True)
        except Exception:
            pass  # unreadable verdict: fall through and redo it

    started = time.time()
    for attempt in range(1, MAX_ATTEMPTS + 1):
        print(f"[run ] {entrant} rep{rep} attempt {attempt}/{MAX_ATTEMPTS} "
              f"(port {port + attempt - 1}, cap {timeout}s)", flush=True)
        tail = ""
        try:
            proc = subprocess.run(
                [sys.executable, "-u", str(HERE / "run_build.py"),
                 "--entrant", entrant, "--only-rep", str(rep),
                 "--timeout", str(timeout), "--port", str(port + attempt - 1), "--out", str(out)],
                timeout=timeout + 900, start_new_session=True,
                capture_output=True, text=True)
            tail = (proc.stdout or "") + (proc.stderr or "")
            print(tail[-3000:], flush=True)
        except subprocess.TimeoutExpired:
            tail = "outer cap exceeded"
            print(f"[warn] {entrant} rep{rep} exceeded the outer cap", flush=True)
        except Exception:
            tail = traceback.format_exc()
            print(f"[fail] {entrant} rep{rep}\n{tail[-800:]}", flush=True)

        if marker.is_file():
            try:
                v = json.loads(marker.read_text())
            except Exception:
                v = None
            if v and v.get("scorer_version") == score_build.SCORER_VERSION:
                print(f"[done] {entrant} rep{rep} {100 * v['score']:.1f}%  "
                      f"({round(time.time() - started)}s, attempt {attempt})", flush=True)
                return v

        if attempt < MAX_ATTEMPTS and looks_transient(tail):
            wait = BACKOFF_SECS[min(attempt - 1, len(BACKOFF_SECS) - 1)]
            print(f"[retry] {entrant} rep{rep} hit a transient provider failure — "
                  f"waiting {wait}s then retrying (attempt {attempt + 1}/{MAX_ATTEMPTS})", flush=True)
            time.sleep(wait)
            continue
        break

    print(f"[fail] {entrant} rep{rep} produced no verdict after {MAX_ATTEMPTS} attempt(s)",
          flush=True)
    return {"entrant": entrant, "rep": rep, "score": 0.0, "failed": True,
            "reason": "no verdict after retries"}


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
    durations: List[float] = []
    port = args.port_base
    unit_no = 0
    for item in PLAN:
        for rep in range(item["reps"]):
            unit_no += 1
            nxt = units[unit_no] if unit_no < total else None
            avg = (sum(durations) / len(durations)) if durations else 600.0
            # Remaining time from each unit's OWN cap where we have one, so a 2.5h swarm episode is
            # not averaged away behind six 8-minute cloud runs.
            remaining = sum(min(timeout_for(e, int(avg)), 9000) * 0.8
                            for e, _r, _w in units[unit_no:])
            print(f"\n>>> [{unit_no}/{total}] {clock()}  NOW: {item['entrant']} rep{rep}"
                  f" — {item['why']}"
                  f"\n    NEXT: {(nxt[0] + ' rep' + str(nxt[1])) if nxt else 'process scoring'}"
                  f", starting ~{eta(avg)}"
                  f"\n    SWEEP ETA: ~{eta(avg + remaining)}"
                  f" ({(avg + remaining) / 60:.0f} min left over {total - unit_no + 1} episodes)",
                  flush=True)
            unit_started = time.time()
            # One item must NEVER kill the sweep. SystemExit is not an Exception and slips through
            # a bare `except Exception` — that has already taken down a whole board once.
            try:
                v = run_one(item["entrant"], rep, args.out, port,
                            timeout_for(item["entrant"], args.timeout))
            except (Exception, SystemExit):
                print(f"[fail] {item['entrant']} rep{rep}\n{traceback.format_exc()[-800:]}",
                      flush=True)
                v = {"entrant": item["entrant"], "rep": rep, "score": 0.0, "failed": True}
            results.append(v)
            if not v.get("skipped"):
                durations.append(time.time() - unit_started)
            port += MAX_ATTEMPTS
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

    # CHAIN, do not defer. Leaving the next stage for a human to trigger is how a loop stalls.
    print(f"\n[{clock()}] chaining: process scoring over every episode that produced a run log",
          flush=True)
    try:
        subprocess.run([sys.executable, "-u", str(HERE / "score_all_process.py"),
                        "--out", str(args.out)], timeout=900)
    except Exception:
        print(f"[warn] process scoring failed\n{traceback.format_exc()[-500:]}", flush=True)

    print(f"\n=== SWEEP COMPLETE {clock()} ===", flush=True)
    for r in results:
        tag = "FAILED" if r.get("failed") else f"{100 * r['score']:.1f}%"
        print(f"  {r.get('entrant'):<14} rep{r.get('rep')}  {tag}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

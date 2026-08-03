#!/usr/bin/env python3
"""Sample the LIVE sink's activity digest so its per-interval rate is recoverable. Exit 0.

F185 registered a specific improvement: the sink's seconds-per-call must be measured as PER-CALL
INTERVALS, not a cumulative average, because a running mean cannot show a change WITHIN its own
window — co-tenancy fell from 3 siblings to 1 during r1's sink and the mean could not see it.

Then BOTH obvious data sources turned out to be blind, which is why this file exists:

  · `llm_request.*.jsonl` mtimes — one file per call, so the mtimes ARE the call times. But the logs
    rotate far faster than F176's "under 3 hours" estimate: 15 fleet calls visible across a 2-hour
    window, against 14 counted in a single 8-minute window earlier the same night. The history is
    gone before it can be differenced.
  · `judge_observed` events — they carry (timestamp, tool_calls) and would be perfect. The judge
    emits exactly ONE for `integrate-verify`, because the over-read gate is exempted for tasks that
    own no files (swarm.rs) and the sink owns none. The series does not exist.

So the only live source is the digest itself, which the engine rewrites on stream activity
(coalesced ~2.5/s). Sampling it on a fixed cadence turns a counter into a series.

READ-ONLY, and deliberately so: it opens one JSON file per tick and writes only to its own TSV. It
cannot contend with the run (no ports, no locks, no writes into the run dir) — which matters because
a crunch that bound a harness port once nearly produced a fabricated "the app does not run".

Usage:
    python3 sinkwatch.py                 # watch the newest run's sink until it ends
    python3 sinkwatch.py --interval 20   # sampling cadence in seconds (default 30)
    python3 sinkwatch.py --report        # read back the series and print per-interval s/call
"""
from __future__ import annotations

import datetime
import glob
import json
import os
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"
OUT = HERE / "sink-samples.tsv"
SINK = "integrate-verify"


def live_digest() -> pathlib.Path | None:
    """The newest run dir that has a sink digest. Newest by mtime, not by name — run dirs are
    reused across units (`swarm-3node-r0` was three different runs tonight), so a name-based pick
    silently reads a finished unit."""
    cands = sorted(glob.glob(str(RUNS / "*" / ".swarm" / "activity" / f"{SINK}.json")),
                   key=os.path.getmtime)
    return pathlib.Path(cands[-1]) if cands else None


def sample(p: pathlib.Path) -> tuple[int, int] | None:
    try:
        d = json.loads(p.read_text(errors="replace"))
    except Exception:
        return None                       # a torn read mid-rewrite is normal; skip this tick
    return int(d.get("tool_calls") or 0), int(d.get("thinking_chars") or 0)


def report() -> int:
    if not OUT.is_file():
        print("no samples yet — run `python3 sinkwatch.py` while a sink is live")
        return 0
    rows = []
    for line in OUT.read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        t, calls, think, run = line.split("\t")
        rows.append((float(t), int(calls), int(think), run))
    if len(rows) < 2:
        print(f"{len(rows)} sample(s) — need at least 2 to difference")
        return 0
    print(f"{len(rows)} samples across {len({r[3] for r in rows})} run(s)\n")
    print(f"{'clock':>10}{'calls':>7}{'d_calls':>9}{'d_secs':>8}{'s/call':>9}  run")
    prev = None
    for t, c, th, run in rows:
        dc = ds = None
        if prev and prev[3] == run:
            dc, ds = c - prev[1], t - prev[0]
        # "no change" NOT "stalled": this file samples a digest and cannot tell a worker that is
        # mid-call from one whose task has ENDED. r2's sink finished around 04:39 and every later
        # sample read as "stalled" — which would be a fabricated stall if anyone believed the label.
        # Cross-check the run log for `task_completed` before calling any flat stretch a stall.
        rate = f"{ds/dc:.0f}" if dc else ("no change" if dc == 0 else "")
        print(f"{datetime.datetime.fromtimestamp(t).strftime('%H:%M:%S'):>10}{c:>7}"
              f"{(dc if dc is not None else ''):>9}{(f'{ds:.0f}' if ds is not None else ''):>8}"
              f"{rate:>9}  {run}")
        prev = (t, c, th, run)
    print("\nCompare against the CUMULATIVE figures, which is the whole point of this file:")
    print("  r0 sink SOLO 63 s/call | r1 sink CO-TENANTED 146 | sink_review + idle-fill 257 | fleet 83")
    return 0


def main(argv: list[str]) -> int:
    if "--report" in argv:
        return report()
    interval = 30
    if "--interval" in argv:
        interval = int(argv[argv.index("--interval") + 1])
    p = live_digest()
    if p is None:
        print("no sink digest on disk — nothing to watch")
        return 0
    run = p.parents[2].name
    if not OUT.is_file():
        OUT.write_text("# epoch\ttool_calls\tthinking_chars\trun\n")
    print(f"watching {run}'s sink every {interval}s -> {OUT.name}")
    last = None
    idle_ticks = 0
    while True:
        s = sample(p)
        if s is not None:
            with OUT.open("a") as fh:
                fh.write(f"{time.time():.0f}\t{s[0]}\t{s[1]}\t{run}\n")
            # Stop on our own evidence rather than a fixed duration: if the digest stops changing
            # for many ticks the sink is finished (or wedged) and further samples say nothing new.
            idle_ticks = idle_ticks + 1 if s == last else 0
            last = s
            if idle_ticks >= 20:
                print(f"digest unchanged for {idle_ticks} ticks — sink finished or wedged; stopping")
                return 0
        time.sleep(interval)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

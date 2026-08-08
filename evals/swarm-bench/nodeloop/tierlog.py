#!/usr/bin/env python3
"""Preserve every cell's per-tier score breakdown, append-only.

F555 decomposed the composite score for the first time and found that tier B — behavioural
correctness, the heaviest weight at 0.30 — is the weakest tier in almost every cell, while tier A
(does it exist and run) sits 0.4 higher. F556 then read the check bodies and confirmed both tiers are
graded on comparable proportional scales, so the gap is real and not an artefact of harshness.

THAT FINDING RESTS ON FIVE CELLS WHEN THE CORPUS HOLDS FORTY-TWO REAL RUNS, and the reason is a
storage shape rather than anything about the engine. `nodeloop-result.json` holds ONE row per cell
NAME and is overwritten every time that name is re-run. `baseline-n3-r0` has been five different
runs; the file remembers the fifth. That is the same history-loss that silently destroyed the
campaign's best result in F538, and here it costs the tier decomposition 88% of its sample.

The fix is the shape loop.log already uses: APPEND-ONLY. Each completed unit gets one line, keyed by
the cell name AND its finish timestamp, so re-running a cell adds a row instead of replacing one.

WHY THIS IS A SEPARATE SCRIPT AND NOT A PATCH TO sweep.py: the sweep is RUNNING, and a running
interpreter does not see source edits — a patch would take effect only after a restart, which would
also throw away the in-flight cell. This is tick-driven instead: it scans for result rows it has not
recorded and appends them. At a 5-minute cadence against ~1.9-hour cells, no row can be overwritten
between passes. Patching sweep.py remains the durable answer whenever the sweep is next restarted.
"""
import json
import sys
from pathlib import Path

RUNS = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop")
LOG = RUNS / "tiers.jsonl"


def existing_keys(path=LOG) -> set:
    """(cell, finished_at) pairs already recorded. The timestamp is half the key on purpose: without
    it a re-run of the same cell would look like a duplicate and be dropped, which is precisely the
    overwrite this file exists to undo."""
    keys = set()
    if not path.exists():
        return keys
    for line in path.read_text(errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        keys.add((r.get("cell"), r.get("finished_at")))
    return keys


def harvest(runs=RUNS, log=LOG) -> list:
    """Append any result carrying a `tiers` block that is not already recorded."""
    seen = existing_keys(log)
    added = []
    for p in sorted(runs.glob("*/nodeloop-result.json")):
        try:
            j = json.loads(p.read_text())
        except (json.JSONDecodeError, OSError):
            continue
        if not j.get("tiers"):
            continue
        key = (p.parent.name, j.get("finished_at"))
        if key in seen:
            continue
        # `void` and `wall_secs` ride along so the reader can apply the same real-run filter the rest
        # of the campaign uses, rather than re-deriving it from a second file.
        row = {"cell": p.parent.name, "finished_at": j.get("finished_at"),
               "arm": j.get("arm"), "nodes": j.get("nodes"), "rep": j.get("rep"),
               "score": j.get("score"), "wall_secs": j.get("wall_secs"),
               "void": bool(j.get("void")), "engine_build": j.get("engine_build"),
               "scorer_version": j.get("scorer_version"),
               "tiers": {k: v.get("mean") for k, v in (j.get("tiers") or {}).items()}}
        added.append(row)
        seen.add(key)
    if added:
        with open(log, "a") as f:
            for r in added:
                f.write(json.dumps(r) + "\n")
    return added


def report(log=LOG) -> str:
    if not log.exists():
        return "no tiers.jsonl yet — run `tierlog.py` once to seed it"
    rows = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
    real = [r for r in rows if not r["void"] and (r.get("wall_secs") or 0) >= 1800]
    L = [f"TIER HISTORY  {len(rows)} row(s), {len(real)} real (non-void, >=30 min)"]
    if not real:
        return "\n".join(L)
    L.append(f"  {'arm':<12}{'nodes':>6}{'n':>4}{'score':>8}{'A run':>8}{'B behav':>9}"
             f"{'C vendor':>10}{'D craft':>9}")
    groups: dict = {}
    for r in real:
        groups.setdefault((r["arm"], r["nodes"]), []).append(r)
    for (arm, nodes), v in sorted(groups.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
        m = lambda k: sum((x["tiers"].get(k) or 0) for x in v) / len(v)  # noqa: E731
        s = sum(x["score"] for x in v) / len(v)
        L.append(f"  {arm:<12}{str(nodes):>6}{len(v):>4}{s:>8.3f}{m('A'):>8.3f}{m('B'):>9.3f}"
                 f"{m('C'):>10.3f}{m('D'):>9.3f}")
    return "\n".join(L)


def self_test() -> int:
    """The property that matters: a RE-RUN of the same cell must add a row, never replace one."""
    import tempfile
    fails = []
    d = Path(tempfile.mkdtemp())
    log = d / "t.jsonl"
    cell = d / "baseline-n3-r0"
    cell.mkdir()
    res = cell / "nodeloop-result.json"

    def write(score, when):
        res.write_text(json.dumps({"arm": "baseline", "nodes": 3, "rep": 0, "score": score,
                                   "wall_secs": 6000, "finished_at": when,
                                   "tiers": {"A": {"mean": 0.9}, "B": {"mean": 0.4},
                                             "C": {"mean": 0.5}, "D": {"mean": 0.6}}}))

    write(0.9033, "2026-08-08T10:55:28")
    if len(harvest(d, log)) != 1:
        fails.append("first harvest did not record the row")
    if harvest(d, log):
        fails.append("re-harvesting an unchanged result duplicated it")

    # THE WHOLE POINT: the same cell re-run must not overwrite its predecessor.
    write(0.0561, "2026-08-08T11:34:29")
    if len(harvest(d, log)) != 1:
        fails.append("a RE-RUN of the same cell was not recorded — the overwrite survives")
    rows = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
    if len(rows) != 2 or {r["score"] for r in rows} != {0.9033, 0.0561}:
        fails.append(f"history not preserved: {[r.get('score') for r in rows]}")

    # A result with no tiers block must be skipped rather than recorded as zeros.
    res.write_text(json.dumps({"arm": "baseline", "nodes": 3, "finished_at": "x", "score": 0.5}))
    if harvest(d, log):
        fails.append("a result with no tiers block was recorded anyway")

    for f in fails:
        print(f"  FAIL {f}")
    print(f"tierlog self-test: {'PASS' if not fails else str(len(fails)) + ' FAILURES'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(self_test())
    new = harvest()
    if new:
        print(f"recorded {len(new)} new row(s): " +
              ", ".join(f"{r['cell']}@{r['finished_at']}" for r in new))
    print(report())

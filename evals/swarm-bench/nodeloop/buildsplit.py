"""Label the HEADLINE's rows with the build that produced them.

WHY THIS EXISTS. spread.py produces the campaign's quality headline from `loop.log`, and loop.log
carries NO build field. F639 and F640 both tried to test that headline for build confounding and
BOTH could only reach a different, smaller corpus — F639 stratified 11 rows from tiers.jsonl, F640
stratified 21 rows from `_archive/<sha>/`, and neither is the 48 rows the headline is computed over.
Three corpora currently give that number TWO DIFFERENT SIGNS (+0.0484 / +0.0067 / -0.0202), so the
question "is the headline confounded by build" has been asked three times and answered zero.

THE JOIN, AND WHY IT REACHES ROWS THE ARCHIVE CANNOT. A cell directory is REUSED, so `result.json`
is overwritten every time the sweep re-runs that cell — which is why only 22 archived result rows
survive against 48 real loop.log rows. But the SCORE survives in loop.log itself, and the BUILD
survives in the run's own event log, of which there are TWO archives (F608):
`_archive/logs/<cell>-<epoch>.jsonl` and `eventlogs/<cell>@<iso>.jsonl`. Joining
loop.log(cell, done-time, score) to eventlog(cell, finish-time) -> build_sha therefore labels rows
whose result.json is long gone.

WHAT IT DELIBERATELY DOES NOT DO. It does not retrofit the ten instruments F605 found reading only
live cell dirs. That is the shared `all_runs()` resolver the queue already specifies, and writing a
third bespoke reader into ten files is the duplication L355 exists to prevent. This answers ONE
question and says how much of the corpus it could not reach.
"""

import datetime
import glob
import json
import os
import re
import statistics
import sys
from collections import defaultdict

BENCH = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench"
RUNS = os.path.join(BENCH, "runs", "nodeloop")
LOOP_LOG = os.path.join(RUNS, "loop.log")

DONE = re.compile(r"\[done\] (\S+) (\S+)\s+score=([\d.]+).*?\((\d+) min\)")
REAL_MIN_MINS = 30
MATCH_TOLERANCE_SECS = 240


def _hms(secs_of_day: float) -> str:
    s = int(secs_of_day) % 86400
    return f"{s // 3600:02d}:{s % 3600 // 60:02d}:{s % 60:02d}"


def _to_secs(hms: str) -> int:
    h, m, s = (int(x) for x in hms.split(":"))
    return h * 3600 + m * 60 + s


def _secs_of_day(dt: datetime.datetime) -> int:
    return dt.hour * 3600 + dt.minute * 60 + dt.second


def _local_secs_of_day(ts):
    """The engine stamps `run_finished.ts` in UTC with an explicit offset; loop.log writes LOCAL.

    Measured: the same run reads `2026-08-08T18:49:23+00:00` in its event log and `21:49:27` in
    loop.log. Ignoring the offset made every gap ~10800s and matched 1 row of 43. Converting through
    the offset rather than subtracting a constant 3h keeps this correct across a DST change.
    """
    if not isinstance(ts, str) or "T" not in ts:
        return None
    try:
        return _secs_of_day(datetime.datetime.fromisoformat(ts).astimezone())
    except ValueError:
        return _to_secs(ts.split("T")[1][:8])


def loop_rows() -> list[dict]:
    """The EXACT rows spread.py calls real — same regex, same 30-minute floor."""
    out = []
    if not os.path.exists(LOOP_LOG):
        return out
    for line in open(LOOP_LOG, errors="replace").read().splitlines():
        if not line.startswith("[done]"):
            continue
        m = DONE.search(line)
        if not m:
            continue
        t, cell, score, mins = m.group(1), m.group(2), float(m.group(3)), int(m.group(4))
        arm, _, tail = cell.rpartition("-n")
        nodes = int(tail[0]) if tail and tail[0].isdigit() else None
        if mins < REAL_MIN_MINS or nodes not in (1, 3) or arm != "baseline":
            continue
        out.append({"time": t, "secs": _to_secs(t), "cell": cell,
                    "nodes": nodes, "score": score, "mins": mins})
    return out


def event_logs() -> list[str]:
    """BOTH archives plus the live cells — F608 measured that using only one silently halves this."""
    paths = glob.glob(os.path.join(RUNS, "_archive", "logs", "*.jsonl"))
    paths += glob.glob(os.path.join(RUNS, "eventlogs", "*.jsonl"))
    paths += glob.glob(os.path.join(RUNS, "*", "run.jsonl"))
    return sorted(set(paths))


def log_facts(path: str):
    """(cell, build_sha, finish_secs_of_day, run_id) — None when the log cannot answer."""
    sha = None
    run_id = None
    finish = None
    try:
        with open(path, errors="replace") as fh:
            for line in fh:
                if '"levers_resolved"' not in line and '"run_finished"' not in line \
                        and '"run_started"' not in line:
                    continue
                try:
                    ev = json.loads(line)
                except ValueError:
                    continue
                name = ev.get("event")
                if name == "levers_resolved" and ev.get("build_sha"):
                    sha = str(ev["build_sha"])
                if name in ("run_started", "run_finished"):
                    run_id = ev.get("run_id") or run_id
                if name == "run_finished":
                    ts = ev.get("ts") or ev.get("timestamp") or ev.get("at")
                    finish = _local_secs_of_day(ts)
    except OSError:
        return None
    if finish is None:
        # datetime.fromtimestamp is already LOCAL; `epoch % 86400` would be UTC and reintroduce
        # exactly the three-hour error this function exists to avoid.
        finish = _secs_of_day(datetime.datetime.fromtimestamp(os.path.getmtime(path)))
    base = os.path.basename(path)
    if base == "run.jsonl":
        cell = os.path.basename(os.path.dirname(path))
    else:
        cell = re.split(r"[-@]\d", base, maxsplit=1)[0]
    return {"cell": cell, "sha": sha, "finish": finish, "run_id": run_id, "path": path}


def build_index() -> list[dict]:
    out = []
    for p in event_logs():
        f = log_facts(p)
        if f and f["sha"]:
            out.append(f)
    return out


def join(rows: list[dict], idx: list[dict]):
    by_cell = defaultdict(list)
    for f in idx:
        by_cell[f["cell"]].append(f)
    matched, unmatched = [], []
    for r in rows:
        best, gap = None, None
        for f in by_cell.get(r["cell"], []):
            d = min(abs(f["finish"] - r["secs"]), 86400 - abs(f["finish"] - r["secs"]))
            if gap is None or d < gap:
                best, gap = f, d
        if best is not None and gap is not None and gap <= MATCH_TOLERANCE_SECS:
            matched.append({**r, "sha": best["sha"], "gap": gap})
        else:
            unmatched.append({**r, "gap": gap})
    return matched, unmatched


def coverage_report() -> str:
    rows, idx = loop_rows(), build_index()
    matched, unmatched = join(rows, idx)
    lines = [f"loop.log REAL baseline rows (>= {REAL_MIN_MINS} min, nodes 1 or 3): {len(rows)}",
             f"event logs carrying a build_sha: {len(idx)}",
             f"MATCHED within {MATCH_TOLERANCE_SECS}s: {len(matched)}  "
             f"({100 * len(matched) / len(rows):.1f}% of the headline's rows)" if rows else "no rows",
             f"UNMATCHED: {len(unmatched)} — these are COULD-NOT-LOOK, not clean"]
    per = defaultdict(lambda: defaultdict(int))
    for m in matched:
        per[m["sha"]][m["nodes"]] += 1
    lines.append("")
    lines.append(f"{'build_sha':<18} {'n1':>4} {'n3':>4}")
    for sha in sorted(per, key=lambda s: -(per[s][1] + per[s][3])):
        lines.append(f"{sha:<18} {per[sha][1]:>4} {per[sha][3]:>4}")
    if unmatched:
        lines.append("")
        lines.append("unmatched rows (cell @ done-time, nearest log gap):")
        for u in unmatched:
            g = "no log for this cell" if u["gap"] is None else f"{u['gap']}s away"
            lines.append(f"  {u['cell']:<18} {u['time']}  score={u['score']:.4f}  {g}")
    return "\n".join(lines)


def analysis_report() -> str:
    rows, idx = loop_rows(), build_index()
    matched, unmatched = join(rows, idx)
    if not matched:
        return "NOTHING MATCHED — the join failed, and that emptiness is the finding, not a null."

    by = defaultdict(lambda: defaultdict(list))
    for m in matched:
        by[m["sha"]][m["nodes"]].append(m["score"])

    out = [f"BUILD-LABELLED HEADLINE ROWS: {len(matched)} of {len(rows)} "
           f"({100 * len(matched) / len(rows):.1f}%)", ""]
    out.append(f"{'build_sha':<18} {'n1':>3} {'n3':>3} {'m1':>8} {'m3':>8} {'diff':>9}")
    both = []
    for sha in sorted(by):
        a1, a3 = by[sha][1], by[sha][3]
        m1 = statistics.mean(a1) if a1 else None
        m3 = statistics.mean(a3) if a3 else None
        d = (m3 - m1) if (a1 and a3) else None
        if d is not None:
            both.append((sha, len(a1), len(a3), d))
        out.append(f"{sha:<18} {len(a1):>3} {len(a3):>3} "
                   f"{('%.4f' % m1) if m1 is not None else '    -   ':>8} "
                   f"{('%.4f' % m3) if m3 is not None else '    -   ':>8} "
                   f"{('%+.4f' % d) if d is not None else '   n/a  ':>9}")

    p1 = [s for sha in by for s in by[sha][1]]
    p3 = [s for sha in by for s in by[sha][3]]
    pooled = statistics.mean(p3) - statistics.mean(p1)
    out += ["", f"POOLED over labelled rows: n1={len(p1)} {statistics.mean(p1):.4f} | "
                f"n3={len(p3)} {statistics.mean(p3):.4f} | diff {pooled:+.4f}"]

    if both:
        wts = [min(a, b) for _, a, b, _ in both]
        wtd = sum(w * d for w, (_, _, _, d) in zip(wts, both)) / sum(wts)
        unw = statistics.mean([d for _, _, _, d in both])
        out += [f"STRATIFIED over {len(both)} both-arm build(s): "
                f"unweighted {unw:+.4f}  min-n-weighted {wtd:+.4f}",
                f"SHIFT from pooling: {wtd - pooled:+.4f}  "
                f"(negative = pooling flatters three nodes)"]
        # L441: the equal-n same-build contrast needs no weighting choice at all.
        eq = [(sha, n1, d) for sha, n1, n3, d in both if n1 == n3]
        out.append("")
        if eq:
            out.append("EQUAL-n SAME-BUILD CONTRASTS — no confound, no weighting choice:")
            for sha, n, d in eq:
                out.append(f"  {sha:<18} {n} vs {n}  diff {d:+.4f}")
        else:
            out.append("NO equal-n build exists in the labelled set — every number above "
                       "depends on a weighting choice.")

    unlabelled_n1 = sum(1 for u in unmatched if u["nodes"] == 1)
    unlabelled_n3 = sum(1 for u in unmatched if u["nodes"] == 3)
    out += ["", f"⚠ UNLABELLED AND THEREFORE EXCLUDED: {len(unmatched)} rows "
                f"({unlabelled_n1} one-node, {unlabelled_n3} three-node). "
                f"COULD-NOT-LOOK, not looked-and-found-nothing."]
    return "\n".join(out)


def self_test() -> int:
    """Controls in both directions, plus the vacuous-truth trap."""
    ok = True
    empty_rows, empty_idx = [], []
    m, u = join(empty_rows, empty_idx)
    if m or u:
        print("FAIL: empty input produced matches"); ok = False
    fake_rows = [{"time": "10:00:00", "secs": 36000, "cell": "baseline-n1-r0",
                  "nodes": 1, "score": 0.5, "mins": 90}]
    near = [{"cell": "baseline-n1-r0", "sha": "deadbeef", "finish": 36100,
             "run_id": "x", "path": "/tmp/x"}]
    far = [{"cell": "baseline-n1-r0", "sha": "deadbeef", "finish": 36000 + 9999,
            "run_id": "x", "path": "/tmp/x"}]
    wrong = [{"cell": "baseline-n3-r9", "sha": "deadbeef", "finish": 36000,
              "run_id": "x", "path": "/tmp/x"}]
    if len(join(fake_rows, near)[0]) != 1:
        print("FAIL: a log 100s away did not match"); ok = False
    if len(join(fake_rows, far)[1]) != 1:
        print("FAIL: a log 9999s away DID match — the tolerance is not binding"); ok = False
    if len(join(fake_rows, wrong)[1]) != 1:
        print("FAIL: a DIFFERENT cell matched — the join ignores cell identity"); ok = False
    print("self-test:", "OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    arg = sys.argv[1] if len(sys.argv) > 1 else "coverage"
    if arg == "selftest":
        raise SystemExit(self_test())
    print(coverage_report() if arg == "coverage" else analysis_report())

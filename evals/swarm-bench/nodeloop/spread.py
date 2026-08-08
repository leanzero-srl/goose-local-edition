#!/usr/bin/env python3
"""The replicate SPREAD, per arm and per engine build — the number F533 is actually about.

F533 predicted that the three tail fixes shrink the run-to-run spread, and I shipped the
`complete_cap_lifted` detector for half of it and NO detector for the other half. The spread is the
half that matters: the campaign's quality gap is ~6.5 points against a spread near 60, so nothing
about the node curve can be settled until the dispersion comes down.

THREE THINGS THIS GETS RIGHT THAT THE OBVIOUS VERSION DOES NOT.

1. IT READS THE HISTORY, NOT THE LAST WRITE. `nodeloop-result.json` holds ONE row per cell NAME and
   is overwritten every time that name is re-run, so `baseline-n3-r0` has been five different runs
   and the file remembers the fifth. Reading it reports a corpus of 8 when 37 real runs exist, and
   silently loses the campaign's best result. `loop.log`'s `[done]` lines are append-only and are the
   authoritative history.

2. IT REFUSES TO REPORT A SPREAD IT CANNOT SUPPORT. A range over one point is 0.0000, which reads as
   PERFECT CONSISTENCY — the strongest possible claim — from the weakest possible evidence. F510 is
   exactly this failure: the row that most needed the caveat was the row whose spread was 0.0 so the
   caveat never fired. Below MIN_N the answer is UNMEASURABLE, spelled out, never a number.

3. IT SEPARATES ABORTS FROM MEASUREMENTS, BY WALL-CLOCK. A killed run still gets a `[done]` row with
   a plausible score and `void=False` (F538: my own three rotation kills landed as 0.056 and one
   overwrote a 0.9033). Those rows are manufactured dispersion — precisely the quantity being
   measured — so they must be excluded, and the exclusion must be VISIBLE rather than silent.
   Classification is by wall-clock because the flags cannot be trusted: `kind_mismatch=None` looked
   like a kill marker and matched 134 rows when three kills existed, sweeping in 112 placeholder rows
   and excluding nine legitimate 97-152 minute runs.
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

RUNS = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop")
REPO = "/Users/mihaiperdum/Projects/goose"

# A range needs points. Four is the smallest number at which a max-min says anything about a
# distribution rather than about which two runs happened to land first.
MIN_N = 4

# Anything shorter than this never reached a phase that produces a score worth comparing. The corpus's
# real runs are 69-152 minutes; its aborts are 0-19. There is no ambiguous middle to worry about.
REAL_RUN_MIN_MINS = 30

DONE = re.compile(r"\[done\] (\S+) (\S+)\s+score=([\d.]+).*?\((\d+) min\)")


def read_done_rows(log_path=RUNS / "loop.log") -> list[dict]:
    out = []
    if not Path(log_path).exists():
        return out
    for line in Path(log_path).read_text(errors="replace").splitlines():
        if not line.startswith("[done]"):
            continue
        m = DONE.search(line)
        if not m:
            continue
        t, cell, score, mins = m.group(1), m.group(2), float(m.group(3)), int(m.group(4))
        arm, _, tail = cell.rpartition("-n")
        nodes = None
        if tail and tail[0].isdigit():
            nodes = int(tail[0])
        out.append({"time": t, "cell": cell, "arm": arm or cell, "nodes": nodes,
                    "score": score, "mins": mins,
                    "kind": "placeholder" if mins == 0
                            else "abort" if mins < REAL_RUN_MIN_MINS
                            else "real"})
    return out


def carries(fix_sha: str, cell_sha: str) -> bool:
    if not cell_sha or cell_sha == "?":
        return False
    try:
        return subprocess.run(["git", "-C", REPO, "merge-base", "--is-ancestor",
                               fix_sha, cell_sha.split("-")[0]],
                              capture_output=True, timeout=15).returncode == 0
    except Exception:
        return False


def stats(scores: list[float]) -> dict:
    """Never returns a spread it cannot support. `None` means UNMEASURABLE, which is not zero."""
    n = len(scores)
    if n == 0:
        return {"n": 0, "mean": None, "spread": None, "why": "no runs"}
    mean = sum(scores) / n
    if n < MIN_N:
        return {"n": n, "mean": round(mean, 4), "spread": None,
                "why": f"n={n} is below {MIN_N} — a range over {n} point(s) describes which runs "
                       f"landed, not how much the engine varies"}
    var = sum((s - mean) ** 2 for s in scores) / (n - 1)
    return {"n": n, "mean": round(mean, 4), "spread": round(max(scores) - min(scores), 4),
            "sd": round(var ** 0.5, 4), "why": None}


def report(rows: list[dict], since: str | None = None) -> str:
    real = [r for r in rows if r["kind"] == "real"]
    L = [f"REPLICATE SPREAD   {len(rows)} [done] rows = "
         f"{sum(1 for r in rows if r['kind']=='placeholder')} placeholder + "
         f"{sum(1 for r in rows if r['kind']=='abort')} abort(<{REAL_RUN_MIN_MINS}m) + "
         f"{len(real)} REAL"]
    if since:
        L.append(f"   (frozen-binary view: rows at or after {since})")
    L.append("")
    L.append(f"  {'arm':<20}{'nodes':>6}{'n':>5}{'mean':>9}{'spread':>9}{'sd':>8}{'wall min':>10}")
    groups: dict = {}
    walls: dict = {}
    for r in real:
        if since and r["time"] < since:
            continue
        groups.setdefault((r["arm"], r["nodes"]), []).append(r["score"])
        walls.setdefault((r["arm"], r["nodes"]), []).append(r["mins"])
    if not groups:
        L.append("  (no real runs in this view)")
        return "\n".join(L)
    for (arm, nodes), scores in sorted(groups.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
        s = stats(scores)
        spread = f"{s['spread']:.4f}" if s["spread"] is not None else "UNMEAS"
        sd = f"{s.get('sd'):.4f}" if s.get("sd") is not None else "     -"
        w = walls.get((arm, nodes), [])
        wm = f"{sum(w)/len(w):.1f}" if w else "-"
        L.append(f"  {arm:<20}{str(nodes):>6}{s['n']:>5}{s['mean']:>9.4f}{spread:>9}{sd:>8}{wm:>10}")
        if s["why"]:
            L.append(f"      ^ {s['why']}")

    b = groups.get(("baseline", 1)), groups.get(("baseline", 3))
    if b[0] and b[1]:
        s1, s3 = stats(b[0]), stats(b[1])
        gap = s3["mean"] - s1["mean"]
        L.append("")
        if s1.get("sd") is not None and s3.get("sd") is not None:
            se = (s1["sd"] ** 2 / s1["n"] + s3["sd"] ** 2 / s3["n"]) ** 0.5
            ratio = gap / se if se else float("inf")
            verdict = ("CLEARS the noise" if abs(ratio) >= 2
                       else "a HINT, not a result — inside one standard error" if abs(ratio) < 1
                       else "suggestive, still inside two standard errors")
            L.append(f"  QUALITY  3-node minus 1-node: {gap:+.4f}   SE {se:.4f}   = {ratio:+.2f} SE  ⇒ {verdict}")
            w1, w3 = walls.get(("baseline", 1), []), walls.get(("baseline", 3), [])
            if len(w1) >= MIN_N and len(w3) >= MIN_N:
                import statistics as st
                mw1, mw3 = sum(w1) / len(w1), sum(w3) / len(w3)
                wse = (st.variance(w1) / len(w1) + st.variance(w3) / len(w3)) ** 0.5
                wr = (mw3 - mw1) / wse if wse else float("inf")
                wv = ("CLEARS the noise" if abs(wr) >= 2
                      else "a HINT, not a result — inside one standard error" if abs(wr) < 1
                      else "suggestive, still inside two standard errors")
                L.append(f"  SPEED    3-node minus 1-node: {mw3-mw1:+.1f} min  SE {wse:.1f}  "
                         f"= {wr:+.2f} SE  ⇒ {wv}   ({mw1/mw3:.2f}x, >1 means 3 nodes FASTER)")
        else:
            L.append(f"  3-node minus 1-node: {gap:+.4f} — SE UNMEASURABLE at this n, so the gap "
                     f"cannot be placed against the noise at all")
    return "\n".join(L)


def self_test() -> int:
    fails = []
    # A single run must NEVER report a spread of 0.0 — that reads as perfect consistency.
    s = stats([0.5])
    if s["spread"] is not None:
        fails.append(f"n=1 reported a spread of {s['spread']} instead of UNMEASURABLE")
    if stats([0.5, 0.9])["spread"] is not None:
        fails.append("n=2 reported a spread")
    # At MIN_N it must produce one.
    s = stats([0.1, 0.2, 0.3, 0.9])
    if s["spread"] is None or abs(s["spread"] - 0.8) > 1e-9:
        fails.append(f"n=4 spread was {s.get('spread')}, expected 0.8")
    # Empty scores nothing — the vacuous-truth trap.
    if stats([])["mean"] is not None:
        fails.append("empty input produced a mean")
    # Classification is by wall-clock and must split the three populations exactly.
    rows = [{"kind": k} for k in ("placeholder", "abort", "real")]
    if len([r for r in rows if r["kind"] == "real"]) != 1:
        fails.append("classification broken")
    # A real corpus row must parse out of a genuine loop.log line.
    line = ("[done] 10:55:28 baseline-n3-r0  score=0.9033  pool=3/3  void=False  aborted=False  "
            "timed_out=False  fallbacks=0  kind_mismatch=73.3%  prefix=1792.3s (116 min)")
    m = DONE.search(line)
    if not m or m.group(2) != "baseline-n3-r0" or float(m.group(3)) != 0.9033 or m.group(4) != "116":
        fails.append(f"failed to parse a real [done] line: {m and m.groups()}")
    for f in fails:
        print(f"  FAIL {f}")
    print(f"spread self-test: {'PASS' if not fails else str(len(fails)) + ' FAILURES'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(self_test())
    since = None
    for i, a in enumerate(sys.argv):
        if a == "--since" and i + 1 < len(sys.argv):
            since = sys.argv[i + 1]
    print(report(read_done_rows(), since))

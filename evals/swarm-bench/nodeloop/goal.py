#!/usr/bin/env python3
"""THE STANDING SCOREBOARD — two pillars, never one.

Mihai, 19:05: "make sure both quality and speed are the pillars to look out for and gradually
improve."

That is the whole contract of this file. QUALITY is the build score; SPEED is wall-clock. Every row
prints both, so a gain in one that was bought with the other cannot be reported as progress. Today
that mattered: three nodes score +0.18 and take 20% longer, and either number alone tells a story
the other contradicts.

SPLIT BY build_sha, ALWAYS. Cell directories are reused, so a cell name identifies a slot and never a
generation. Pooling across commits once produced a "the wall gap has closed to parity" headline that
was an artefact of one cell built from a DIRTY tree (F479). Rows here are grouped by commit and the
cross-commit mean is never computed.

WITHIN-ARM SPREAD IS PRINTED BESIDE EVERY DELTA. A gap smaller than the spread of the arm it came
from is a direction, not an effect, and this campaign has already published three such gaps as
findings before retracting them.
"""
import json
import os
import subprocess
import sys
import re
from statistics import mean

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import shardshare  # noqa: E402  (path is set above)


ARCHIVE = os.path.join(RUNS, "_archive")


def archive(cell: str, sha: str, result: dict, mtime: float) -> None:
    """SNAPSHOT EVERY RESULT, BECAUSE THE BENCH OVERWRITES ITS OWN HISTORY.

    The sweep re-runs a cell IN THE SAME DIRECTORY and replaces `nodeloop-result.json`. Measured: the
    0.9178 / 180-min cell that anchored a whole morning's comparison vanished mid-session, taking its
    commit from n=2 vs 2 down to n=1 vs 2 — the evidence for a claim deleted while the claim stood.

    Keyed by build_sha AND mtime, so a re-run on the same commit ACCUMULATES as a replicate rather
    than overwriting its predecessor. Replicates are the scarcest thing in this campaign: every
    outcome comparison so far has died on a within-arm spread measured at n=2.

    Archiving happens here rather than in the sweep on purpose — the sweep is a running interpreter
    and would not see the edit (L265). Reading the scoreboard is what preserves the data.
    """
    d = os.path.join(ARCHIVE, sha)
    os.makedirs(d, exist_ok=True)
    path = os.path.join(d, f"{cell}-{int(mtime)}.json")
    if not os.path.exists(path):
        with open(path, "w") as fh:
            json.dump(result, fh)


def archived() -> list[dict]:
    out = []
    if not os.path.isdir(ARCHIVE):
        return out
    for sha in sorted(os.listdir(ARCHIVE)):
        for f in sorted(os.listdir(os.path.join(ARCHIVE, sha))):
            try:
                r = json.load(open(os.path.join(ARCHIVE, sha, f)))
            except Exception:
                continue
            pool = r.get("actual_pool")
            if not pool or r.get("score") is None:
                continue
            cell, _, stamp = f[:-5].rpartition("-")
            out.append({"cell": cell, "sha": sha, "nodes": len(pool),
                        "quality": float(r["score"]),
                        "speed_min": (r.get("wall_secs") or 0) / 60.0,
                        "mtime": float(stamp) if stamp.isdigit() else 0.0,
                        "archived": True})
    return out


def cells() -> list[dict]:
    out = []
    for d in sorted(os.listdir(RUNS)):
        res = os.path.join(RUNS, d, "nodeloop-result.json")
        if not os.path.exists(res):
            continue
        try:
            r = json.load(open(res))
        except Exception:
            continue
        pool = r.get("actual_pool")
        if not pool or r.get("score") is None:
            continue  # never scored, or scored with no fleet — not a data point
        try:
            ev = shardshare.load(d)
            sha = shardshare.build_sha(ev)
        except SystemExit:
            sha = "?"
        mt = os.path.getmtime(res)
        archive(d, sha, r, mt)
        out.append({
            "cell": d, "sha": sha, "nodes": len(pool),
            "quality": float(r["score"]), "speed_min": (r.get("wall_secs") or 0) / 60.0,
            "mtime": mt, "archived": False,
        })
    # Merge the archive in, de-duplicated on (sha, cell, mtime) so a live result and its own snapshot
    # never double-count into a mean.
    seen = {(r["sha"], r["cell"], int(r["mtime"])) for r in out}
    for a in archived():
        if (a["sha"], a["cell"], int(a["mtime"])) not in seen:
            out.append(a)
    return out


def occupancy_of(cell: str):
    """Device-busy occupancy — the only occupancy measure validated (F480/F483). Slot utilisation is
    WITHDRAWN and must not be resurrected here without settling the 3-on-2 artefact first."""
    try:
        o = subprocess.run(["python3", os.path.join(HERE, "occupancy.py"),
                            os.path.join(RUNS, cell)], capture_output=True, text=True, timeout=120).stdout
    except Exception:
        return None
    m = re.search(r"EXECUTE OCCUPANCY ([0-9.]+)", o)
    return float(m.group(1)) if m else None


def main() -> int:
    rows = cells()
    if not rows:
        print("no scored cells yet")
        return 2
    by_sha: dict[str, list[dict]] = {}
    for r in rows:
        by_sha.setdefault(r["sha"], []).append(r)

    print("=" * 78)
    print("GOAL: does MORE THAN ONE NODE buy both QUALITY and SPEED?")
    print("  QUALITY = build score (higher better)   SPEED = wall minutes (LOWER better)")
    print("=" * 78)

    # Newest commit last, so 'gradually improve' reads top-to-bottom.
    for sha in sorted(by_sha, key=lambda s: max(r["mtime"] for r in by_sha[s])):
        grp = by_sha[sha]
        print(f"\n--- build_sha {sha} ---")
        for r in sorted(grp, key=lambda x: (x["nodes"], x["cell"])):
            occ = occupancy_of(r["cell"])
            print(f"  {r['cell']:<18} {r['nodes']}-node   quality {r['quality']:.4f}   "
                  f"speed {r['speed_min']:6.0f} min   occ {occ if occ is not None else '?'}")
        n1 = [r for r in grp if r["nodes"] == 1]
        nm = [r for r in grp if r["nodes"] > 1]
        if not (n1 and nm):
            print("  (no paired comparison in this commit — a lone arm proves nothing)")
            continue
        dq = mean(r["quality"] for r in nm) - mean(r["quality"] for r in n1)
        ds = mean(r["speed_min"] for r in nm) / mean(r["speed_min"] for r in n1)
        spread = max(r["quality"] for r in nm) - min(r["quality"] for r in nm)
        print(f"  => QUALITY {dq:+.4f}   SPEED {ds:.2f}x   "
              f"(multi-node within-arm spread {spread:.4f}, n={len(nm)} vs {len(n1)})")
        # BOTH PILLARS OR IT IS NOT PROGRESS.
        verdict = ("BOTH PILLARS" if dq > 0 and ds < 1.0 else
                   "QUALITY ONLY — bought with time" if dq > 0 else
                   "SPEED ONLY — bought with quality" if ds < 1.0 else
                   "NEITHER")
        print(f"     VERDICT: {verdict}")
        if dq > 0 and abs(dq) <= spread:
            print(f"     ⚠ the quality gap ({dq:+.4f}) is INSIDE the multi-node spread "
                  f"({spread:.4f}) — a DIRECTION, not a proven effect")
    return 0


if __name__ == "__main__":
    sys.exit(main())

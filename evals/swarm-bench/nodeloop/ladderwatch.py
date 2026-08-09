#!/usr/bin/env python3
"""Evaluate F654 — did the pool-invariant fix stop the confidence ladder firing?

WHY THIS IS A FILE AND NOT ANOTHER THROWAWAY SCRIPT. Every corpus error this campaign has produced
came from an ad-hoc analysis written once to answer one question: the double-counting bug (F651), the
per-cell log join that REVERSED a published finding (F669), and the display-label artefact that
produced a wrong mechanism story twice (F668, F673). The shipped instruments were clean every time.
So the ladder verdict — which decides whether an engine change stays — gets an instrument with
controls, not a heredoc.

THE THREE PRE-REGISTERED PREDICTIONS (F654), evaluated exactly as written:
  P1  ladder rate below 20% on post-fix 3-node runs
  P2  post-fix 3-node planning below 20 min
  P3  CONTROL — 1-node planning stays within 2 min of 12.8. If it MOVES, the fix is REVERTED rather
      than explained: one node cannot reach the changed code path (it drafts ONE skeleton, so no
      cross-draft agreement exists and no convergence round occurs), so a one-node change means the
      edit did something nobody predicted.

⚠️ NEVER POOL ACROSS BINARIES. `engine_build` identifies the binary; the fix landed 2026-08-09
09:09:41. Rows from before that measure a different engine and are reported separately, never merged.
"""
from __future__ import annotations

import datetime as dt
import statistics as st
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from buildsplit import result_rows  # noqa: E402

FIX_LANDED = dt.datetime(2026, 8, 9, 9, 9, 41)
LADDER_RATE_BAR = 0.20
PLANNING_BAR_MIN = 20.0
ONE_NODE_REF_MIN = 12.8
ONE_NODE_TOLERANCE_MIN = 2.0
MIN_N = 3


def _build_dt(engine_build) -> dt.datetime | None:
    try:
        return dt.datetime.fromtimestamp(int(str(engine_build).split("-")[0]))
    except (ValueError, TypeError, OSError):
        return None


def vintage(row: dict) -> str:
    """POST if this row ran on a binary built at or after the fix, PRE if before, UNKNOWN otherwise.

    UNKNOWN is its own bucket rather than being folded into PRE. A row whose binary cannot be dated
    is a row that cannot join either arm, and silently assigning it to one is how a corpus acquires
    a result nobody measured.
    """
    d = _build_dt(row.get("engine_build"))
    if d is None:
        return "UNKNOWN"
    return "POST" if d >= FIX_LANDED else "PRE"


def rows_by_vintage(nodes: int) -> dict:
    out = {"PRE": [], "POST": [], "UNKNOWN": []}
    for r in result_rows():
        if r.get("void") or r.get("actual_nodes") != nodes:
            continue
        if not (r.get("prefix") or {}).get("prefix_secs"):
            continue
        out[vintage(r)].append(r)
    return out


def _planning_min(rows: list) -> list:
    return [r["prefix"].get("planning_secs", 0) / 60 for r in rows]


def _ladders(rows: list) -> list:
    """redraft_rounds, EXCLUDING rows where it is None.

    None means the field was never recorded, NOT that the run did not ladder. Treating missing as
    zero produced a phantom collinearity that had to be withdrawn 90 minutes later (F646).
    """
    return [r["prefix"]["redraft_rounds"] for r in rows
            if r["prefix"].get("redraft_rounds") is not None]


def _fmt(rows: list, label: str) -> str:
    if not rows:
        return f"  {label:6s} n=0"
    pl = _planning_min(rows)
    ld = _ladders(rows)
    pf = [r["prefix"]["prefix_secs"] / 60 for r in rows]
    rate = (sum(1 for x in ld if x > 0) / len(ld)) if ld else None
    line = (f"  {label:6s} n={len(rows):2d}  planning {st.mean(pl):5.1f} min"
            f"  prefix {st.mean(pf):5.1f} min")
    if rate is not None:
        line += f"  laddered {sum(1 for x in ld if x > 0)}/{len(ld)} = {100 * rate:4.0f}%  rounds {sorted(ld, reverse=True)}"
    else:
        line += "  redraft_rounds UNRECORDED on every row"
    return line


def report() -> int:
    three = rows_by_vintage(3)
    one = rows_by_vintage(1)

    print("=" * 78)
    print(f"F654 — did the pool-invariant fix stop the ladder?   fix landed {FIX_LANDED}")
    print("=" * 78)
    print("THREE NODES:")
    for k in ("PRE", "POST", "UNKNOWN"):
        print(_fmt(three[k], k))
    print("ONE NODE (the control arm — the fix cannot reach this code path):")
    for k in ("PRE", "POST", "UNKNOWN"):
        print(_fmt(one[k], k))

    post3, post1 = three["POST"], one["POST"]
    print("\n" + "-" * 78)

    if len(post3) < MIN_N:
        print(f"P1/P2: INSUFFICIENT — {len(post3)} post-fix 3-node run(s), minimum is {MIN_N}.")
        print("       Reporting the shortfall rather than interpreting it. This is not a null result.")
    else:
        ld = _ladders(post3)
        if not ld:
            print("P1: UNMEASURABLE — no post-fix 3-node row records redraft_rounds.")
        else:
            rate = sum(1 for x in ld if x > 0) / len(ld)
            print(f"P1 ladder rate < {LADDER_RATE_BAR:.0%}: "
                  f"{'PASS' if rate < LADDER_RATE_BAR else 'FAIL'} — {100 * rate:.0f}% ({sum(1 for x in ld if x > 0)}/{len(ld)})")
        pl = _planning_min(post3)
        print(f"P2 planning < {PLANNING_BAR_MIN:.0f} min: "
              f"{'PASS' if st.mean(pl) < PLANNING_BAR_MIN else 'FAIL'} — {st.mean(pl):.1f} min"
              f"{f' (sd {st.stdev(pl):.1f})' if len(pl) > 1 else ''}")

    if len(post1) < MIN_N:
        print(f"P3 CONTROL: INSUFFICIENT — {len(post1)} post-fix 1-node run(s), minimum is {MIN_N}.")
        print("       ⚠️ P1/P2 MUST NOT BE ACTED ON WITHOUT THIS CONTROL. A 3-node improvement with no")
        print("       1-node control cannot distinguish 'the ladder stopped' from 'the engine changed'.")
    else:
        pl1 = _planning_min(post1)
        drift = abs(st.mean(pl1) - ONE_NODE_REF_MIN)
        ok = drift <= ONE_NODE_TOLERANCE_MIN
        print(f"P3 CONTROL 1-node planning within {ONE_NODE_TOLERANCE_MIN:.0f} min of {ONE_NODE_REF_MIN}: "
              f"{'PASS' if ok else 'FAIL'} — {st.mean(pl1):.1f} min (drift {drift:.1f})")
        if not ok:
            print("       🔴 REVERT a9f43543d. One node cannot reach the changed code path, so a shift")
            print("       here means the edit did something nobody predicted. Do not explain it away.")

    print("-" * 78)
    print("⚠️ WHAT A PASS DOES AND DOES NOT MEAN (F661): P1/P2 passing proves THE LADDER STOPPED")
    print("   FIRING — a mechanism verdict, valid at small n. It does NOT prove the run got 15%")
    print("   faster. That is the goal's outcome claim, it costs ~44 runs per arm, and reporting the")
    print("   mechanism win as the goal win is a proxy-metric failure.")
    return 0


def selftest() -> None:
    """Controls in BOTH directions — a test that only passes on good input is half a test."""
    assert vintage({"engine_build": "1786178750-235858544"}) == "PRE", "pre-fix binary must read PRE"
    late = int(FIX_LANDED.timestamp()) + 60
    assert vintage({"engine_build": f"{late}-1"}) == "POST", "post-fix binary must read POST"
    early = int(FIX_LANDED.timestamp()) - 60
    assert vintage({"engine_build": f"{early}-1"}) == "PRE", "one minute before the fix is PRE"
    assert vintage({"engine_build": None}) == "UNKNOWN", "an undateable build is UNKNOWN, never PRE"
    assert vintage({}) == "UNKNOWN", "a missing build is UNKNOWN, never PRE"
    # redraft_rounds: None must be EXCLUDED, not counted as zero (F646).
    rows = [{"prefix": {"redraft_rounds": None}}, {"prefix": {"redraft_rounds": 0}},
            {"prefix": {"redraft_rounds": 2}}]
    assert _ladders(rows) == [0, 2], "None must be dropped, not read as 'did not ladder'"
    # POSITIVE CONTROL on the corpus itself: the instrument must SEE the known pre-fix population.
    three = rows_by_vintage(3)
    assert three["PRE"], "POSITIVE CONTROL FAILED: no pre-fix 3-node rows visible — instrument blind"
    ld = _ladders(three["PRE"])
    assert any(x > 0 for x in ld), "POSITIVE CONTROL FAILED: cannot see a ladder that is known present"
    one = rows_by_vintage(1)
    assert one["PRE"] and all(x == 0 for x in _ladders(one["PRE"])), \
        "NEGATIVE CONTROL FAILED: one node must show zero ladders (measured 11 of 11)"
    print(f"selftest: PASS — sees {len(three['PRE'])} pre-fix 3-node rows with ladders present, "
          f"{len(one['PRE'])} one-node rows with none, and drops None rather than counting it as 0")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        selftest()
    else:
        selftest()
        print()
        sys.exit(report())

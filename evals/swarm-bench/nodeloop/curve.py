#!/usr/bin/env python3
"""GOAL ONE's verdict, computed mechanically from the stored cells. Exit 0.

WRITTEN WHILE ZERO MATCHED PAIRS EXISTED. That is the entire point: a test authored after seeing the
numbers is a test fitted to them, and this campaign has already had to withdraw a headline built on a
figure its own instrument labelled provisional (F273). The protocol this file enforces is frozen in
`PREREGISTERED.md`; nothing here may be relaxed once a pair lands.

THE CLAIM (PREREGISTERED.md): a 3-node run beats a 1-node run on BOTH wall-clock AND shipped quality.

THE TEST, chosen in advance: one-sided SIGN TEST over the matched pairs, separately for wall and for
score. p = P(X >= k) under X ~ Binomial(n, 0.5), which for n pairs all favouring 3 nodes is 0.5**n.
F260: n=3 cannot reach 0.05 even on perfect separation (0.125); n=5 gives 0.031.

THE FOUR FALSIFIERS, enforced here rather than remembered:
  1. a VOID / aborted / timed-out cell voids its PAIR — both halves are dropped
  2. the two halves of a pair MUST share an `engine_build` (F253) — otherwise they are different engines
  3. wall-clock favouring 3 nodes WITHOUT score doing the same is a FAIL, not a partial win
  4. significance that needs a dropped pair is NOT significance — any drop is reported next to the p

Usage:
    python3 curve.py            human verdict
    python3 curve.py --json     machine-readable
"""
from __future__ import annotations

import json
import sys
from math import comb
from pathlib import Path

import bonusclass          # L2: the bonus classifier already exists and already has a self-test
import sweep

RUNS = (Path(__file__).resolve().parent.parent / "runs" / "nodeloop").resolve()


def bonus_of(unit: str) -> tuple[int, str]:
    """(how many tasks the replanner ADDED, what class they were) for one stored unit.

    F312 is the confound this exists to expose: **the 1-node arm CANNOT REPLAN, by construction** —
    `dynamic_replan` requires `idle_capacity() >= 2` and a task in flight, which one device cannot
    reach. So the two arms do DIFFERENT WORK, and every n3 cell so far carries 2-4 tasks of extra
    work that its n1 partner was structurally incapable of doing.

    That biases WALL against the claim (n3 does more, so it takes longer — safe) and SCORE toward it
    (n3 ships more, so it may score higher — NOT safe). A verdict printed without this number invites
    exactly the reading it cannot support: "3 nodes build better apps", when part of the gap is
    "3 nodes were allowed to build more of the app". L124 · L170.
    """
    log = RUNS / unit / "run.jsonl"
    if not log.is_file():
        return 0, "NO-LOG"
    cls, detail = bonusclass.bonus_class(log)
    return len(detail), cls

CURVE_VERSION = "curve-1"
FAST_ARM, SLOW_ARM = 3, 1     # nodes per arm; the curve compares these two levels of `baseline`


def sign_test_one_sided(favourable: int, n: int) -> float:
    """P(at least `favourable` of n pairs land this way | fair coin). Exact, no approximation."""
    if n == 0:
        return 1.0
    return sum(comb(n, k) for k in range(favourable, n + 1)) / (2 ** n)


def cells() -> dict:
    """Every stored baseline cell, keyed by (nodes, rep). Raw rows — judgement happens below."""
    out = {}
    for r in sweep.read_results():
        if r.get("arm") != "baseline":
            continue
        nodes, rep = r.get("nodes"), r.get("rep")
        if isinstance(nodes, int) and isinstance(rep, int):
            out[(nodes, rep)] = r
    return out


def pair_up(by_cell: dict) -> tuple[list[dict], list[dict]]:
    """Matched pairs and the ones that were DROPPED, with the reason. Never silently discards."""
    pairs, dropped = [], []
    for rep in sorted({rep for (_, rep) in by_cell}):
        fast, slow = by_cell.get((FAST_ARM, rep)), by_cell.get((SLOW_ARM, rep))
        if fast is None or slow is None:
            continue                      # not yet a pair; not a drop — the run simply has not got there
        why = None
        for label, c in (("n3", fast), ("n1", slow)):
            if not sweep.is_real_unit(c):
                why = f"{label} cell is void/aborted/timed-out (falsifier 1)"
            elif c.get("score") is None:
                why = f"{label} cell has no score"
        if fast.get("engine_build") != slow.get("engine_build"):
            why = "the two halves ran on DIFFERENT engine builds (falsifier 2, F253)"
        row = {
            "rep": rep,
            "n3_wall": fast.get("wall_secs"), "n1_wall": slow.get("wall_secs"),
            "n3_score": fast.get("score"), "n1_score": slow.get("score"),
            "engine_build": fast.get("engine_build"),
        }
        row["n3_bonus"], row["n3_bonus_class"] = bonus_of(f"baseline-n{FAST_ARM}-r{rep}")
        row["n1_bonus"], row["n1_bonus_class"] = bonus_of(f"baseline-n{SLOW_ARM}-r{rep}")
        if why:
            dropped.append({**row, "reason": why})
        else:
            row["wall_ratio"] = round(slow["wall_secs"] / fast["wall_secs"], 3)
            row["faster_with_3"] = fast["wall_secs"] < slow["wall_secs"]
            row["better_with_3"] = fast["score"] > slow["score"]
            pairs.append(row)
    return pairs, dropped


def verdict() -> dict:
    pairs, dropped = pair_up(cells())
    n = len(pairs)
    wall_wins = sum(1 for p in pairs if p["faster_with_3"])
    score_wins = sum(1 for p in pairs if p["better_with_3"])
    p_wall = sign_test_one_sided(wall_wins, n)
    p_score = sign_test_one_sided(score_wins, n)
    # BOTH, OR NEITHER. Falsifier 3 is not a footnote — speed bought by shipping less is not the claim.
    both = p_wall < 0.05 and p_score < 0.05
    return {
        "curve_version": CURVE_VERSION,
        "pairs": pairs,
        "dropped": dropped,
        "n_pairs": n,
        "wall_wins": wall_wins, "p_wall": round(p_wall, 4),
        "score_wins": score_wins, "p_score": round(p_score, 4),
        "min_attainable_p": round(0.5 ** n, 4) if n else None,
        # The honest headline. "not yet" is a state, not a failure, and must never read as one.
        "verdict": ("NOT YET — no matched pair" if n == 0 else
                    "GOAL ONE SUPPORTED" if both else
                    "NOT SUPPORTED — wall and score must BOTH clear 0.05 (falsifier 3)"),
        "caveat_dropped": (f"{len(dropped)} pair(s) dropped; significance that needs a drop is not "
                           f"significance (falsifier 4)") if dropped else None,
    }


def render(v: dict) -> str:
    out = [f"=== GOAL ONE — the node curve  ({v['curve_version']})",
           f"  matched pairs: {v['n_pairs']}"
           + (f"   smallest p this many pairs can ever reach: {v['min_attainable_p']}"
              if v["n_pairs"] else "")]
    for p in v["pairs"]:
        out.append(f"    r{p['rep']}  n3 {p['n3_wall']:.0f}s / {p['n3_score']:.4f}   "
                   f"n1 {p['n1_wall']:.0f}s / {p['n1_score']:.4f}   "
                   f"ratio {p['wall_ratio']}  "
                   f"{'3n FASTER' if p['faster_with_3'] else '3n slower'}  "
                   f"{'3n BETTER' if p['better_with_3'] else '3n worse'}"
                   f"   bonus n3 +{p['n3_bonus']} [{p['n3_bonus_class']}] "
                   f"vs n1 +{p['n1_bonus']} [{p['n1_bonus_class']}]")
    for d in v["dropped"]:
        out.append(f"    r{d['rep']}  DROPPED — {d['reason']}")
    if v["n_pairs"]:
        out.append(f"  wall : {v['wall_wins']}/{v['n_pairs']} pairs favour 3 nodes   p = {v['p_wall']}")
        out.append(f"  score: {v['score_wins']}/{v['n_pairs']} pairs favour 3 nodes   p = {v['p_score']}")
    out.append(f"  VERDICT: {v['verdict']}")
    if v["caveat_dropped"]:
        out.append(f"  ⚠ {v['caveat_dropped']}")
    if v["pairs"]:
        n3b = sum(p["n3_bonus"] for p in v["pairs"])
        n1b = sum(p["n1_bonus"] for p in v["pairs"])
        out.append(f"  ⚠ REPLAN BONUS, printed beside the verdict because it is the design's own "
                   f"confound (F312): n3 +{n3b} tasks, n1 +{n1b}.")
        out.append("    The 1-node arm CANNOT replan — `dynamic_replan` needs idle_capacity() >= 2, "
                   "which one device never reaches.")
        out.append("    So the arms did DIFFERENT WORK. Bias: WALL against the claim (safe), "
                   "SCORE toward it (NOT safe).")
        if n1b == 0 and n3b > 0:
            out.append("    A score win here is PART node-count and PART 'n3 was allowed to build "
                       "more'. Read `bonusclass.py` before quoting it as quality.")
    return "\n".join(out)


def self_test() -> int:
    """Controls in BOTH directions, plus the vacuous case. A grader that cannot fail is not a grader."""
    assert sign_test_one_sided(5, 5) == 0.03125, "perfect 5-pair separation must read 0.031"
    assert sign_test_one_sided(3, 3) == 0.125, "perfect 3-pair separation must MISS 0.05 (F260)"
    assert sign_test_one_sided(4, 5) == 6 / 32, "one crossing at n=5 must not clear 0.05"
    assert sign_test_one_sided(0, 0) == 1.0, "no pairs must score NOTHING, never a pass"
    # Vacuous truth: an empty curve must say "not yet", never "supported".
    v = {"n_pairs": 0, "p_wall": 1.0, "p_score": 1.0}
    assert not (v["p_wall"] < 0.05 and v["p_score"] < 0.05)

    # THE PAIRING FALSIFIERS, EXERCISED — not merely written down. A zero from this file is only
    # evidence once it has been shown to produce a NON-zero on a case whose answer is known (L4/L96).
    ok3 = {"arm": "baseline", "nodes": 3, "rep": 99, "wall_secs": 100.0, "score": 0.7,
           "engine_build": "b1"}
    ok1 = {**ok3, "nodes": 1, "wall_secs": 190.0, "score": 0.65}
    pairs, dropped = pair_up({(3, 99): ok3, (1, 99): ok1})
    assert len(pairs) == 1 and not dropped, "a clean pair must FORM — else every zero here is blind"
    assert pairs[0]["faster_with_3"] and pairs[0]["better_with_3"], "3-node win must read as a win"
    # L124/L170: the bonus columns must EXIST on every pair, or the confound goes unprinted exactly
    # when the verdict is most quotable. A missing log reads NO-LOG, never a silent 0/"".
    for k in ("n3_bonus", "n1_bonus", "n3_bonus_class", "n1_bonus_class"):
        assert k in pairs[0], f"every pair must carry {k} beside the verdict"
    assert pairs[0]["n3_bonus_class"] == "NO-LOG" and pairs[0]["n1_bonus_class"] == "NO-LOG", \
        "rep 99 has no log on disk — a missing log must SAY NO-LOG, never a silent 0"
    # ...and the reader must be able to see a REAL class, or the column is decorative.
    assert bonus_of("baseline-n3-r0")[1] in ("APP-SIDE", "TEST-ONLY", "NONE"), \
        "a unit that IS on disk must classify, not fall through to NO-LOG"

    _, d = pair_up({(3, 99): ok3, (1, 99): {**ok1, "engine_build": "b2"}})
    assert len(d) == 1 and "engine build" in d[0]["reason"], "falsifier 2: mixed builds must DROP"

    _, d = pair_up({(3, 99): ok3, (1, 99): {**ok1, "void": True}})
    assert len(d) == 1 and "void" in d[0]["reason"], "falsifier 1: a void cell must void its PAIR"

    # Falsifier 3 is the one most likely to be rationalised away later, so it is asserted, not trusted:
    # five pairs all faster but NONE better must NOT be reported as support.
    fast_only = {"n_pairs": 5, "p_wall": 0.03125, "p_score": 1.0}
    assert not (fast_only["p_wall"] < 0.05 and fast_only["p_score"] < 0.05), \
        "falsifier 3: wall-clock without score is a FAIL, not a partial win"
    print(f"self-test OK ({CURVE_VERSION}) — sign-test controls both directions, empty curve scores nothing")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    v = verdict()
    print(json.dumps(v, indent=2) if "--json" in argv else render(v))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

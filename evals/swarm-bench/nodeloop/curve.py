"""The NODE-RATIO VERDICT, push-button (built BEFORE the data per the act-don't-observe rule).

Pairs baseline n1 rep-i with baseline n3 rep-i on the CURRENT engine and runs the pre-registered
F327 sign test (8 pairs minimum — the first n that absorbs one loss). Two axes, per the goal:
QUALITY (n3 score > n1 score) and SPEED (n3 wall < n1 wall). Exclusions applied exactly as the
ledger rules state: void rows, abandoned/aborted, lms_node_mismatch (the F812 oracle), and — for
the WALL axis only — resumed rows (their prologue skip makes wall incomparable; score stays).

Run any time: partial data reports "k of 8 pairs — verdict pending", never a fabricated call.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import sweep  # noqa: E402


def usable(r: dict, axis: str) -> bool:
    if not r or r.get("score") is None:
        return False
    if r.get("void") or r.get("abandoned") or r.get("aborted"):
        return False
    if r.get("lms_node_mismatch"):
        return False
    # F846: the n1 arm's 8 rows ran on build 1786817447 and are ACCEPTED under the F846 binary
    # by explicit argument (Mihai, Sunday 2026-08-16): the batch's only behavioral change that
    # could touch a 1-node run is the pytest-collision retry — the early-close race cannot
    # engage with one model (fleet_models.len() > 1 gate). Whitelisting the build for n1 rows
    # only; every other row still requires the current binary exactly.
    # ROLLING MODE (Sunday evening): rows stand across the one-run-at-a-time binaries; the
    # scorer version is the comparability rail (sb-5.2 throughout), and every row's build id
    # is recorded in the ledger. The strict single-binary filter returns when the config
    # stabilizes and the formal curve re-runs.
    import os
    if not os.environ.get("BENCH_ROLLING"):
        accepted = {sweep.engine_build()}
        if r.get("nodes") == 1:
            accepted.add("1786817447-236358400")
        if r.get("engine_build") not in accepted:
            return False
    if axis == "wall" and r.get("resumed_from"):
        return False
    return True


def load(nodes: int, rep: int) -> dict | None:
    # F838: the n1 arm runs three-at-a-time via parallel_n1.py with device pins; its rows land
    # under runs/parallel-n1 and take precedence. The sweep path stays the n3 source (and the
    # n1 fallback if the parallel driver was never used for a rep).
    candidates = []
    if nodes == 1:
        candidates.append(HERE.parent / "runs" / "parallel-n1"
                          / f"swarm-1node-r{rep}" / "nodeloop-result.json")
    candidates.append(sweep.result_path("baseline", nodes, rep))
    for p in candidates:
        if p.is_file():
            try:
                return json.loads(p.read_text())
            except Exception:
                continue
    return None


def binom_p_one_sided(k: int, n: int) -> float:
    """P(X >= k) under fair-coin — the pre-registered one-sided sign test."""
    return sum(math.comb(n, i) for i in range(k, n + 1)) / (2 ** n)


def main() -> int:
    pairs_q, pairs_w, rows = [], [], []
    for rep in range(8):
        a, b = load(1, rep), load(3, rep)
        rows.append({
            "rep": rep,
            "n1": (a or {}).get("score"),
            "n3": (b or {}).get("score"),
            "n1_wall": (a or {}).get("wall_secs"),
            "n3_wall": (b or {}).get("wall_secs"),
            "n1_ok": usable(a, "score"), "n3_ok": usable(b, "score"),
        })
        if usable(a, "score") and usable(b, "score"):
            pairs_q.append((a["score"], b["score"]))
        if (usable(a, "wall") and usable(b, "wall")
                and a.get("wall_secs") and b.get("wall_secs")):
            pairs_w.append((a["wall_secs"], b["wall_secs"]))

    print(f"engine: {sweep.engine_build()}")
    print(f"{'rep':>4}{'n1 score':>10}{'n3 score':>10}{'n1 wall':>10}{'n3 wall':>10}")
    for r in rows:
        print(f"{r['rep']:>4}"
              f"{str(r['n1'] if r['n1_ok'] else '—'):>10}"
              f"{str(r['n3'] if r['n3_ok'] else '—'):>10}"
              f"{str(round(r['n1_wall']) if r['n1_wall'] and r['n1_ok'] else '—'):>10}"
              f"{str(round(r['n3_wall']) if r['n3_wall'] and r['n3_ok'] else '—'):>10}")

    out = {"engine": sweep.engine_build(), "rows": rows}
    for name, pairs, better in (("quality", pairs_q, lambda a, b: b > a),
                                ("speed", pairs_w, lambda a, b: b < a)):
        n = len(pairs)
        k = sum(1 for a, b in pairs if better(a, b))
        entry = {"pairs": n, "n3_better": k}
        if n < 8:
            # EARLY CALL (Mihai 2026-08-15: "think of a way to test n1 versus n3 quicker").
            # Zero-assumption sequential stopping: call the verdict early ONLY when the 8-pair
            # outcome is already GUARANTEED whatever the remaining pairs do. p(7,8)=0.0352 and
            # p(6,8)=0.1445 pin the thresholds: 7 wins before 8 pairs guarantees BETTER; 2
            # losses guarantee NOT-ESTABLISHED. No alpha is spent — this is arithmetic, not
            # peeking — so the pre-registered n=8 test is untouched when no bound is hit.
            losses = n - k
            if k >= 7:
                entry["verdict"] = "n3 BETTER (early call — guaranteed at 8)"
                entry["early_call"] = True
                print(f"\n{name}: {k}/{n} favour 3 nodes — EARLY CALL: BETTER is guaranteed "
                      f"regardless of the remaining {8 - n} pair(s)")
            elif losses >= 2:
                entry["verdict"] = "NOT ESTABLISHED (early call — guaranteed at 8)"
                entry["early_call"] = True
                print(f"\n{name}: {k}/{n} favour 3 nodes — EARLY CALL: with {losses} losses, "
                      f"p can never reach 0.05 at 8 pairs; remaining runs answer nothing")
            else:
                entry["verdict"] = f"PENDING — {n} of 8 pairs"
                print(f"\n{name}: {k}/{n} favour 3 nodes — VERDICT PENDING ({n} of 8 pairs)")
        else:
            p = binom_p_one_sided(k, n)
            entry["p_one_sided"] = round(p, 4)
            entry["verdict"] = ("n3 BETTER (p<=0.05)" if p <= 0.05
                                else "NOT ESTABLISHED at 8 pairs")
            print(f"\n{name}: {k}/{n} favour 3 nodes, one-sided p = {p:.4f} -> {entry['verdict']}")
        out[name] = entry

    (HERE.parent / "runs" / "nodeloop" / "curve-verdict.json").write_text(
        json.dumps(out, indent=2))
    print("\nwritten: runs/nodeloop/curve-verdict.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

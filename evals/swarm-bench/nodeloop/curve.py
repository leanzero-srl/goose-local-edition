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
    if r.get("engine_build") != sweep.engine_build():
        return False
    if axis == "wall" and r.get("resumed_from"):
        return False
    return True


def load(nodes: int, rep: int) -> dict | None:
    p = sweep.result_path("baseline", nodes, rep)
    if not p.is_file():
        return None
    try:
        return json.loads(p.read_text())
    except Exception:
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

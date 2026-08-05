#!/usr/bin/env python3
"""F313's registered test, made mechanical. Exit 0.

THE CLAIM (registered in F313, confirmed out-of-sample by F316): a cell whose dynamic-replan bonus
work touched APPLICATION files lands high on tier B; a cell whose bonus work was only TEST files
lands low.

WHY THIS IS A SCRIPT. I classified the bonus work by hand twice and got it wrong the first time —
F312 asserted "all 14 added tasks are test-*" from their NAMES, and `owned_files` showed three of
them own `vendorsync/api.py` and `vendorsync/web/index.html` (L171: a task's name is not its
effect). A rule I re-apply by eye each cell is a rule I will misapply again; four more cells are
coming and each arrives with its class knowable BEFORE its score.

⚠ THE MECHANISM IS FALSIFIED, THE CORRELATION IS NOT. F314 showed the three `index.html` checks are
saturated (3/3 and clean in 7 of 7), so `think_off-n3-r1`'s index.html-only bonus CANNOT be what
lifted its B. This script therefore measures an ASSOCIATION with no working causal story — it is a
lead being tracked honestly, not a finding. It prints that caveat every time so no future reader
mistakes the p for a mechanism.

⚠ THE p IS NOT PRE-REGISTERED. The split was constructed after seeing six cells. The only genuinely
out-of-sample point is `baseline-n3-r2` (F316). Everything else is description.

Usage:
    python3 bonusclass.py              the table, the prediction, the exact p
    python3 bonusclass.py --self-test  controls in both directions
"""
from __future__ import annotations

import json
import sys
from math import comb
from pathlib import Path

import sweep

HERE = Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"
LOW, HIGH = 0.5, 0.9          # the registered thresholds: test-only < LOW, app-side > HIGH


def is_test_file(path: str) -> bool:
    """A test file by PATH, not by task name. `vendorsync/tests/test_x.py` is a test; `api.py` is not."""
    p = path.lower().lstrip("/")
    return "test" in Path(p).name or "/tests/" in f"/{p}"


def classify(owned: list[str] | None) -> str:
    """APP-SIDE if the task owns any non-test source file, else TEST-ONLY. Unknown owns nothing."""
    if not owned:
        return "UNKNOWN"
    return "TEST-ONLY" if all(is_test_file(f) for f in owned) else "APP-SIDE"


def bonus_class(run_log: Path) -> tuple[str, list[tuple[str, str]]]:
    """The cell's class = APP-SIDE if ANY replan-added task owns an app file. Returns the detail too."""
    added, owned_by = [], {}
    for line in run_log.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except Exception:
            continue
        t = e.get("event") or e.get("type")
        if t == "replanned":
            added += e.get("added") or []
        elif t == "task_dispatched":
            owned_by[e.get("task_id")] = e.get("owned_files")
    detail = [(n, classify(owned_by.get(n))) for n in added]
    if not detail:
        return "NONE", detail
    return ("APP-SIDE" if any(c == "APP-SIDE" for _, c in detail) else "TEST-ONLY"), detail


def cells() -> list[dict]:
    """Every stored cell with a tier-B score, joined to its bonus class. Live dirs are skipped."""
    tiers = {}
    for r in sweep.read_results():
        unit = f"{r.get('arm')}-n{r.get('nodes')}-r{r.get('rep')}"
        t = r.get("tiers") or {}
        if t.get("B") is not None and not r.get("void"):
            tiers[unit] = (t["B"]["mean"], r.get("score"))
    out = []
    for unit, (b, score) in tiers.items():
        log = RUNS / unit / "run.jsonl"
        if not log.is_file():
            continue
        cls, detail = bonus_class(log)
        out.append({"unit": unit, "B": b, "score": score, "class": cls, "detail": detail})
    return sorted(out, key=lambda c: -c["B"])


def exact_p(cs: list[dict]) -> float | None:
    """P(all k APP-SIDE cells are the k highest-B, under no association). None if not separated."""
    app = [c for c in cs if c["class"] == "APP-SIDE"]
    k, n = len(app), len(cs)
    if k == 0 or k == n:
        return None
    if not all(c["class"] == "APP-SIDE" for c in cs[:k]):
        return None                 # not a clean top-k split; the combinatorial argument does not apply
    return 1 / comb(n, k)


def report() -> int:
    cs = cells()
    print(f"{'unit':22s} {'B':8s} {'score':8s} {'bonus class':11s} added")
    for c in cs:
        names = ", ".join(f"{n}[{k[0]}]" for n, k in c["detail"]) or "-"
        print(f"{c['unit']:22s} {c['B']:<8} {str(c['score']):8s} {c['class']:11s} {names[:60]}")

    hits, misses = [], []
    for c in cs:
        if c["class"] == "APP-SIDE":
            (hits if c["B"] > HIGH else misses).append(c["unit"])
        elif c["class"] == "TEST-ONLY":
            (hits if c["B"] < LOW else misses).append(c["unit"])
    print(f"\n  registered prediction: TEST-ONLY -> B < {LOW} · APP-SIDE -> B > {HIGH}")
    print(f"  hits {len(hits)}   MISSES {len(misses)}" + (f"  {misses}" if misses else ""))
    p = exact_p(cs)
    print(f"  exact p (app-side cells are the top-k of n): {p:.4f}" if p else
          "  exact p: n/a — the split is no longer a clean top-k")
    print("\n  ⚠ THE MECHANISM IS FALSIFIED (F314: the index.html checks are saturated in 7 of 7),")
    print("    so this is an ASSOCIATION WITH NO CAUSAL STORY. The p is also NOT pre-registered —")
    print("    the split was built after six cells; only baseline-n3-r2 was out-of-sample (F316).")
    print("  ⚠ ANY MISS above falsifies the claim. A miss is the result, not a rounding error.")
    return 0


def self_test() -> int:
    """Both directions, plus the classifier traps that already bit me once."""
    assert classify(["vendorsync/api.py"]) == "APP-SIDE"
    assert classify(["vendorsync/web/index.html"]) == "APP-SIDE"
    assert classify(["tests/test_store.py"]) == "TEST-ONLY"
    assert classify(["vendorsync/tests/test_meridian_edge_cases.py"]) == "TEST-ONLY"
    assert classify(["test_integration.py"]) == "TEST-ONLY"
    assert classify(["/tests/test_sync_idempotency.py"]) == "TEST-ONLY"
    # a task named like a test that owns an app file must read APP-SIDE — the F312 trap (L171)
    assert classify(["vendorsync/api.py", "tests/test_api.py"]) == "APP-SIDE"
    assert classify(None) == "UNKNOWN" and classify([]) == "UNKNOWN"

    mk = lambda u, b, c: {"unit": u, "B": b, "score": 0, "class": c, "detail": []}
    clean = [mk("a", 1.0, "APP-SIDE"), mk("b", 0.97, "APP-SIDE"),
             mk("c", 0.36, "TEST-ONLY"), mk("d", 0.32, "TEST-ONLY"),
             mk("e", 0.21, "TEST-ONLY"), mk("f", 0.21, "TEST-ONLY"), mk("g", 0.31, "TEST-ONLY")]
    assert abs(exact_p(clean) - 1 / comb(7, 2)) < 1e-12, "a clean 2-of-7 top split must give 1/21"
    # AND IT MUST BE ABLE TO SAY NO: one app-side cell out of the top band kills the argument.
    broken = list(clean)
    broken[1] = mk("b", 0.20, "APP-SIDE")
    broken.sort(key=lambda c: -c["B"])
    assert exact_p(broken) is None, "a non-top-k split must NOT produce a p"
    assert exact_p([mk("a", 1.0, "TEST-ONLY")]) is None, "no app-side cell must produce no p"
    print("self-test OK — classifier reads paths not names; p refuses a non-top-k split")
    return 0


def main(argv: list[str]) -> int:
    return self_test() if "--self-test" in argv else report()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

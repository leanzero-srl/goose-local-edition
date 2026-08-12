#!/usr/bin/env python3
"""The ANALYTIC sb-3<->sb-4 bridge (F771: re-exercise is dead — apps bake their vendor URL).

Recomputes the sb-3 total from a recorded sb-4 verdict's per-check rows: pure arithmetic,
no app execution, fully deterministic. The two checks sb-4 removed are reconstructed from
second_sync_cost's recorded detail ("<c>/<n> conditional, <k>/<n> answered 304"):
vendor_conditional = c>0 and k>0; resync_conditional_ratio = k/n. Checks sb-3 never had are
dropped. Usage: bridge_calc.py <sb4-verdict.json> [unit-name]; appends to bridge-ledger.jsonl.
"""
import json, re, sys, time
from pathlib import Path

SB3_TIER_WEIGHT = {"A": 0.25, "B": 0.30, "C": 0.25, "D": 0.20}
SB4_ONLY = {"second_sync_cost", "client_all_payments", "client_total_count", "client_true_order",
            "client_create_replay", "client_idempotency_key", "client_integer_amounts",
            "update_propagation", "restart_persistence", "row_integrity",
            "chronological_order_full", "json_everywhere", "health_semantics"}


def sb3_total(verdict: dict) -> dict:
    rows = {r["check"]: r for r in verdict["checks"]}
    legacy = [dict(r) for r in verdict["checks"] if r["check"] not in SB4_ONLY]
    ssc = rows.get("second_sync_cost", {})
    m = re.search(r"(\d+)/(\d+) conditional, (\d+)/(\d+) answered 304", str(ssc.get("detail", "")))
    if m:
        c, n, k, _n2 = map(int, m.groups())
        legacy.append({"check": "vendor_conditional", "tier": "C",
                       "score": 1.0 if (c > 0 and k > 0) else 0.0})
        legacy.append({"check": "resync_conditional_ratio", "tier": "D",
                       "score": (k / n) if n else 0.0})
    tiers, total = {}, 0.0
    for tier, w in SB3_TIER_WEIGHT.items():
        sub = [r for r in legacy if r["tier"] == tier]
        mean = sum(r["score"] for r in sub) / len(sub) if sub else 0.0
        tiers[tier] = round(mean, 4)
        total += mean * w
    return {"sb3_analytic": round(total, 4), "sb3_tiers": tiers,
            "reconstructed": bool(m), "legacy_checks": len(legacy)}


def main() -> int:
    vpath = Path(sys.argv[1])
    verdict = json.loads(vpath.read_text())
    out = {"unit": sys.argv[2] if len(sys.argv) > 2 else vpath.parent.name,
           "t": round(time.time(), 1),
           "sb4": verdict.get("score"), "sb4_hard": verdict.get("hard"),
           **sb3_total(verdict)}
    ledger = Path(__file__).resolve().parent / "bridge-ledger.jsonl"
    with ledger.open("a") as fh:
        fh.write(json.dumps(out) + "\n")
    print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

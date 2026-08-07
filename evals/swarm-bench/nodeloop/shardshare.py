#!/usr/bin/env python3
"""F455's threshold, read the same way every time.

The claim is that `e2e_oracle` turns the verify-e2e fan into an actual partition. The evidence is
how lopsided the shards are: pre-fix, one shard of four did 28 of 31 tool calls while the other
three made a single call each and reported.

THE THRESHOLD WAS REGISTERED BEFORE THE FIX SHIPPED and is hard-coded here so it cannot be nudged
after seeing a number: the busiest shard must hold UNDER 60% of e2e tool calls. Falsified at 60% or
above. The `e2e_oracle_off` ablation inverts it — that cell must reproduce >=60%.

Why a script rather than a one-off probe: an ad-hoc query rewritten from memory each time is its own
measurement risk. Five separate blind zeros in this campaign came from a probe that looked right and
read the wrong nesting level, and `levers_resolved` is one of them — the levers live under a nested
`levers` key, so a top-level `.get("e2e_oracle")` returns None on every cell ever run and None reads
exactly like OFF.
"""
import json
import os
import sys
import glob

THRESHOLD = 0.60
RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"


def load(cell: str) -> list:
    d = cell if os.path.isdir(cell) else os.path.join(RUNS, cell)
    logs = sorted(glob.glob(os.path.join(d, ".swarm", "run-*.jsonl")), key=os.path.getmtime)
    path = logs[-1] if logs else os.path.join(d, "run.jsonl")
    if not os.path.exists(path):
        raise SystemExit(f"no run log under {d}")
    with open(path) as fh:
        return [json.loads(line) for line in fh if line.strip()]


def build_sha(ev: list) -> str:
    """WHICH CODE THIS CELL ACTUALLY RAN.

    Cell directories are REUSED — the sweep re-runs `baseline-n3-r0` in the same path — so a cell name
    identifies a slot, never a generation. That cost four separate re-checks in one day, and once put a
    retry timeline from one generation beside a session from another before the numbers disagreed
    loudly enough to catch it.

    `levers_resolved` carries `build_sha`, which is the GIT COMMIT, not a binary fingerprint. It is the
    only field that answers "does this cell contain the change I am asking about", so it is printed on
    every readout rather than left to be remembered.
    """
    lv = [e for e in ev if e.get("event") == "levers_resolved"]
    if not lv:
        return "?"
    return str(lv[-1].get("build_sha") or "?")


def oracle_state(ev: list):
    """True / False / None, where None means THE BINARY HAD NO SUCH LEVER — not 'off'."""
    lv = [e for e in ev if e.get("event") == "levers_resolved"]
    if not lv:
        return None, "no levers_resolved event"
    levers = lv[-1].get("levers")
    if not isinstance(levers, dict):
        return None, "levers_resolved carries no nested `levers` dict"
    if "e2e_oracle" not in levers:
        return None, "this binary predates the lever (absent != False)"
    return bool(levers["e2e_oracle"]), "read from the run's own event"


def enumerated_in_prompt(ev: list):
    """THE PRIMARY CHECK, and the one that is valid at n=1.

    The tool-call share below is an OUTCOME, and its pre-fix control distribution is 53.3%, 62.5%,
    81.8% and 90.3% — it straddles the 60% threshold registered for it, so one cell either side
    settles nothing. This is a MECHANISM check instead: with the oracle on, `e2e_shard_spec` emits a
    numbered "THE ADVERTISED SURFACE" table into the shard's description, and `plan_loaded.tasks[]`
    carries that description verbatim. Whether the engine put the table there is a fact about the
    code path, not a sample from a noisy distribution.

    Negative control passed before any positive case existed: both pre-fix cells carry a 1236-char
    description with no table and no endpoint string.
    """
    pl = [e for e in ev if e.get("event") == "plan_loaded"]
    if not pl:
        return None, "no plan_loaded"
    shards = [t for t in (pl[-1].get("tasks") or [])
              if str(t.get("id", "")).startswith("verify-e2e::")]
    if not shards:
        return None, "no verify-e2e:: shards in the plan"
    with_table = [t for t in shards if "THE ADVERTISED SURFACE" in (t.get("description") or "")]
    with_path = [t for t in shards if "/api/" in (t.get("description") or "")]
    detail = (f"{len(with_table)}/{len(shards)} shards carry the enumerated table, "
              f"{len(with_path)}/{len(shards)} name a real endpoint")
    if not with_table:
        return False, detail
    if len(with_table) != len(shards):
        return False, detail + " — a PARTIAL injection is worse than none: the shards that lack it "
    return True, detail


def shard_calls(ev: list) -> dict:
    """Tool calls per DISTINCT shard. A retried shard emits two task_completed rows; summing them is
    correct (it really did that work) but the shard count must stay distinct."""
    per = {}
    for e in ev:
        if e.get("event") != "task_completed":
            continue
        tid = str(e.get("task_id", ""))
        if not tid.startswith("verify-e2e::"):
            continue
        per[tid] = per.get(tid, 0) + len(e.get("tool_calls") or [])
    return per


def report(cell: str) -> int:
    ev = load(cell)
    state, why = oracle_state(ev)
    per = shard_calls(ev)
    print(f"=== {cell}   build_sha {build_sha(ev)} ===")
    print(f"  e2e_oracle = {state}   ({why})")
    injected, idetail = enumerated_in_prompt(ev)
    print(f"  MECHANISM  = {injected}   ({idetail})")
    if state is True and injected is False:
        print("  ⚠ LEVER ON BUT THE TABLE NEVER REACHED THE SHARDS — that is a DEAD lever, and no "
              "share number below can be attributed to the oracle.")
    if not per:
        print("  NO verify-e2e:: shards completed — nothing to measure. Not a pass, not a fail.")
        return 2
    total = sum(per.values())
    for tid in sorted(per):
        print(f"    {tid:<16} {per[tid]:3d} tool calls")
    if total == 0:
        print("  shards ran but made ZERO tool calls — the instrument sees them, the work is absent.")
        return 2
    busiest = max(per.values())
    share = busiest / total
    print(f"  {len(per)} shards, {total} tool calls, busiest holds {share:.1%}")
    if state is True:
        ok = share < THRESHOLD
        print(f"  VERDICT: {'PASS' if ok else 'FAIL'} — oracle ON, threshold is < {THRESHOLD:.0%}")
        return 0 if ok else 1
    if state is False:
        ok = share >= THRESHOLD
        print(f"  VERDICT: {'reproduces the defect' if ok else 'UNEXPECTED'} — oracle OFF, the "
              f"pre-fix/ablation expectation is >= {THRESHOLD:.0%}")
        return 0 if ok else 1
    print("  VERDICT: UNKNOWN — cannot attribute a share to a lever this binary does not have.")
    return 2


if __name__ == "__main__":
    cells = sys.argv[1:] or ["baseline-n1-r0", "baseline-n1-r1", "baseline-n3-r0", "baseline-n3-r1"]
    worst = 0
    for c in cells:
        worst = max(worst, report(c))
    sys.exit(worst)

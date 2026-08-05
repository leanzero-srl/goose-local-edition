#!/usr/bin/env python3
"""What the redraft ladder actually BUYS. Exit 0.

THE CHANNEL. `retarget_discarded` does not merely mark that a redraft happened — it carries the
ENTIRE DISCARDED PLAN: every task's `id`, `desc_chars`, `owned_files`, `deps`. I counted these
events for three ticks and never opened one (the F311/F320 mistake, third time). So a comparison
that costs a full 2-hour run to obtain is sitting in logs already on disk: the plan the engine paid
500-1000 s of prefix to THROW AWAY, next to the plan it kept.

THE QUESTION. F303 measured the redraft's PRICE — the prefix splits cleanly, no-redraft
1091-1330 s, redraft 1731-2839 s. Nobody has measured what it BUYS. If the accepted plan is not
systematically better-shaped than the plan discarded before it, that price is waste, and it is waste
on the wall-clock arm of GOAL ONE.

⚠ THE TWO PLANS USE DIFFERENT KEYS FOR THE SAME FIELD. `plan_loaded.tasks[].files` vs
`retarget_discarded.tasks[].owned_files`. Reading `owned_files` on both — the obvious thing to
write — returns None for every accepted task and silently reports that the accepted plan owns
nothing at all (L154). `owned()` below is the only place either key is read.

⚠ WHAT "BETTER" MEANS HERE, fixed BEFORE the numbers were computed. The engine's own design intent
is `parallel_tests`: one test subtask PER leaf module, depending on ONLY that module, so the tests
run beside the entry-point build instead of after it. A plan with SEPARATE test tasks has that
shape; a plan that folds `tests/test_store.py` into the `store` task destroys it. Root count is the
width available at t=0 — the fleet has 6 slots, so a plan with 4 roots cannot fill them however many
nodes exist.

Usage:
    python3 planshape.py              accepted vs each discarded plan, per run
    python3 planshape.py --self-test  controls in both directions
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

from bonusclass import is_test_file      # L2: the classifier already exists and already has a self-test

HERE = Path(__file__).resolve().parent
RUNS = (HERE.parent / "runs" / "nodeloop").resolve()


def owned(task: dict) -> list[str]:
    """The task's files. `plan_loaded` calls it `files`; `retarget_discarded` calls it `owned_files`."""
    return task.get("owned_files") or task.get("files") or []


def depth(tasks: list[dict]) -> int:
    """Longest dependency chain. A cycle or a dangling dep is reported as the walk's own bound."""
    by_id = {t.get("id"): t for t in tasks}
    memo: dict[str, int] = {}

    def walk(tid: str, seen: frozenset) -> int:
        if tid in memo:
            return memo[tid]
        if tid in seen or tid not in by_id:
            return 0
        d = 1 + max((walk(p, seen | {tid}) for p in (by_id[tid].get("deps") or [])), default=0)
        memo[tid] = d
        return d

    return max((walk(t.get("id"), frozenset()) for t in tasks), default=0)


def shape(tasks: list[dict]) -> dict:
    """Deterministic shape of one plan. No judgement, no model opinion — counts only."""
    app_files, test_files = set(), set()
    sep_test = folded = 0
    for t in tasks:
        fs = owned(t)
        if not fs:
            continue
        tests = [f for f in fs if is_test_file(f)]
        apps = [f for f in fs if not is_test_file(f)]
        app_files.update(apps)
        test_files.update(tests)
        if tests and not apps:
            sep_test += 1
        elif tests and apps:
            folded += 1
    return {
        "n_tasks": len(tasks),
        "roots": sum(1 for t in tasks if not (t.get("deps") or [])),
        "depth": depth(tasks),
        "sep_test_tasks": sep_test,
        "folded_test_tasks": folded,
        "app_files": len(app_files),
        "test_files": len(test_files),
        "desc_chars": sum(t.get("desc_chars") or len(t.get("description") or "") for t in tasks),
    }


def fingerprint(tasks: list[dict]) -> dict[str, tuple]:
    """Each task reduced to what the SCHEDULER acts on: what it owns and what it waits for.

    Deliberately NOT the description text. Two plans differing only in prose dispatch the same work
    to the same devices in the same order — and the redraft's whole justification is that it produces
    a better plan, not a better-worded one.
    """
    return {t.get("id"): (tuple(sorted(owned(t))), tuple(sorted(t.get("deps") or [])))
            for t in tasks}


def diff(a: list[dict], b: list[dict]) -> dict:
    """Task-level change from plan a to plan b."""
    fa, fb = fingerprint(a), fingerprint(b)
    added = sorted(set(fb) - set(fa))
    removed = sorted(set(fa) - set(fb))
    changed = sorted(k for k in set(fa) & set(fb) if fa[k] != fb[k])
    return {"added": added, "removed": removed, "changed": changed,
            "identical": not (added or removed or changed)}


def plans(run_log: Path) -> list[tuple[str, dict, list[dict]]]:
    """Every plan this run produced, in order: each discard, then the accepted one (if it got there)."""
    out = []
    for line in run_log.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except Exception:
            continue
        t = e.get("event") or e.get("type")
        if t == "retarget_discarded":
            ts = e.get("tasks") or []
            out.append((f"discard r{e.get('round')}", shape(ts), ts))
        elif t == "plan_loaded":
            ts = e.get("tasks") or []
            out.append(("ACCEPTED", shape(ts), ts))
    return out


COLS = ["n_tasks", "roots", "depth", "sep_test_tasks", "folded_test_tasks",
        "app_files", "test_files", "desc_chars"]


def report() -> int:
    logs = sorted(RUNS.glob("*/run.jsonl"))
    print(f"files opened: {len(logs)}")                      # L174
    hdr = f"{'run':22s} {'plan':12s} " + " ".join(f"{c[:9]:>9s}" for c in COLS)
    print(hdr)
    verdicts = []
    for p in logs:
        ps = plans(p)
        if len(ps) < 2 or not any(k == "ACCEPTED" for k, _, _ in ps):
            continue                                          # nothing to compare within this run
        for label, s, _ in ps:
            print(f"{p.parent.name:22s} {label:12s} " + " ".join(f"{s[c]:>9}" for c in COLS))
        first_l, first, first_t = next(x for x in ps if x[0].startswith("discard"))
        acc_l, acc, acc_t = next(x for x in ps if x[0] == "ACCEPTED")
        prev_l, _, prev_t = ps[-2]
        d = diff(prev_t, acc_t)
        pct = (abs(acc["desc_chars"] - ps[-2][1]["desc_chars"]) / max(1, ps[-2][1]["desc_chars"])) * 100
        verdicts.append({"run": p.parent.name, "first": first, "accepted": acc,
                         "last_diff": d, "last_label": prev_l, "desc_pct": pct})
        print(f"    {prev_l} -> ACCEPTED: "
              + ("STRUCTURALLY IDENTICAL — same ids, same owned files, same deps"
                 if d["identical"] else
                 f"+{len(d['added'])} -{len(d['removed'])} ~{len(d['changed'])} tasks")
              + f"   prose delta {pct:.2f}%")
        for k in ("added", "removed", "changed"):
            if d[k]:
                print(f"      {k}: {d[k]}")
        print()

    if not verdicts:
        print("  no run yet has BOTH a discarded and an accepted plan — nothing to compare")
        return 0

    print("  REGISTERED BEFORE COMPUTING: if the ladder systematically NARROWS the plan, the accepted")
    print("  plan has FEWER roots and FEWER separate test tasks than the FIRST plan it discarded.")
    print("  ⚠ FALSIFIER: accepted >= first on BOTH counts in 2 or more runs kills the narrowing claim.")
    kills = []
    for v in verdicts:
        dr = v["accepted"]["roots"] - v["first"]["roots"]
        dt = v["accepted"]["sep_test_tasks"] - v["first"]["sep_test_tasks"]
        narrowed = dr < 0 and dt < 0
        if dr >= 0 and dt >= 0:
            kills.append(v["run"])
        print(f"    {v['run']:22s} roots {dr:+d}  sep_test {dt:+d}   "
              f"{'NARROWED' if narrowed else 'widened/mixed'}")
    print(f"  runs where the accepted plan is NOT narrower on either count: {len(kills)} {kills or ''}")
    print(f"  n = {len(verdicts)} runs. A direction at n=3 is a direction, never a magnitude (L10/L133).")
    return 0


def self_test() -> int:
    """Both key spellings, both plan shapes, and the vacuous case that must score nothing."""
    # THE KEY TRAP: the same plan must measure identically under either spelling.
    a = [{"id": "store", "files": ["vendorsync/store.py"], "deps": []}]
    b = [{"id": "store", "owned_files": ["vendorsync/store.py"], "deps": []}]
    assert shape(a) == shape(b), "`files` and `owned_files` must be read as the same field"
    assert shape(a)["app_files"] == 1 and shape(a)["test_files"] == 0

    sep = [{"id": "store", "files": ["vendorsync/store.py"], "deps": []},
           {"id": "test-store", "files": ["tests/test_store.py"], "deps": ["store"]}]
    fold = [{"id": "store", "files": ["vendorsync/store.py", "tests/test_store.py"], "deps": []}]
    assert shape(sep)["sep_test_tasks"] == 1 and shape(sep)["folded_test_tasks"] == 0
    assert shape(fold)["folded_test_tasks"] == 1 and shape(fold)["sep_test_tasks"] == 0, \
        "a task owning BOTH an app file and a test file is FOLDED, never separate"
    # both shapes own the same two files — the metric must distinguish structure, not file count
    assert shape(sep)["app_files"] == shape(fold)["app_files"] == 1
    assert shape(sep)["test_files"] == shape(fold)["test_files"] == 1
    assert shape(sep)["roots"] == 1 and shape(fold)["roots"] == 1

    # The diff must see a real change AND must not invent one. Both directions or it proves nothing.
    p1 = [{"id": "a", "files": ["x.py"], "deps": []}, {"id": "b", "files": ["y.py"], "deps": ["a"]}]
    assert diff(p1, list(reversed(p1)))["identical"], "task ORDER is not a plan change"
    assert diff(p1, [{**p1[0], "description": "totally rewritten prose"}, p1[1]])["identical"], \
        "prose-only edits must read IDENTICAL — the scheduler never sees the wording"
    assert diff(p1, [p1[0]])["removed"] == ["b"], "a dropped task must be seen"
    assert diff(p1, p1 + [{"id": "c", "files": [], "deps": []}])["added"] == ["c"]
    assert diff(p1, [{**p1[0], "deps": ["b"]}, p1[1]])["changed"] == ["a"], "a rewired dep must be seen"
    assert diff(p1, [{**p1[0], "files": ["z.py"]}, p1[1]])["changed"] == ["a"], "reassigned file seen"

    chain = [{"id": "a", "deps": []}, {"id": "b", "deps": ["a"]}, {"id": "c", "deps": ["b"]}]
    assert depth(chain) == 3, "a 3-long chain must read depth 3"
    assert depth([{"id": "a", "deps": []}, {"id": "b", "deps": []}]) == 1, "two roots are depth 1"
    # A cycle must TERMINATE rather than hang or recurse forever — a hang unattended is a dead loop.
    assert depth([{"id": "a", "deps": ["b"]}, {"id": "b", "deps": ["a"]}]) >= 1

    # Vacuous truth: an empty plan must score NOTHING, never full marks (`all([])` is True).
    z = shape([])
    assert all(z[c] == 0 for c in COLS), "an empty plan must score zero on every column"
    print("self-test OK — both key spellings agree, folded != separate, cycles terminate, empty scores 0")
    return 0


def main(argv: list[str]) -> int:
    return self_test() if "--self-test" in argv else report()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

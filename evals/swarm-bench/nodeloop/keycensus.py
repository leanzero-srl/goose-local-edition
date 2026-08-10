#!/usr/bin/env python3
"""Which key does each built app read the vendor's payment page from, and does it predict Tier B?

WHY. F753 reproduced two zero-row syncs offline and found one of them was a single wrong JSON key:
the vendor answers `{"data": [...], "next_cursor", "total"}` and two apps read
`data.get("payments", [])`, whose `.get` default turns a protocol mismatch into a silent empty page.
On five cells that separated Tier B perfectly. Five cells is an anecdote. This asks the same question
of every cell in the archive.

⚠️ THE KEY IS NOT THE ONLY WAY TO SCORE ZERO. `baseline-n3-r1` reads the CORRECT key and still syncs
nothing, because it mishandles the cursor and the endpoint 5xxs. So a cell reading `data` is NOT
predicted to pass — the honest claim is one-directional and the table below is printed in a way that
lets the wrong-key rows and the right-key-still-broken rows be counted separately.

HOW THE KEY IS READ. AST, not grep: only string-literal subscripts (`x["data"]`) and `.get("data")`
calls count. A grep for `data` matches the variable named `data` in almost every one of these files —
that is the F678 decoy in a new costume, and it would report every app as reading the right key.
"""
from __future__ import annotations

import ast
import json
import os
import sys
from collections import defaultdict

assert os.path.basename(os.getcwd()) == "nodeloop", "run this from the nodeloop dir"

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"
CANDIDATES = {"data", "payments", "items", "results", "records", "rows"}
RIGHT = "data"

# Both directions, by name, asserted before any aggregate prints.
CONTROLS = [("baseline-n1-r0", "data"), ("baseline-n3-r0", "payments")]


def keys_read(path: str) -> set[str]:
    """String-literal mapping keys the module reads, restricted to the candidate set."""
    try:
        tree = ast.parse(open(path).read())
    except (OSError, SyntaxError):
        return set()
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Subscript) and isinstance(node.slice, ast.Constant) \
                and isinstance(node.slice.value, str):
            found.add(node.slice.value)
        elif isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) \
                and node.func.attr == "get" and node.args \
                and isinstance(node.args[0], ast.Constant) \
                and isinstance(node.args[0].value, str):
            found.add(node.args[0].value)
    return found & CANDIDATES


def cell(name: str) -> dict | None:
    d = os.path.join(RUNS, name)
    mer = os.path.join(d, "vendorsync", "meridian.py")
    res = os.path.join(d, "nodeloop-result.json")
    if not (os.path.exists(mer) and os.path.exists(res)):
        return None
    r = json.loads(open(res).read())
    t = r.get("tiers")
    if not isinstance(t, dict) or "B" not in t:
        return None
    ks = keys_read(mer)
    return {"cell": name, "nodes": r.get("nodes"), "score": r.get("score"),
            "build": r.get("engine_build"), "void": r.get("void"),
            "B": round(t["B"]["mean"], 4), "keys": ks,
            "right": RIGHT in ks, "wrong_only": bool(ks) and RIGHT not in ks}


def main() -> int:
    rows = {}
    for n in sorted(os.listdir(RUNS)):
        c = cell(n)
        if c:
            rows[n] = c
    if not rows:
        sys.exit("🔴 no readable cells — an empty corpus proves nothing, it does not prove clean")

    for name, want in CONTROLS:
        c = rows.get(name)
        if c is None:
            sys.exit(f"🔴 CONTROL FAILED: {name} unreadable")
        if want not in c["keys"]:
            sys.exit(f"🔴 CONTROL FAILED: {name} reads {sorted(c['keys'])}, expected {want!r}. "
                     "The AST extractor is wrong; nothing below may be believed.")
    if rows[CONTROLS[0][0]]["right"] == rows[CONTROLS[1][0]]["right"]:
        sys.exit("🔴 CONTROL FAILED: the two control cells must differ on `right` — one reads the "
                 "vendor's key and one does not. Identical readings mean the check is blind.")
    print(f"controls: {CONTROLS[0][0]} reads 'data', {CONTROLS[1][0]} reads 'payments' — the "
          "extractor separates them ✅\n")

    by_build = defaultdict(list)
    for c in rows.values():
        if not c["void"]:
            by_build[c["build"]].append(c)

    tot_wrong = tot_wrong_zero = tot_right = tot_right_zero = 0
    for build, rs in sorted(by_build.items(), key=lambda kv: -len(kv[1])):
        print(f"===== build {build}  ({len(rs)} non-void cells with a meridian.py)")
        for c in sorted(rs, key=lambda x: -x["B"]):
            mark = "RIGHT" if c["right"] else ("WRONG" if c["wrong_only"] else "none ")
            print(f"  {c['cell']:26s} n={c['nodes']} B={c['B']:.4f} score={c['score']:.4f} "
                  f"{mark}  keys={sorted(c['keys'])}")
        w = [c for c in rs if c["wrong_only"]]
        r_ = [c for c in rs if c["right"]]
        tot_wrong += len(w)
        tot_wrong_zero += sum(1 for c in w if c["B"] < 0.5)
        tot_right += len(r_)
        tot_right_zero += sum(1 for c in r_ if c["B"] < 0.5)
        print()

    print("===== POOLED ACROSS BUILDS — legitimate here because this is a property of the app's "
          "SOURCE, not a score comparison")
    print(f"  reads only a WRONG key : {tot_wrong:3d} cells, {tot_wrong_zero} with Tier B < 0.5")
    print(f"  reads the RIGHT key    : {tot_right:3d} cells, {tot_right_zero} with Tier B < 0.5")
    print("  A right-key cell with B < 0.5 is a DIFFERENT defect (n3-r1 is the cursor bug) and is "
          "exactly why the claim stays one-directional.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

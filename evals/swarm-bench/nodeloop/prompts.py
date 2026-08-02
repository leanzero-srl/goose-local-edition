#!/usr/bin/env python3
"""How big is each KIND of prompt the fleet actually receives, and how much is inherited junk? Exit 0.

This exists because I got the same number wrong THREE TIMES in three ticks, each time differently:

  F133  published "system message 10,587 chars, 39% of payload" from **n=5, no model filter**.
  F137  corrected it to 22,803 / 81% after discovering `~/.local/state/goose/logs/` is shared with
        EVERY goose session on this machine — including a 1.3 MB Playwright screenshot from an
        `us.anthropic.claude-haiku-4-5` run.
  F138  found 22,803 was ALSO wrong, because it pooled three populations that have nothing to do
        with each other: 341-char scout calls, 12k plan-draft calls, and 20-43k worker calls — AND
        it mixed PRE- and POST-F87 eras, where F87 is the fix that stopped Mihai's personal
        CLAUDE.md (client-config rules, Mac Studio rsync, UI design, "writing as the user") from
        riding along on every worker prompt.

A median over a mixture of populations is a number about nothing. So this instrument refuses to
produce one: it filters to the fleet, SPLITS BY CALL KIND, flags inherited-hint contamination
explicitly, and prints n for every cell. Three hand-rolled queries produced three wrong headlines;
this is the fourth query, written once.

Usage:
    python3 prompts.py                 # all fleet calls in the recent window, split by kind
    python3 prompts.py --since <epoch> # only calls after a timestamp (e.g. a boundary crossing)
"""
from __future__ import annotations

import datetime
import glob
import json
import os
import statistics as st
import sys

LOGS = os.path.expanduser("~/.local/state/goose/logs/llm_request.*.jsonl")

# The ONLY reliable session discriminator in the file. Nothing else says which goose session a call
# belongs to — see F137, where a 1.3 MB screenshot payload from an unrelated session was silently
# pooled into a swarm measurement.
FLEET = "qwen3.6-27b"

# Markers that identify what the call IS.
#
# ⚠ THE FIRST VERSION OF THIS WAS WRONG AND POOLED THE SINK IN WITH THE WORKERS. It keyed on
# "PROJECT FILE LAYOUT" / "a dependency you import" — but `integrate-verify` carries BOTH, because it
# needs the map and the contracts more than anyone. So the "worker" cell mixed 20k-char workers with
# 23k-char sinks, which is the same pooling error F138 was written to stop, one level finer.
#
# The DISCRIMINATOR is ownership, because that is what actually changes the rules a call receives: a
# file-owning worker is told "write NOTHING outside them"; the sink is told "you own no files, you may
# edit ANY file the fix requires". Two opposite instructions, so they must never share a cell.
OWNS = "YOU OWN — write EXACTLY these ABSOLUTE paths"
SINK = ("You own no single file", "You own no file and must WRITE NO file",
        "you may edit ANY file the fix requires")
WORKER = ("PROJECT FILE LAYOUT", "a dependency you import")
# F87's defect signature: Mihai's global CLAUDE.md reaching a 27B that is writing a Python app.
HINTS = ("MANDATORY rules (ALL projects", "Workhorse — Mac Studio sync", "Project Hints")


def sys_text(msg) -> str:
    c = msg.get("content")
    if isinstance(c, list):
        return "".join(x.get("text", "") for x in c if isinstance(x, dict))
    return str(c)


def classify(t: str, n: int) -> str:
    if OWNS in t:
        return "worker"
    if any(m in t for m in SINK):
        return "sink/fix"
    if any(m in t for m in WORKER):
        return "planner/detail"
    if n < 600:
        return "judge/spiral"
    if n < 3000:
        return "scout/small"
    return "planner/detail"


def main(argv: list[str]) -> int:
    since = 0.0
    if "--since" in argv:
        since = float(argv[argv.index("--since") + 1])
    rows = []
    skipped_foreign = 0
    for f in sorted(glob.glob(LOGS), key=os.path.getmtime):
        mt = os.path.getmtime(f)
        if mt < since:
            continue
        try:
            with open(f, errors="replace") as fh:
                e = json.loads(fh.readline())
        except Exception:
            continue
        if FLEET not in ((e.get("model_config") or {}).get("model_name") or ""):
            skipped_foreign += 1
            continue
        inp = e.get("input") or {}
        ms = inp.get("messages") or []
        sysm = next((m for m in ms if isinstance(m, dict) and m.get("role") == "system"), None)
        if not sysm:
            continue
        t = sys_text(sysm)
        rows.append({
            "when": mt,
            "sys": len(t),
            "tools": len(json.dumps(inp.get("tools") or [])),
            "msgs": len(json.dumps(ms)),
            "kind": classify(t, len(t)),
            "hints": any(m in t for m in HINTS),
        })
    if not rows:
        print("no fleet calls in the window — that is a fact about the WINDOW, not about the engine.")
        print(f"({skipped_foreign} calls from other goose sessions were excluded by the model filter.)")
        return 0

    print(f"=== FLEET PROMPT SIZES — {len(rows)} call(s); {skipped_foreign} foreign call(s) excluded ===")
    if since:
        print(f"    window: after {datetime.datetime.fromtimestamp(since)}")
    print()
    print(f"{'kind':<16}{'n':>4}{'sys median':>12}{'sys max':>10}{'tools':>8}{'msgs median':>13}  inherited-hint calls")
    for kind in ("worker", "sink/fix", "planner/detail", "scout/small", "judge/spiral"):
        g = [r for r in rows if r["kind"] == kind]
        if not g:
            continue
        bad = sum(1 for r in g if r["hints"])
        print(f"{kind:<16}{len(g):>4}{st.median([r['sys'] for r in g]):>12,.0f}"
              f"{max(r['sys'] for r in g):>10,}{st.median([r['tools'] for r in g]):>8,.0f}"
              f"{st.median([r['msgs'] for r in g]):>13,.0f}  "
              + (f"{bad}/{len(g)}  <-- F87 CONTAMINATION" if bad else "none"))

    w = [r for r in rows if r["kind"] == "worker"]
    if w:
        clean = [r for r in w if not r["hints"]]
        dirty = [r for r in w if r["hints"]]
        print()
        if dirty:
            newest = max(r["when"] for r in dirty)
            print(f"INHERITED HINTS on {len(dirty)}/{len(w)} worker calls; newest "
                  f"{datetime.datetime.fromtimestamp(newest)}")
            print("  If that timestamp PREDATES the F87 ship, this is history, not a regression —")
            print("  check before raising an alarm. I raised one and was wrong (F138).")
        if clean and dirty:
            print(f"  clean worker system prompt: median {st.median([r['sys'] for r in clean]):,.0f} "
                  f"(n={len(clean)})   contaminated: {st.median([r['sys'] for r in dirty]):,.0f} "
                  f"(n={len(dirty)})")
        base = clean or w
        med = st.median([r["sys"] for r in base])
        tot = st.median([r["sys"] + r["msgs"] for r in base])
        print(f"\nTHE LEVER: a worker system prompt is {med:,.0f} chars (n={len(base)}), "
              f"{100 * med / tot:.0f}% of what that worker reads.")
        print("  Compliance on this model class falls 0.588 at 10 rules to 0.094 at 40, so this is")
        print("  the instruction-density budget — quote THIS number, with its n, and nothing pooled.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

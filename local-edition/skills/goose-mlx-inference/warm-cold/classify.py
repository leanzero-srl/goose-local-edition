"""classify.py <samples.jsonl>... — per file: how many sampled replies open with a FALSE-STATE claim about
the session (a step skipped / interrupted / not gone through / to redo, when the history shows it ran), how
many open with junk (`!`, `[Image`, `[Tool`), and how many go straight to the call. Prints every false-state
hit so the words can be read, not just counted."""
import json
import re
import sys

FALSE_STATE = re.compile(
    r"skipp|redo|re-run|rerun|retry|again|interrupt|didn't go through|wasn't executed|wasn't used|start over"
    r"|already spawned|failed|only one call|only included|stale|All 20 steps|returned nothing|missing"
    r"|one message|cut off|wait for further|isn't available|already ran|I've completed the second"
    r"|Step 2 succeeded|CSV file written|didn't complete",
    re.I,
)
JUNK = re.compile(r"^!|\[Image|\[Tool ", re.I)

for path in sys.argv[1:]:
    rows = [json.loads(line) for line in open(path)]
    pre = [(r["content"] or "").split("<tool_call>")[0].strip() for r in rows]
    junk = [p for p in pre if JUNK.search(p)]
    false_state = [p for p in pre if FALSE_STATE.search(p) and not JUNK.search(p)]
    print(f"{path}: n={len(rows)} false-state {len(false_state)} junk {len(junk)} direct-call {sum(1 for p in pre if not p)}")
    for p in false_state:
        print("    ", repr(p[:110]))

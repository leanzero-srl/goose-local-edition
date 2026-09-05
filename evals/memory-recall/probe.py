#!/usr/bin/env python3
"""The memory-recall instrument: what the REAL store would recall for a set of requests.

Runs the built `goose mcp memory` server over stdio, sends each canned request's words through
search_memories exactly as the recall extension does (function words dropped), and prints the top
hits with their rare-term count and score plus which of them would ride into the turn context
(rare ≥ 1 and score ≥ half the best). Read the WORDS of what it recalls; a slot filled by an entry
that merely shares vocabulary with the request is a finding — file it as a VIGIL-ACTIONS row for the
memory-skills-surgeon.

    python3 evals/memory-recall/probe.py [--goose target/debug/goose] [--queries evals/memory-recall/queries.txt]
"""
import argparse, json, os, re, subprocess, sys

STOPWORDS = set("""a about after again all also an and any are as at be been before but by can could did do does
doing done for from get give had has have he her here him his how i if in into is it its just let like make me
more most my need no not now of on one only or other our out over please same she should so some than that the
their them then there these they this those to too up us use very want was we were what when where which who why
will with would you your""".split())
MIN_SHARE = 0.5   # recall's RECALL_MIN_SHARE_OF_TOP
MAX_MEMORIES = 3  # recall's RECALL_MAX_MEMORIES

def terms(text):
    words = sorted(set(re.split(r"[^0-9a-z]+", text.lower())))
    return [w for w in words if len(w) > 1 and w not in STOPWORDS]

def probe(goose, query, limit=6):
    msgs = [
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}},
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_memories","arguments":{"query":query,"limit":limit}}},
    ]
    stdin = "\n".join(json.dumps(m) for m in msgs) + "\n"
    out = subprocess.run([goose, "mcp", "memory"], input=stdin, capture_output=True, text=True, timeout=60).stdout
    for line in out.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == 2:
            return m["result"]["content"][0]["text"]
    return ""

HIT = re.compile(r"^(\d+)/(\d+) terms \((\d+) rare\)(, exact phrase)?, score ([0-9.]+) — ## (\S+) \((\w+)")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--goose", default="target/debug/goose")
    ap.add_argument("--queries", default=os.path.join(os.path.dirname(__file__), "queries.txt"))
    args = ap.parse_args()
    queries = [q.strip() for q in open(args.queries) if q.strip() and not q.startswith("#")]
    total_slots = 0
    for q in queries:
        t = terms(q)
        text = probe(args.goose, " ".join(t))
        hits = [HIT.match(l) for l in text.splitlines()]
        hits = [h for h in hits if h]
        eligible = [h for h in hits if int(h.group(3)) >= 1]
        top = max((float(h.group(5)) for h in eligible), default=0.0)
        recalled = [h for h in eligible if float(h.group(5)) >= top * MIN_SHARE][:MAX_MEMORIES]
        total_slots += len(recalled)
        print(f"\n### {q}\n    terms: {' '.join(t)}")
        for h in hits:
            mark = "RECALL" if h in recalled else "      "
            print(f"    {mark} {h.group(1)}/{h.group(2)} terms, {h.group(3)} rare, score {float(h.group(5)):5.1f}  {h.group(6)} ({h.group(7)})")
    print(f"\nrecalled slots used: {total_slots} of {len(queries) * MAX_MEMORIES} across {len(queries)} requests")

if __name__ == "__main__":
    main()

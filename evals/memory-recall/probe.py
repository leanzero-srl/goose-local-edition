#!/usr/bin/env python3
"""The memory-recall instrument: what recall would add to the turn context for a set of requests,
computed by `goose recall` — the extension's own functions over the real store and skill catalogue.

Read the WORDS of what it recalls: a slot filled by an entry that merely shares vocabulary with the
request is a finding — file it as a VIGIL-ACTIONS row for the memory-skills-surgeon.

    python3 evals/memory-recall/probe.py [--goose target/debug/goose] [--queries evals/memory-recall/queries.txt]
"""
import argparse, os, re, subprocess

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--goose", default="target/debug/goose")
    ap.add_argument("--queries", default=os.path.join(os.path.dirname(__file__), "queries.txt"))
    args = ap.parse_args()
    queries = [q.strip() for q in open(args.queries) if q.strip() and not q.startswith("#")]
    slots = 0
    skills = 0
    for q in queries:
        out = subprocess.run([args.goose, "recall", q], capture_output=True, text=True, timeout=120).stdout
        head = out.split("\n--- turn context part")[0]
        print(f"\n### {q}\n" + "\n".join("    " + l for l in head.splitlines() if l.strip()))
        m = re.search(r"memories \(\d+ matched, (\d+) would ride along\)", out)
        slots += int(m.group(1)) if m else 0
        m = re.search(r"skills \(\d+ in catalogue, (\d+) suggested\)", out)
        skills += int(m.group(1)) if m else 0
    print(f"\nmemory slots used: {slots} of {len(queries) * 3}; skills suggested: {skills} across {len(queries)} requests")

if __name__ == "__main__":
    main()

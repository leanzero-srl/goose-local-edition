#!/usr/bin/env python3
"""The memory-recall instrument: what recall would add to the turn context for a set of requests,
computed by `goose recall` — the extension's own functions over the real store, skill catalogue and
session history.

Read the WORDS of what it recalls: a slot filled by an entry that merely shares vocabulary with the
request is a finding — file it as a VIGIL-ACTIONS row for the memory-skills-surgeon.

    python3 evals/memory-recall/probe.py [--goose target/debug/goose] [--queries evals/memory-recall/queries.txt] [--cwd DIR]

A query line may carry labels: `request ||| skill-a, skill-b ||| session=<id> past=<id>` — the skills
that SHOULD be suggested, the session to replay the past-session pick in, the past session that
SHOULD be named. With labels, the tail prints skill precision/recall and the past-session tally.
"""
import argparse, os, re, subprocess

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--goose", default="target/debug/goose")
    ap.add_argument("--queries", default=os.path.join(os.path.dirname(__file__), "queries.txt"))
    ap.add_argument("--cwd", default=None, help="working directory recall runs in (the desktop's is $HOME)")
    args = ap.parse_args()
    goose = os.path.abspath(args.goose)
    lines = [q.strip() for q in open(args.queries) if q.strip() and not q.startswith("#")]
    slots = skills = 0
    tp = fp = fn = 0
    past_right = past_wrong = past_missed = 0
    loads = []
    for line in lines:
        parts = [p.strip() for p in line.split("|||")]
        q = parts[0]
        labelled = len(parts) > 1
        expected = {s.strip() for s in parts[1].split(",") if s.strip()} if labelled else set()
        opts = dict(re.findall(r"(\w+)=(\S+)", parts[2])) if len(parts) > 2 else {}
        cmd = [goose, "recall", q] + (["--session", opts["session"]] if "session" in opts else [])
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=120, cwd=args.cwd).stdout
        head = out.split("\n--- turn context part")[0]
        print(f"\n### {q}\n" + "\n".join("    " + l for l in head.splitlines() if l.strip()))
        m = re.search(r"memories \(\d+ matched, (\d+) would ride along\)", out)
        slots += int(m.group(1)) if m else 0
        m = re.search(r"skills \(\d+ in catalogue, (\d+) suggested\)", out)
        n = int(m.group(1)) if m else 0
        skills += n
        section = out.split("\nskills (")[1].split("  candidates:")[0] if "\nskills (" in out else ""
        got = {l.strip() for l in section.splitlines()[1:] if l.startswith("  ") and not l.strip().startswith(("names ", "catalogue frequency"))}
        m = re.search(r"^  names (\S+) —", out, re.M)
        if m:
            loads.append((q[:60], m.group(1)))
        m = re.search(r"past session \(\d+ rows share a word, names (\S+)\)", out)
        past = m.group(1).rstrip(")") if m else None
        if labelled:
            tp += len(got & expected); fp += len(got - expected); fn += len(expected - got)
            wanted = set(opts["past"].split("|")) if "past" in opts else set()
            if past and past in wanted: past_right += 1
            elif past: past_wrong += 1
            if wanted and past not in wanted: past_missed += 1
            want = opts.get("past")
            print(f"    => skills {sorted(got)} expected {sorted(expected)}; past {past} expected {want}")
    print(f"\nmemory slots used: {slots} of {len(lines) * 3}; skills suggested: {skills} across {len(lines)} requests")
    if tp + fp + fn:
        print(f"skills: {tp} right, {fp} wrong, {fn} missed — precision {tp}/{tp+fp}, recall {tp}/{tp+fn}")
        print(f"past sessions: {past_right} right, {past_wrong} wrong, {past_missed} missed")
        print("named (would load without a call when the body fits): " + ("; ".join(f"{s} ← {q}" for q, s in loads) or "none"))

if __name__ == "__main__":
    main()

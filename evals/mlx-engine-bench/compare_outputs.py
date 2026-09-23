#!/usr/bin/env python3
"""Prove two bench runs answered IDENTICALLY: compare_outputs.py <results.json> <results.json> [...]

Every request (workload, rep, turn) of the first run is compared with the same request of each
other run — content, reasoning and tool-call name+arguments, byte for byte. The runs must share a
nonce (the same prompts). One exception is detected, not assumed: a (c) turn's <turn-context>
carries the PREVIOUS turn's measured prompt+completion tokens, so once an earlier turn's
completion length differs the later prompt differs too — such a pair is reported as
"prompt differs (cascade)" and is not a content comparison. (a) differences are printed but do not
fail the check: (a) is not replayable even back-to-back on one mount (bench.py docstring). Exit 1
on any same-prompt (b)/(c) difference.
"""

import json
import sys


def load(path):
    r = json.load(open(path))
    outputs, usage = {}, {}
    for w, runs in r["workloads"].items():
        for rep, run in enumerate(runs):
            for turn, req in enumerate(run):
                if "output" not in req:
                    sys.exit(f"{path}: no recorded outputs (run predates output capture)")
                outputs[(w, rep, turn + 1)] = req["output"]
                usage[(w, rep, turn + 1)] = (req["prompt_tokens"], req["completion_tokens"])
    return r["label"], r["nonce"], outputs, usage


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    ref_label, ref_nonce, ref, ref_usage = load(sys.argv[1])
    failed = False
    for path in sys.argv[2:]:
        label, nonce, other, other_usage = load(path)
        if nonce != ref_nonce:
            sys.exit(f"{label}: nonce {nonce!r} != {ref_nonce!r} — different prompts, nothing to compare")
        keys = sorted(set(ref) | set(other))
        cascade = {k for k in keys if k[0] == "c" and k[2] > 1 and ref_usage.get((k[0], k[1], k[2] - 1)) != other_usage.get((k[0], k[1], k[2] - 1))}
        diffs = [k for k in keys if k not in cascade and ref.get(k) != other.get(k)]
        same = len(keys) - len(cascade) - len(diffs)
        print(f"{ref_label} vs {label}: {same}/{len(keys) - len(cascade)} same-prompt requests identical"
              + (f"; {len(cascade)} prompt differs (cascade) {sorted(cascade)}" if cascade else ""))
        for k in diffs:
            failed = failed or k[0] != "a"
            print(f"  DIFFERS {k}:\n    {ref_label}: {json.dumps(ref.get(k))[:400]}\n    {label}: {json.dumps(other.get(k))[:400]}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

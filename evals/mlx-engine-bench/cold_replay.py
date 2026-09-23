#!/usr/bin/env python3
"""Replay chosen bench requests COLD (prefix cache cleared before each) on the mounted engine.

Usage: cold_replay.py --label <label> --nonce <nonce> --usage-from <results.json> [--seed 7]
                      [--repeat 2] c:2:2 c:2:4 a:0:1 ...

Each target is workload:rep:turn from a bench.py run with the same --nonce, rebuilt byte for byte
by bench.py's own builders. A (c) turn's <turn-context> carries the PREVIOUS turn's measured
prompt+completion tokens, so --usage-from names the run whose earlier turns this replay continues
(its recorded usage is fed back in place of a live call); the body is then byte-identical to the one
that run sent. Before every request POST /v1/cache/clear empties the prefix cache (and
/v1/status must then report 0 entries), so the answer is the engine's output for that prompt with
NO reuse — the reference point for "does the reuse offset change the content". --repeat sends each
target that many times (each cold) to measure within-mount replayability.
"""

import argparse
import json
import os
import sys
import urllib.request
from datetime import datetime

import bench

HERE = os.path.dirname(os.path.abspath(__file__))


class Recorder(Exception):
    pass


def body_for(workload, rep, turn, args, model, fixture, window, usage):
    captured = []

    def capture(base, body):
        captured.append(body)
        if len(captured) == turn:
            raise Recorder()
        prior = usage["workloads"][workload][rep][len(captured) - 1]
        return {"prompt_tokens": prior["prompt_tokens"], "completion_tokens": prior["completion_tokens"]}

    original = bench.measured
    bench.measured = capture
    try:
        if workload == "a":
            bench.workload_a(None, model, args, rep)
        elif workload == "b":
            bench.workload_b(None, model, args, rep)
        else:
            bench.workload_c(None, model, args, rep, fixture, window)
    except Recorder:
        pass
    finally:
        bench.measured = original
    return captured[turn - 1]


def clear(base):
    req = urllib.request.Request(f"{base}/v1/cache/clear", data=b"", method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        r.read()
    entries = json.loads(bench.http_get(f"{base}/v1/status")).get("cache", {}).get("entry_count")
    if entries:
        sys.exit(f"cache clear left {entries} entries — not cold")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--base-url", default="http://127.0.0.1:8090")
    ap.add_argument("--nonce", required=True)
    ap.add_argument("--seed", type=int, default=None)
    ap.add_argument("--temperature", type=float, default=0.0)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--usage-from", required=True)
    ap.add_argument("--transient-tail", action="store_true")
    ap.add_argument("--long-words", type=int, default=28600)
    ap.add_argument("targets", nargs="+")
    args = ap.parse_args()
    args.omit_temperature = False
    base = args.base_url.rstrip("/")
    models = json.loads(bench.http_get(f"{base}/v1/models"))["data"]
    model, window = models[0]["id"], models[0].get("context_window") or 0
    fixture = json.load(open(os.path.join(HERE, "fixtures", "goose-agent-request.json")))
    usage = json.load(open(args.usage_from))
    if usage["nonce"] != args.nonce:
        sys.exit(f"--usage-from nonce {usage['nonce']!r} != {args.nonce!r}")
    out = {"label": args.label, "nonce": args.nonce, "seed": args.seed, "usage_from": args.usage_from, "started": datetime.now().isoformat(),
           "engine": {k: models[0].get(k) for k in ("id", "request_extensions")}, "replays": []}
    for target in args.targets:
        workload, rep, turn = target.split(":")
        body = body_for(workload, int(rep), int(turn), args, model, fixture, window, usage)
        for attempt in range(args.repeat):
            clear(base)
            r = bench.measured(base, body)
            print(f"{target} #{attempt} ttft={r['ttft_s']:.2f}s saved={r['cache'].get('tokens_saved')} "
                  f"out={json.dumps(r['output'])[:160]}", flush=True)
            out["replays"].append({"target": target, "attempt": attempt, **r})
    out_dir = os.path.join(HERE, "results", f"{datetime.now().strftime('%Y-%m-%d')}-{args.label}")
    os.makedirs(out_dir, exist_ok=True)
    json.dump(out, open(os.path.join(out_dir, "results.json"), "w"), indent=1)
    print(f"written: {out_dir}")


if __name__ == "__main__":
    main()

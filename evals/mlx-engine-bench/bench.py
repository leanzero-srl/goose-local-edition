#!/usr/bin/env python3
"""MLX engine baseline harness — talks straight to the mounted Rapid-MLX engine.

Usage:
  bench.py --label <label> [--base-url http://127.0.0.1:8090] [--reps 3]
           [--workloads a,b,c] [--nonce <text>] [--temperature 0 | --omit-temperature]

Three fixed workloads, each repeated --reps times:
  a  short chat   ~200-token prompt  -> max 300 tokens out
  b  long prompt  ~32k-token fixed document -> max 200 tokens out (prefill + TTFT)
  c  goose agent  the real goose system prompt + 79 tool schemas (fixtures/, sanitized), a
                  4-turn tool conversation: each turn appends a FIXED assistant tool call and a
                  FIXED tool result, and moves a CHANGING <turn-context> to the request tail the
                  way goose does (moim.rs compose_moim + openai.rs append_turn_context_tail), so
                  the prefix-cache reuse across tool calls is what gets measured.

Every request runs alone (the harness is the only client) and is bracketed by /v1/status and
/metrics snapshots; the deltas are the engine's own counters for that one request.

Why every repetition carries a NONCE as the first line of its prompt: an identical prompt would
be answered from the prefix cache (and, greedy, possibly the response cache), and the cache is
persisted across remounts — so a repeated prompt would measure the cache of a PREVIOUS run, not
the configuration under test. The nonce makes every repetition cold at token ~0; workload (c)
then measures only the reuse INSIDE its own conversation. Pass --nonce explicitly to replay a
conversation on purpose (the unmount -> mount persistence check).

Why temperature 0 and NO seed: greedy decoding makes the output (and so the completion length)
repeatable; a seed would pin Rapid-MLX's MTP depth to max_k and switch off the EV auto-K
controller production runs (scheduler.py:2133-2138 `disable_auto_k=... lane_rng is not None`).
goose itself sends no temperature (the engine's default sampler applies) — --omit-temperature
reproduces that for a production-shaped check.
"""

import argparse
import json
import os
import random
import re
import statistics
import sys
import time
import urllib.request
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- engine I/O


def http_get(url: str) -> str:
    with urllib.request.urlopen(url, timeout=30) as r:
        return r.read().decode()


def snapshot(base: str) -> dict:
    status = json.loads(http_get(f"{base}/v1/status"))
    metrics = {}
    for line in http_get(f"{base}/metrics").splitlines():
        if not line or line.startswith("#"):
            continue
        m = re.match(r"^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{[^}]*\})?\s+([-+0-9.eEinfNa]+)$", line)
        if not m:
            continue
        name, labels, value = m.group(1), m.group(2) or "", m.group(3)
        if not (name.startswith("rapid_mlx_spec_decode") or name.startswith("rapid_mlx_prefix_cache")):
            continue
        labels = re.sub(r',?model_id="[^"]*"', "", labels)
        try:
            metrics[name + labels] = float(value)
        except ValueError:
            pass
    return {"status": status, "metrics": metrics}


CACHE_KEYS = ["hits", "misses", "tokens_saved", "evictions", "non_trimmable_skips", "load_skipped"]


def deltas(before: dict, after: dict) -> dict:
    cb, ca = before["status"].get("cache", {}), after["status"].get("cache", {})
    cache = {k: ca.get(k, 0) - cb.get(k, 0) for k in CACHE_KEYS if k in ca}
    cache["entry_count_after"] = ca.get("entry_count")
    cache["current_memory_mb_after"] = ca.get("current_memory_mb")
    spec = {}
    for k, v in after["metrics"].items():
        if not k.startswith("rapid_mlx_spec_decode") or "accept_ratio" in k or "k_cost_ms" in k:
            continue
        d = v - before["metrics"].get(k, 0.0)
        if d:
            spec[k.replace("rapid_mlx_spec_decode_", "").replace('family="qwen3.8",', "")] = d
    return {"cache": cache, "spec_decode": spec}


def stream_chat(base: str, body: dict) -> dict:
    body = dict(body, stream=True, stream_options={"include_usage": True})
    req = urllib.request.Request(
        f"{base}/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json"},
    )
    t0 = time.perf_counter()
    t_first = t_last = None
    usage, finish, req_metrics = None, None, None
    content_chars = reasoning_chars = 0
    tool_calls = []
    with urllib.request.urlopen(req, timeout=None) as r:
        for raw in r:
            line = raw.decode().strip()
            if not line.startswith("data:"):
                continue
            payload = line[5:].strip()
            if payload == "[DONE]":
                break
            chunk = json.loads(payload)
            if chunk.get("usage"):
                usage = chunk["usage"]
            for ch in chunk.get("choices", []):
                delta = ch.get("delta", {})
                produced = False
                if delta.get("content"):
                    content_chars += len(delta["content"])
                    produced = True
                for key in ("reasoning_content", "reasoning"):
                    if delta.get(key):
                        reasoning_chars += len(delta[key])
                        produced = True
                if delta.get("tool_calls"):
                    tool_calls.extend(delta["tool_calls"])
                    produced = True
                if produced:
                    now = time.perf_counter()
                    if t_first is None:
                        t_first = now
                    t_last = now
                if ch.get("finish_reason"):
                    finish = ch["finish_reason"]
            if chunk.get("metrics"):
                req_metrics = chunk["metrics"]
    t_end = time.perf_counter()
    if usage is None:
        raise RuntimeError(f"engine sent no usage chunk (finish={finish})")
    if t_first is None:
        raise RuntimeError(f"engine produced no output delta (finish={finish}, usage={usage})")
    completion = usage["completion_tokens"]
    decode_span = (t_last - t_first) if t_last and t_last > t_first else None
    return {
        "ttft_s": t_first - t0,
        "total_s": t_end - t0,
        "prompt_tokens": usage["prompt_tokens"],
        "completion_tokens": completion,
        "decode_tps": ((completion - 1) / decode_span) if decode_span and completion > 1 else None,
        "finish_reason": finish,
        "content_chars": content_chars,
        "reasoning_chars": reasoning_chars,
        "tool_call_deltas": len(tool_calls),
        "request_metrics": req_metrics,
    }


def measured(base: str, body: dict) -> dict:
    before = snapshot(base)
    result = stream_chat(base, body)
    after = snapshot(base)
    result.update(deltas(before, after))
    saved = result["cache"].get("tokens_saved", 0)
    uncached = result["prompt_tokens"] - saved
    result["uncached_prompt_tokens"] = uncached
    result["prefill_tps"] = uncached / result["ttft_s"] if result["ttft_s"] > 0 else None
    return result


# ---------------------------------------------------------------- fixed texts

WORDS = (
    "system memory engine model token cache request worker queue latency throughput "
    "prefill decode kernel buffer tensor layer attention state checkpoint budget window "
    "the a of to and in for on with by from at as into over under between across through "
    "reads writes measures holds returns stores computes loads keeps moves drops shares "
    "quickly slowly carefully fully partly rarely often always never exactly roughly "
    "server client network disk file record entry index table column value field schema "
    "first second third final early late fresh stale warm cold large small long short "
    "user operator developer reviewer tester maintainer owner service scheduler supervisor"
).split()


def document(n_words: int, seed: int) -> str:
    rng = random.Random(seed)
    sentences, count = [], 0
    while count < n_words:
        k = rng.randint(8, 18)
        words = [rng.choice(WORDS) for _ in range(k)]
        words[0] = words[0].capitalize()
        sentences.append(" ".join(words) + ".")
        count += k
        if rng.random() < 0.12:
            sentences.append("\n\n")
    return " ".join(sentences)


SHORT_PROMPT = (
    "You are reviewing the design of a small inference server that runs one language model on a "
    "single workstation. The server keeps a prefix cache of previously processed prompts, admits "
    "at most eight concurrent requests, and speculatively drafts a few tokens per step with a "
    "lightweight head that the full model then verifies. Operators complain that the first token "
    "of a long request can take many seconds, while short chats feel fast. Explain, in plain "
    "prose and without lists, where the time goes for a long prompt versus a short one, what the "
    "prefix cache can and cannot save when the end of every prompt changes, and which two "
    "measurements an operator should take before changing any setting. Write at least four "
    "paragraphs. Keep each paragraph concrete: name the component, the cost, and the reason, and "
    "say what you would expect to see on a machine with ample memory but a single GPU."
)

TURN_TOOL_CALLS = [
    ("tree", {"path": "/Users/operator/project/src", "depth": 2}),
    ("shell", {"command": "rg -n 'fn handle_request' /Users/operator/project/src"}),
    ("shell", {"command": "sed -n '1,160p' /Users/operator/project/src/server/handler.rs"}),
]


def tool_result(turn: int) -> str:
    rng = random.Random(1000 + turn)
    lines = []
    for i in range(140):
        indent = "    " * rng.randint(0, 3)
        words = " ".join(rng.choice(WORDS) for _ in range(rng.randint(3, 9)))
        lines.append(f"{i + 1:4d}  {indent}{words.replace(' ', '_', 1)}({rng.randint(0, 99)});")
    return "\n".join(lines)


def turn_context(turn: int, context_used: int | None, window: int) -> str:
    minute = 7 + 2 * turn
    ctx = (
        "context: no call completed yet"
        if context_used is None
        else f"context: {context_used:,} of {window:,} tokens used ({round(100 * context_used / window)}%)"
    )
    return "\n".join(
        [
            "<turn-context>",
            f"<current-time>2026-09-23 14:{minute:02d}:00</current-time>",
            "<working-directory>/Users/operator/project</working-directory>",
            f"<context>{ctx}</context>",
            "",
            "<ledger>\nThe project ledger (.goose/ledger.md) is empty — ledger_append the first finding, decision or dead end you meet.\n</ledger>",
            "</turn-context>",
        ]
    )


# ---------------------------------------------------------------- workloads


def sampling(args) -> dict:
    return {} if args.omit_temperature else {"temperature": args.temperature}


def workload_a(base, model, args, rep):
    msg = f"Session {args.nonce}-a{rep}.\n{SHORT_PROMPT}"
    return [measured(base, {"model": model, "messages": [{"role": "user", "content": msg}], "max_tokens": 300, **sampling(args)})]


def workload_b(base, model, args, rep):
    doc = document(args.long_words, seed=42)
    msg = (
        f"Session {args.nonce}-b{rep}.\nBelow is an operations log. Read it, then answer the question after it.\n\n"
        f"{doc}\n\nQuestion: in about 150 words of plain prose, which components does the log mention most, and what does it say about them?"
    )
    return [measured(base, {"model": model, "messages": [{"role": "user", "content": msg}], "max_tokens": 200, **sampling(args)})]


def workload_c(base, model, args, rep, fixture, window):
    system = f"Session {args.nonce}-c{rep}.\n" + fixture["system_prompt"]
    task = (
        "Find where the server handles an incoming request, read the handler, and tell me in two "
        "sentences where a request could wait before its first token."
    )
    history = [{"role": "system", "content": system}, {"role": "user", "content": task}]
    turns, context_used = [], None
    for turn in range(4):
        tail = turn_context(turn, context_used, window)
        messages = [dict(m) for m in history]
        if messages[-1]["role"] == "user":
            messages[-1]["content"] = messages[-1]["content"] + "\n" + tail
        else:
            messages.append({"role": "user", "content": tail})
        body = {"model": model, "messages": messages, "tools": fixture["tools"], "max_tokens": 200, **sampling(args)}
        r = measured(base, body)
        r["turn"] = turn + 1
        turns.append(r)
        context_used = r["prompt_tokens"] + r["completion_tokens"]
        if turn < len(TURN_TOOL_CALLS):
            name, arguments = TURN_TOOL_CALLS[turn]
            call_id = f"call_{turn + 1}"
            history.append(
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{"id": call_id, "type": "function", "function": {"name": name, "arguments": json.dumps(arguments)}}],
                }
            )
            history.append({"role": "tool", "tool_call_id": call_id, "content": tool_result(turn)})
    return turns


# ---------------------------------------------------------------- reporting


def summarize(values):
    vals = [v for v in values if v is not None]
    if not vals:
        return None
    return {"median": statistics.median(vals), "min": min(vals), "max": max(vals), "n": len(vals)}


def fmt(s, digits=2):
    if s is None:
        return "n/a"
    return f"{s['median']:.{digits}f} ({s['min']:.{digits}f}–{s['max']:.{digits}f})"


def spec_accept(runs):
    drafted = accepted = verify = 0.0
    for r in runs:
        rm = (r.get("request_metrics") or {}).get("speculative_decoding") or {}
        drafted += sum(rm.get("drafted_by_depth") or [])
        accepted += sum(rm.get("accepted_by_depth") or [])
        verify += rm.get("verify_calls") or 0
    return drafted, accepted, verify


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--base-url", default="http://127.0.0.1:8090")
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--workloads", default="a,b,c")
    ap.add_argument("--nonce", default=None)
    ap.add_argument("--temperature", type=float, default=0.0)
    ap.add_argument("--omit-temperature", action="store_true")
    ap.add_argument("--long-words", type=int, default=28600, help="document size for (b); ~32k tokens at the default (measured 24,000 words -> 26,838 tokens)")
    ap.add_argument("--rep-start", type=int, default=0, help="first repetition index (replays a specific nonce-rep)")
    args = ap.parse_args()
    base = args.base_url.rstrip("/")
    args.nonce = args.nonce or f"{args.label}-{datetime.now().strftime('%Y%m%dT%H%M%S')}"

    models = json.loads(http_get(f"{base}/v1/models"))["data"]
    model, window = models[0]["id"], models[0].get("context_window") or 0
    engine = {k: models[0].get(k) for k in ("id", "context_window", "is_hybrid", "tool_call_parser", "reasoning_parser", "speculative_decoding")}
    status0 = json.loads(http_get(f"{base}/v1/status"))
    if status0.get("num_running") or status0.get("num_waiting"):
        sys.exit(f"refusing: engine is busy ({status0.get('num_running')} running, {status0.get('num_waiting')} waiting)")
    fixture = json.load(open(os.path.join(HERE, "fixtures", "goose-agent-request.json")))

    warm = stream_chat(base, {"model": model, "messages": [{"role": "user", "content": f"Warmup {args.nonce}: reply OK."}], "max_tokens": 8, **sampling(args)})

    results = {"label": args.label, "nonce": args.nonce, "started": datetime.now().isoformat(), "engine": engine,
               "sampling": sampling(args) or "engine default (omitted)", "reps": args.reps, "warmup": warm, "workloads": {}}
    for w in args.workloads.split(","):
        runs = []
        for rep in range(args.rep_start, args.rep_start + args.reps):
            if w == "a":
                out = workload_a(base, model, args, rep)
            elif w == "b":
                out = workload_b(base, model, args, rep)
            elif w == "c":
                out = workload_c(base, model, args, rep, fixture, window)
            else:
                sys.exit(f"unknown workload {w}")
            runs.append(out)
            last = out[-1]
            print(f"[{w} rep{rep}] ttft={last['ttft_s']:.2f}s prompt={last['prompt_tokens']} out={last['completion_tokens']} "
                  f"decode={last['decode_tps'] or 0:.1f}t/s saved={last['cache'].get('tokens_saved')}", flush=True)
        results["workloads"][w] = runs
    results["finished"] = datetime.now().isoformat()
    results["status_after"] = json.loads(http_get(f"{base}/v1/status"))

    out_dir = os.path.join(HERE, "results", f"{datetime.now().strftime('%Y-%m-%d')}-{args.label}")
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "results.json"), "w") as f:
        json.dump(results, f, indent=1)

    md = [f"# {args.label}", "", f"engine `{model}` · sampling {results['sampling']} · reps {args.reps} · nonce `{args.nonce}`",
          "", "median (min–max) over reps", "",
          "| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |",
          "|---|---|---|---|---|---|---|---|"]
    for w, runs in results["workloads"].items():
        if w == "c":
            for turn in range(len(runs[0])):
                rs = [run[turn] for run in runs]
                d, a, v = spec_accept(rs)
                md.append(f"| c turn {turn + 1} | {fmt(summarize([r['ttft_s'] for r in rs]))} | {fmt(summarize([r['prefill_tps'] for r in rs]), 0)} | "
                          f"{fmt(summarize([r['decode_tps'] for r in rs]), 1)} | {fmt(summarize([r['prompt_tokens'] for r in rs]), 0)} | "
                          f"{fmt(summarize([r['completion_tokens'] for r in rs]), 0)} | {fmt(summarize([r['cache'].get('tokens_saved') for r in rs]), 0)} | "
                          f"{a:.0f}/{d:.0f} ({v:.0f}) |")
        else:
            rs = [run[0] for run in runs]
            d, a, v = spec_accept(rs)
            md.append(f"| {w} | {fmt(summarize([r['ttft_s'] for r in rs]))} | {fmt(summarize([r['prefill_tps'] for r in rs]), 0)} | "
                      f"{fmt(summarize([r['decode_tps'] for r in rs]), 1)} | {fmt(summarize([r['prompt_tokens'] for r in rs]), 0)} | "
                      f"{fmt(summarize([r['completion_tokens'] for r in rs]), 0)} | {fmt(summarize([r['cache'].get('tokens_saved') for r in rs]), 0)} | "
                      f"{a:.0f}/{d:.0f} ({v:.0f}) |")
    with open(os.path.join(out_dir, "summary.md"), "w") as f:
        f.write("\n".join(md) + "\n")
    print("\n".join(md))
    print(f"\nwritten: {out_dir}")


if __name__ == "__main__":
    main()

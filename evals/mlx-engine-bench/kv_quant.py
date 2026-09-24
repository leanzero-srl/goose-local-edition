#!/usr/bin/env python3
"""KV-cache compression measurement — one engine configuration per invocation.

Usage:
  kv_quant.py --label bf16|int8|int4[-suffix] --base-url http://127.0.0.1:8093 [--phases quality,memory]

Run it once per `--kv-cache-dtype` on an engine YOU started (never the owner's :8090), then
`kv_quant_compare.py` joins the runs. Two phases:

quality  greedy (temperature 0, thinking off) with `logprobs` + `top_logprobs=5`, NON-streaming, so
         every answer carries its token sequence and the runner-up at every position. The prompts
         are real: the bench's short chat (a), six short tasks from the owner's domain, the bench's
         goose agent conversation (c, 4 turns, 42k–49k-token prompts with 79 tool schemas), the
         bench's 32k document (b), and a retrieval check — a fact buried ~31k tokens into a ~40k
         prompt. Logprob requests leave the MTP fast path (the engine computes them on plain
         decode), so this phase measures QUALITY only, never speed.
memory   streaming, no logprobs (MTP active, production-shaped): one prompt each at ~8k, ~32k and
         ~128k tokens, the 128k one carrying two buried facts (~35k and ~110k deep). While each
         request runs, /v1/status is polled for Metal `active_memory_gb`; the record keeps the
         active memory before the request, its maximum during prefill, and its value during decode
         (after the first token). The KV bytes/token is the slope of the decode-phase delta between
         contexts, which cancels the weights and the fixed recurrent state.

Every prompt carries the run's NONCE on its first line so nothing is answered from a prefix cache
another configuration (or the owner's engine) filled — start the engine with
RAPID_MLX_PREFIX_CACHE_AUTOLOAD=0 as well, so its persisted cache is neither read nor overwritten.
"""

import argparse
import json
import os
import sys
import threading
import time
import urllib.request
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from bench import SHORT_PROMPT, TURN_TOOL_CALLS, document, tool_result, turn_context  # noqa: E402

NO_THINKING = {"chat_template_kwargs": {"enable_thinking": False}}

SHORT_TASKS = [
    (
        "jql",
        "Write one JQL query that finds every Bug in project PAY created in the last 14 days that is "
        "still unassigned, highest priority first. Then explain each clause of the query in one sentence.",
    ),
    (
        "confluence",
        "Draft the outline of a Confluence page for a production incident postmortem: the section "
        "headings, and one sentence under each saying what belongs there.",
    ),
    (
        "python",
        "Write a Python function `parse_duration(text: str) -> int` that converts strings like "
        "'1h30m', '45s' or '2h' into a number of seconds and raises ValueError on anything else. "
        "Include three doctest examples.",
    ),
    (
        "rust",
        "In Rust, when would you choose Arc<RwLock<T>> over Arc<Mutex<T>>, and when is the Mutex the "
        "better choice even for read-heavy data? Answer in two short paragraphs.",
    ),
    (
        "arithmetic",
        "A build farm has 3 machines. Machine A finishes a job in 12 minutes, B in 18 minutes, C in 36 "
        "minutes. Working together on identical jobs, how many jobs do they finish in 3 hours? Show "
        "the calculation step by step.",
    ),
    (
        "jira-admin",
        "A Jira admin wants a workflow where an issue can only move to Done after a reviewer other "
        "than the assignee approves it. Describe the workflow statuses, transitions and the "
        "condition or validator you would configure.",
    ),
]

NEEDLE_HERON = "The vault access code for Project Heron is KESTREL-4471."
NEEDLE_DELTA = "The backup rotation owner for cluster Delta is Ioana Marinescu."
# measured (bench.py --long-words help): 24,000 words -> 26,838 tokens, i.e. 1.118 tokens per word
# for this vocabulary; the word counts below are the token targets divided by it.
TOKENS_PER_WORD = 26838 / 24000


def words_for(tokens: int) -> int:
    return int(tokens / TOKENS_PER_WORD)


def buried(total_words: int, needles: list[tuple[int, str]], seed: int) -> str:
    """A document of `total_words` with each needle inserted as its own paragraph at a word depth."""
    doc = document(total_words, seed=seed).split(" ")
    for depth_words, text in sorted(needles, reverse=True):
        doc.insert(depth_words, f"\n\n{text}\n\n")
    return " ".join(doc)


# ---------------------------------------------------------------- engine I/O


def http_json(url: str, body: dict | None = None, timeout: float | None = 30) -> dict:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def status(base: str) -> dict:
    return http_json(f"{base}/v1/status")


def chat_logprobs(base: str, model: str, messages: list, tools=None, max_tokens=256) -> dict:
    body = {
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "logprobs": True,
        "top_logprobs": 5,
        **NO_THINKING,
    }
    if tools:
        body["tools"] = tools
    t0 = time.perf_counter()
    resp = http_json(f"{base}/v1/chat/completions", body, timeout=None)
    elapsed = time.perf_counter() - t0
    choice = resp["choices"][0]
    content = (choice.get("logprobs") or {}).get("content") or []
    msg = choice.get("message") or {}
    return {
        "elapsed_s": elapsed,
        "usage": resp.get("usage"),
        "finish_reason": choice.get("finish_reason"),
        "content": msg.get("content"),
        "tool_calls": [
            {"name": (c.get("function") or {}).get("name"), "arguments": (c.get("function") or {}).get("arguments")}
            for c in (msg.get("tool_calls") or [])
        ],
        "tokens": [
            {
                "token": t.get("token"),
                "logprob": t.get("logprob"),
                "top": [[a.get("token"), a.get("logprob")] for a in (t.get("top_logprobs") or [])],
            }
            for t in content
        ],
    }


class MemoryPoller:
    """Polls /v1/status while a request runs; phase flips to 'decode' at the first token."""

    def __init__(self, base: str, period: float):
        self.base, self.period = base, period
        self.phase = "prefill"
        self.samples: list[tuple[float, str, float]] = []
        self.errors = 0
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        t0 = time.perf_counter()
        while not self._stop.is_set():
            try:
                m = status(self.base).get("metal") or {}
                self.samples.append((time.perf_counter() - t0, self.phase, float(m["active_memory_gb"])))
            except Exception:
                self.errors += 1
            self._stop.wait(self.period)

    def __enter__(self):
        self._thread.start()
        return self

    def __exit__(self, *exc):
        self._stop.set()
        self._thread.join()


def stream_measured(base: str, model: str, content: str, max_tokens: int, poll_s: float) -> dict:
    before = status(base)
    body = {
        "model": model,
        "messages": [{"role": "user", "content": content}],
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "stream": True,
        "stream_options": {"include_usage": True},
        **NO_THINKING,
    }
    req = urllib.request.Request(
        f"{base}/v1/chat/completions", data=json.dumps(body).encode(), headers={"content-type": "application/json"}
    )
    parts, usage, t_first, t_last = [], None, None, None
    with MemoryPoller(base, poll_s) as poller:
        t0 = time.perf_counter()
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
                    text = delta.get("content") or delta.get("reasoning_content") or delta.get("reasoning")
                    if text:
                        now = time.perf_counter()
                        if t_first is None:
                            t_first = now
                            poller.phase = "decode"
                        t_last = now
                        parts.append(text)
        t_end = time.perf_counter()
    after = status(base)
    if usage is None or t_first is None:
        raise RuntimeError(f"engine sent no usage/output (usage={usage})")
    completion = usage["completion_tokens"]
    span = t_last - t_first if t_last and t_last > t_first else None
    pre = [s for s in poller.samples if s[1] == "prefill"]
    dec = [s for s in poller.samples if s[1] == "decode"]
    base_active = float(before["metal"]["active_memory_gb"])
    return {
        "prompt_tokens": usage["prompt_tokens"],
        "completion_tokens": completion,
        "ttft_s": t_first - t0,
        "total_s": t_end - t0,
        "prefill_tps": usage["prompt_tokens"] / (t_first - t0),
        "decode_tps": (completion - 1) / span if span and completion > 1 else None,
        "active_before_gb": base_active,
        "active_prefill_max_gb": max((s[2] for s in pre), default=None),
        "active_decode_max_gb": max((s[2] for s in dec), default=None),
        "active_after_gb": float(after["metal"]["active_memory_gb"]),
        "process_peak_after_gb": float(after["metal"]["peak_memory_gb"]),
        "samples": len(poller.samples),
        "sample_errors": poller.errors,
        "cache_before": before.get("cache"),
        "cache_after": after.get("cache"),
        "spec_decode_after": (after.get("mtp_prompt_lookup") or {}).get("vendored_steps"),
        "content": "".join(parts),
    }


# ---------------------------------------------------------------- phases


def quality(base: str, model: str, nonce: str, fixture: dict) -> dict:
    out = {}

    def ask(key, messages, tools=None, max_tokens=256):
        r = chat_logprobs(base, model, messages, tools=tools, max_tokens=max_tokens)
        out[key] = r
        u = r["usage"] or {}
        print(
            f"[quality {key}] prompt={u.get('prompt_tokens')} out={u.get('completion_tokens')} "
            f"tokens-with-logprobs={len(r['tokens'])} {r['elapsed_s']:.1f}s",
            flush=True,
        )

    ask("a-short-chat", [{"role": "user", "content": f"Session {nonce}-a.\n{SHORT_PROMPT}"}])
    for key, text in SHORT_TASKS:
        ask(key, [{"role": "user", "content": f"Session {nonce}-{key}.\n{text}"}])

    # The bench's goose agent conversation (workload c): fixed tool calls and results, the
    # <turn-context> tail moving each turn the way goose sends it.
    system = f"Session {nonce}-c.\n" + fixture["system_prompt"]
    task = (
        "Find where the server handles an incoming request, read the handler, and tell me in two "
        "sentences where a request could wait before its first token."
    )
    history = [{"role": "system", "content": system}, {"role": "user", "content": task}]
    context_used = None
    for turn in range(4):
        tail = turn_context(turn, context_used, 262144)
        messages = [dict(m) for m in history]
        if messages[-1]["role"] == "user":
            messages[-1]["content"] = messages[-1]["content"] + "\n" + tail
        else:
            messages.append({"role": "user", "content": tail})
        ask(f"c-agent-turn{turn + 1}", messages, tools=fixture["tools"], max_tokens=200)
        u = out[f"c-agent-turn{turn + 1}"]["usage"] or {}
        context_used = (u.get("prompt_tokens") or 0) + (u.get("completion_tokens") or 0)
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

    doc = document(28600, seed=42)
    ask(
        "b-long-32k",
        [
            {
                "role": "user",
                "content": f"Session {nonce}-b.\nBelow is an operations log. Read it, then answer the question after it.\n\n"
                f"{doc}\n\nQuestion: in about 150 words of plain prose, which components does the log mention most, and what does it say about them?",
            }
        ],
        max_tokens=200,
    )

    haystack = buried(words_for(40000), [(words_for(31000), NEEDLE_HERON)], seed=7)
    ask(
        "retrieval-31k",
        [
            {
                "role": "user",
                "content": f"Session {nonce}-r.\nBelow is an operations log. One line in it states a vault access code.\n\n"
                f"{haystack}\n\nQuestion: what is the vault access code for Project Heron? Reply with the code only.",
            }
        ],
        max_tokens=32,
    )
    return out


def memory(base: str, model: str, nonce: str, contexts: list[int], poll_s: float) -> dict:
    out = {}
    for ctx in contexts:
        if ctx >= 100000:
            doc = buried(words_for(ctx), [(words_for(35000), NEEDLE_HERON), (words_for(110000), NEEDLE_DELTA)], seed=11)
            question = (
                "Two lines in the log above state facts about Project Heron and cluster Delta. Answer "
                "both, one per line: the vault access code for Project Heron, and the backup rotation "
                "owner for cluster Delta."
            )
        else:
            doc = document(words_for(ctx), seed=ctx)
            question = "In one sentence, which component does the log above mention most?"
        content = f"Session {nonce}-m{ctx}.\nBelow is an operations log.\n\n{doc}\n\n{question}"
        r = stream_measured(base, model, content, max_tokens=64, poll_s=poll_s)
        out[str(ctx)] = r
        print(
            f"[memory {ctx}] prompt={r['prompt_tokens']} ttft={r['ttft_s']:.1f}s prefill={r['prefill_tps']:.0f}t/s "
            f"decode={r['decode_tps'] or 0:.1f}t/s active before/prefill-max/decode-max "
            f"{r['active_before_gb']:.2f}/{r['active_prefill_max_gb']}/{r['active_decode_max_gb']} GB "
            f"answer={r['content'][:120]!r}",
            flush=True,
        )
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--base-url", default="http://127.0.0.1:8093")
    ap.add_argument("--phases", default="quality,memory")
    ap.add_argument("--contexts", default="8192,32768,131072")
    ap.add_argument("--poll-seconds", type=float, default=0.5)
    ap.add_argument("--out-dir", default=None)
    ap.add_argument(
        "--nonce",
        default=None,
        help="reuse another run's nonce so the prompts are byte-identical to it (each configuration "
        "runs on its own fresh engine with RAPID_MLX_PREFIX_CACHE_AUTOLOAD=0, so no cache is shared)",
    )
    args = ap.parse_args()
    base = args.base_url.rstrip("/")
    if base.endswith(":8090"):
        sys.exit("refusing: :8090 is the owner's engine — start your own instance on another port")
    models = http_json(f"{base}/v1/models")["data"]
    model = models[0]["id"]
    st = status(base)
    if st.get("num_running") or st.get("num_waiting"):
        sys.exit(f"refusing: engine is busy ({st.get('num_running')} running, {st.get('num_waiting')} waiting)")
    nonce = args.nonce or f"kvq-{args.label}-{datetime.now().strftime('%Y%m%dT%H%M%S')}"
    fixture = json.load(open(os.path.join(HERE, "fixtures", "goose-agent-request.json")))
    out_dir = args.out_dir or os.path.join(HERE, "results", f"{datetime.now().strftime('%Y-%m-%d')}-kv-quant")
    path = os.path.join(out_dir, f"{args.label}.json")
    # One file per configuration; the quality phase (MTP off) and the memory phase (MTP on) run on
    # two engine instances and land in the same record, each phase stamped with its own engine.
    record = json.load(open(path)) if os.path.exists(path) else {}
    record.update({
        "label": args.label,
        "nonce": nonce,
        "started": datetime.now().isoformat(),
        "engine": {k: models[0].get(k) for k in ("id", "context_window", "is_hybrid", "speculative_decoding")},
        "status_start": {k: st.get(k) for k in ("metal", "cache")},
    })
    engine = record["engine"]
    phases = args.phases.split(",")
    if "quality" in phases:
        record["quality_engine"] = engine
        record["quality_nonce"] = nonce
        record["quality"] = quality(base, model, nonce, fixture)
    if "memory" in phases:
        record["memory_engine"] = engine
        record["memory_nonce"] = nonce
        record["memory"] = memory(base, model, nonce, [int(c) for c in args.contexts.split(",")], args.poll_seconds)
    record["finished"] = datetime.now().isoformat()
    record["status_end"] = status(base)
    os.makedirs(out_dir, exist_ok=True)
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
    print(f"written: {path}")


if __name__ == "__main__":
    main()

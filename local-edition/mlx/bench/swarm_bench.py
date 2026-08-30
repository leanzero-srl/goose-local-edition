#!/usr/bin/env python3
"""Swarm-shaped bench for the MLX engine bake-off.

Simulates what the swarm actually does to an engine: N concurrent agent loops, each a
multi-turn tool-calling conversation with a growing, shifting-prefix context. Scores:

  D1 concurrency   — per-stream TTFT + aggregate decode throughput at N in {1,4,8}
  D2 tool fidelity — fraction of turns yielding a well-formed, expected tool call
  D3 hybrid/prefix — identical conversation replayed back-to-back: cache-hit TTFT delta
                     AND fidelity ON the hit (the DeltaNet footgun: omlx #825, mlx-lm #980)
  D4 variance      — stddev of TTFT and decode rate at N=4
  D5 memory        — engine RSS max during the run (needs --engine-pid)

Every failure is a recorded row with a reason. >20% errored requests → the whole run is
verdict=inconclusive with a void_reason; the harness never averages over silence.
"""
import argparse
import json
import statistics
import subprocess
import threading
import time
import urllib.error
import urllib.request

TOOLS = [
    {"type": "function", "function": {
        "name": "shell",
        "description": "Execute a shell command in the project workspace and return stdout/stderr.",
        "parameters": {"type": "object", "properties": {
            "command": {"type": "string", "description": "The command to run"}},
            "required": ["command"]}}},
    {"type": "function", "function": {
        "name": "text_editor",
        "description": "Create or overwrite a file at path with the given content.",
        "parameters": {"type": "object", "properties": {
            "path": {"type": "string"}, "content": {"type": "string"}},
            "required": ["path", "content"]}}},
]

SYSTEM = (
    "You are a swarm worker agent building a small Python service. You have two tools: "
    "shell(command) and text_editor(path, content). You MUST respond to every user turn by "
    "calling exactly one tool — never answer in plain text. Work precisely and keep file "
    "content short but real. The project lives in ./app."
)

TURNS = [
    ("text_editor", "Create app/config.py defining PORT = 8321 and DB_PATH = 'app.db'."),
    ("shell", "List the app directory recursively to confirm config.py exists."),
    ("text_editor", "Create app/store.py with a Store class exposing get(key) and put(key, value) over a dict."),
    ("shell", "Run python3 -c \"import app.store\" to check the module imports."),
    ("text_editor", "Create app/api.py with a handle(request) function returning {'ok': True} for path '/health'."),
    ("shell", "Run the test command python3 -m pytest app -q and report the output."),
]

TOOL_RESULTS = {
    "shell": "exit 0\napp\napp/config.py\napp/store.py\napp/api.py\n",
    "text_editor": "File written successfully.",
}


class RssSampler(threading.Thread):
    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.max_rss_kb, self.stop = pid, 0, threading.Event()

    def run(self):
        while not self.stop.is_set():
            try:
                out = subprocess.check_output(["ps", "-o", "rss=", "-p", str(self.pid)], text=True)
                self.max_rss_kb = max(self.max_rss_kb, int(out.strip() or 0))
            except Exception:
                pass
            self.stop.wait(2.0)


def stream_chat(base, model, messages, timeout, extra=None):
    """One streaming chat request. Returns a result row; never raises."""
    body = {"model": model, "messages": messages, "tools": TOOLS,
            "temperature": 0.2, "max_tokens": 700, "stream": True,
            "stream_options": {"include_usage": True}}
    if extra:
        body.update(extra)
    req = urllib.request.Request(base + "/v1/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json",
                                          "Authorization": "Bearer local"})
    t0 = time.monotonic()
    ttft = None
    finish = None
    text_parts = []
    calls = {}
    usage = None
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            for raw in resp:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                payload = line[5:].strip()
                if payload == "[DONE]":
                    break
                chunk = json.loads(payload)
                if chunk.get("usage"):
                    usage = chunk["usage"]
                choices = chunk.get("choices") or []
                if not choices:
                    continue
                delta = choices[0].get("delta") or {}
                if delta.get("content") or delta.get("tool_calls"):
                    if ttft is None:
                        ttft = time.monotonic() - t0
                if delta.get("content"):
                    text_parts.append(delta["content"])
                for tc in delta.get("tool_calls") or []:
                    slot = calls.setdefault(tc.get("index", 0), {"name": "", "arguments": ""})
                    fn = tc.get("function") or {}
                    if fn.get("name"):
                        slot["name"] += fn["name"]
                    if fn.get("arguments"):
                        slot["arguments"] += fn["arguments"]
                if choices[0].get("finish_reason"):
                    finish = choices[0]["finish_reason"]
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError, json.JSONDecodeError) as e:
        detail = ""
        if isinstance(e, urllib.error.HTTPError):
            try:
                detail = " " + e.read(300).decode("utf-8", "replace")
            except Exception:
                pass
        return {"error": "%s: %s%s" % (type(e).__name__, e, detail), "total_s": time.monotonic() - t0}
    total = time.monotonic() - t0
    if usage and usage.get("completion_tokens"):
        ctok, tsrc = usage["completion_tokens"], "usage"
    else:
        ctok, tsrc = int(sum(len(p) for p in text_parts) / 4) + sum(
            (len(c["arguments"]) + len(c["name"])) // 4 for c in calls.values()), "estimate"
    decode_s = max(total - (ttft or 0), 1e-6)
    return {"error": None, "ttft_s": ttft, "total_s": total, "finish": finish,
            "completion_tokens": ctok, "tokens_source": tsrc,
            "decode_tps": ctok / decode_s, "text": "".join(text_parts),
            "tool_calls": [calls[k] for k in sorted(calls)]}


def judge_tool_call(row, expected_tool):
    """A turn passes only if the engine produced one parseable call to the expected tool."""
    if row["error"]:
        return False, "request error"
    if not row["tool_calls"]:
        return False, "no tool call (finish=%s, text=%r)" % (row["finish"], row["text"][:120])
    call = row["tool_calls"][0]
    if call["name"] != expected_tool:
        return False, "wrong tool %r (expected %r)" % (call["name"], expected_tool)
    try:
        args = json.loads(call["arguments"])
    except json.JSONDecodeError as e:
        return False, "unparseable arguments: %s: %r" % (e, call["arguments"][:120])
    if not isinstance(args, dict) or not args:
        return False, "empty/non-object arguments"
    return True, "ok"


def agent_loop(base, model, loop_id, timeout, results, turns=TURNS):
    messages = [{"role": "system", "content": SYSTEM}]
    for turn_no, (expected_tool, instruction) in enumerate(turns):
        messages.append({"role": "user", "content": instruction})
        row = stream_chat(base, model, messages, timeout)
        ok, why = judge_tool_call(row, expected_tool)
        row.update({"loop": loop_id, "turn": turn_no, "expected": expected_tool,
                    "tool_ok": ok, "tool_why": why})
        row.pop("text", None)
        results.append(row)
        if ok:
            call = row["tool_calls"][0]
            messages.append({"role": "assistant", "content": None, "tool_calls": [
                {"id": "call_%d_%d" % (loop_id, turn_no), "type": "function",
                 "function": {"name": call["name"], "arguments": call["arguments"]}}]})
            messages.append({"role": "tool", "tool_call_id": "call_%d_%d" % (loop_id, turn_no),
                             "content": TOOL_RESULTS[expected_tool]})
        else:
            messages.append({"role": "assistant", "content": row.get("text") or "(no output)"})
            messages.append({"role": "user", "content": "You did not call the required tool. "
                                                        "Call %s now for: %s" % (expected_tool, instruction)})
    return messages


def run_concurrency(base, model, n, timeout):
    results = []
    threads = [threading.Thread(target=agent_loop, args=(base, model, i, timeout, results))
               for i in range(n)]
    t0 = time.monotonic()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    wall = time.monotonic() - t0
    return results, wall


def summarize(results, wall):
    ok_rows = [r for r in results if not r["error"]]
    ttfts = [r["ttft_s"] for r in ok_rows if r["ttft_s"] is not None]
    tps = [r["decode_tps"] for r in ok_rows]
    fid = [r for r in results if r.get("tool_ok")]
    total_tokens = sum(r["completion_tokens"] for r in ok_rows)
    return {
        "requests": len(results), "errors": len(results) - len(ok_rows),
        "error_reasons": sorted({r["error"] for r in results if r["error"]}),
        "fidelity": round(len(fid) / len(results), 3) if results else 0.0,
        "fidelity_failures": [r["tool_why"] for r in results if not r.get("tool_ok") and not r["error"]][:6],
        "ttft_mean_s": round(statistics.mean(ttfts), 3) if ttfts else None,
        "ttft_p95_s": round(sorted(ttfts)[int(0.95 * (len(ttfts) - 1))], 3) if ttfts else None,
        "ttft_stdev_s": round(statistics.stdev(ttfts), 3) if len(ttfts) > 1 else 0.0,
        "decode_tps_mean": round(statistics.mean(tps), 1) if tps else None,
        "decode_tps_stdev": round(statistics.stdev(tps), 1) if len(tps) > 1 else 0.0,
        "tokens_estimated": sum(1 for r in ok_rows if r["tokens_source"] == "estimate"),
        "aggregate_tps": round(total_tokens / wall, 1) if wall else None,
        "wall_s": round(wall, 1),
    }


def prefix_probe(base, model, timeout):
    """Same 3-turn conversation twice; the second run should hit every cache. Fidelity on
    the SECOND run is the hybrid footgun detector."""
    turns = TURNS[:3]
    r1, r2 = [], []
    agent_loop(base, model, 100, timeout, r1, turns)
    agent_loop(base, model, 101, timeout, r2, turns)
    f1 = sum(1 for r in r1 if r.get("tool_ok")) / len(r1) if r1 else 0
    f2 = sum(1 for r in r2 if r.get("tool_ok")) / len(r2) if r2 else 0
    t1 = [r["ttft_s"] for r in r1 if not r["error"] and r["ttft_s"]]
    t2 = [r["ttft_s"] for r in r2 if not r["error"] and r["ttft_s"]]
    return {
        "cold_fidelity": round(f1, 3), "hit_fidelity": round(f2, 3),
        "cold_ttft_mean_s": round(statistics.mean(t1), 3) if t1 else None,
        "hit_ttft_mean_s": round(statistics.mean(t2), 3) if t2 else None,
        "hit_failures": [r["tool_why"] for r in r2 if not r.get("tool_ok")][:6],
        "footgun_detected": f2 < f1,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True)
    ap.add_argument("--base", required=True, help="http://127.0.0.1:PORT")
    ap.add_argument("--model", required=True)
    ap.add_argument("--engine-pid", type=int)
    ap.add_argument("--timeout", type=float, default=600.0)
    ap.add_argument("--concurrency", default="1,4,8")
    ap.add_argument("--ledger", help="experiments.jsonl to append the result row to")
    ap.add_argument("--commit", default="")
    args = ap.parse_args()

    with urllib.request.urlopen(args.base + "/v1/models", timeout=20) as r:
        served = [m["id"] for m in json.load(r).get("data", [])]
    print("engine=%s serving=%s" % (args.engine, served))

    sampler = RssSampler(args.engine_pid) if args.engine_pid else None
    if sampler:
        sampler.start()

    report = {"served_models": served}
    all_rows = 0
    all_errors = 0
    for n in [int(x) for x in args.concurrency.split(",")]:
        print("== concurrency N=%d ==" % n)
        results, wall = run_concurrency(args.base, args.model, n, args.timeout)
        s = summarize(results, wall)
        report["n%d" % n] = s
        all_rows += s["requests"]
        all_errors += s["errors"]
        print(json.dumps(s, indent=2))

    print("== prefix/hybrid probe ==")
    report["prefix_probe"] = prefix_probe(args.base, args.model, args.timeout)
    print(json.dumps(report["prefix_probe"], indent=2))

    if sampler:
        sampler.stop.set()
        sampler.join(3)
        report["engine_max_rss_gb"] = round(sampler.max_rss_kb / (1024 ** 2), 2)

    error_rate = all_errors / all_rows if all_rows else 1.0
    verdict = "pass" if error_rate <= 0.2 and not report["prefix_probe"]["footgun_detected"] else \
              ("inconclusive" if error_rate > 0.2 else "fail")
    void_reason = None
    if error_rate > 0.2:
        void_reason = "error rate %.0f%% across %d requests — measurements not trustworthy" % (100 * error_rate, all_rows)
    elif report["prefix_probe"]["footgun_detected"]:
        void_reason = "hybrid prefix-cache footgun: fidelity dropped on cache hit"

    row = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "experiment": "swarm-bench",
           "engine": args.engine, "config": {"model": args.model, "base": args.base,
                                             "concurrency": args.concurrency, "turns": len(TURNS)},
           "result": report, "verdict": verdict, "commit": args.commit}
    if void_reason:
        row["void_reason"] = void_reason
    if args.ledger:
        with open(args.ledger, "a") as fh:
            fh.write(json.dumps(row) + "\n")
    print("VERDICT: %s%s" % (verdict, " — " + void_reason if void_reason else ""))
    return 0 if verdict == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())

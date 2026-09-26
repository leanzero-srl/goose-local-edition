"""sample.py <cap.jsonl> <req> <n> <out.jsonl> [max_tokens] — send one captured goose request n times exactly as
goose does (no sampling fields: the engine's defaults from generation_config, temperature 1.0 / top_p 0.95 /
top_k 20 on the 27B), non-streaming, and record each completion's text + tool calls.
SAMPLING='{"temperature":0.7,"top_p":0.8}' in the environment overrides the sampling fields."""
import json
import os
import sys
import time
import urllib.request

cap, req, n, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
max_tokens = int(sys.argv[5]) if len(sys.argv) > 5 else 48
body = [json.loads(l)["body"] for l in open(cap)][req]
body = dict(body)
body.pop("stream_options", None)
body.update(stream=False, max_tokens=max_tokens)
if os.environ.get("SAMPLING"):
    body.update(json.loads(os.environ["SAMPLING"]))
for i in range(n):
    r = urllib.request.Request("http://127.0.0.1:18571/v1/chat/completions", data=json.dumps(body).encode(),
                               headers={"Content-Type": "application/json"})
    t0 = time.time()
    with urllib.request.urlopen(r, timeout=3600) as resp:
        j = json.loads(resp.read())
    m = j["choices"][0]["message"]
    rec = {"i": i, "req": req, "secs": round(time.time() - t0, 2), "content": m.get("content"),
           "calls": [c["function"]["name"] for c in (m.get("tool_calls") or [])], "usage": j.get("usage")}
    with open(out, "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(i, rec["secs"], repr((m.get("content") or "")[:100]), rec["calls"], flush=True)

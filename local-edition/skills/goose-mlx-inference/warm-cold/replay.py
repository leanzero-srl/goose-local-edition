"""replay.py <cap.jsonl> <out.jsonl> <indices, e.g. 0-20,5,10> [max_tokens]
Send the captured goose requests at temperature 0 (non-streaming, top-5 logprobs), one after another,
and record the generated tokens + logprobs + the engine's usage per request."""
import json
import sys
import time
import urllib.request

cap, out, spec = sys.argv[1], sys.argv[2], sys.argv[3]
max_tokens = int(sys.argv[4]) if len(sys.argv) > 4 else 256
rows = [json.loads(l)["body"] for l in open(cap)]
idx = []
for part in spec.split(","):
    if "-" in part:
        a, b = part.split("-")
        idx += list(range(int(a), int(b) + 1))
    else:
        idx.append(int(part))
for n, i in enumerate(idx):
    body = dict(rows[i])
    body.pop("stream_options", None)
    body.update(stream=False, temperature=0.0, top_p=1.0, top_k=0, seed=0, max_tokens=max_tokens,
                logprobs=True, top_logprobs=5)
    req = urllib.request.Request("http://127.0.0.1:18571/v1/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=3600) as r:
        resp = json.loads(r.read())
    dt = time.time() - t0
    ch = resp["choices"][0]
    lp = (ch.get("logprobs") or {}).get("content") or []
    rec = {"seq": n, "req": i, "secs": round(dt, 2), "usage": resp.get("usage"), "finish": ch.get("finish_reason"),
           "message": ch.get("message"),
           "tokens": [t["token"] for t in lp],
           "lps": [t["logprob"] for t in lp],
           "top": [[(x["token"], x["logprob"]) for x in t.get("top_logprobs", [])] for t in lp]}
    with open(out, "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(n, i, f"{dt:.1f}s", len(lp), "tok", json.dumps(resp.get("usage"))[:200], flush=True)

#!/usr/bin/env python3
"""R2 load through goose's Link relay (the path chat uses to a linked Mac's engine).

usage: load.py <out-dir> --workers N --minutes M --prompt-tokens T [--abort-ratio 0.2] [--max-tokens 128]
Reads the relay base URL from goose's route record. Every request: a UNIQUE prompt (so the engine's
prefix cache churns), streamed; per request one CSV row: start, status, ttft_s, tokens, max_gap_s,
total_s, aborted, error. A 1-token CANARY runs every 60 s in its own thread — it, not any health
route, says whether the engine still generates. Stdlib only.
"""
import argparse, csv, http.client, json, os, random, threading, time, urllib.parse

ap = argparse.ArgumentParser()
ap.add_argument('out'); ap.add_argument('--workers', type=int, default=2)
ap.add_argument('--minutes', type=float, default=20); ap.add_argument('--prompt-tokens', type=int, default=13000)
ap.add_argument('--abort-ratio', type=float, default=0.2); ap.add_argument('--max-tokens', type=int, default=128)
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)
route = json.load(open(os.path.expanduser('~/.local/state/goose/mlx-remote-route.json')))
base = urllib.parse.urlparse(route['base_url']); model = route['served_model_id']
WORDS = open('/usr/share/dict/words').read().split()
lock = threading.Lock(); stop_at = time.time() + a.minutes * 60
rows = open(os.path.join(a.out, 'requests.csv'), 'w', newline=''); w = csv.writer(rows)
w.writerow(['kind', 'worker', 'start', 'status', 'ttft_s', 'tokens', 'max_gap_s', 'total_s', 'aborted', 'error'])

def prompt(n_tokens):
    # measured 2026-09-25 (ladder.py): ~2.1 tokens per dictionary word on this Qwen tokenizer; a nonce first so no two prompts share a prefix.
    words = [random.choice(WORDS) for _ in range(int(n_tokens / 2.1))]
    return f"[{random.getrandbits(64):x}] Summarise the following list in one sentence: " + ' '.join(words)

def one(kind, worker, content, max_tokens, abort_after):
    t0 = time.time(); status = ''; ttft = None; toks = 0; gap = 0.0; last = None; err = ''; aborted = False
    try:
        c = http.client.HTTPConnection(base.hostname, base.port, timeout=600)
        body = json.dumps({'model': model, 'stream': True, 'max_tokens': max_tokens, 'temperature': 0,
                           'messages': [{'role': 'user', 'content': content}]})
        c.request('POST', base.path + '/v1/chat/completions', body, {'Content-Type': 'application/json'})
        r = c.getresponse(); status = r.status
        if r.status != 200:
            err = r.read(300).decode(errors='replace').replace('\n', ' ')
        else:
            for raw in r:
                line = raw.decode(errors='replace').strip()
                if not line.startswith('data:') or line == 'data: [DONE]': continue
                now = time.time()
                if ttft is None: ttft = now - t0
                if last is not None: gap = max(gap, now - last)
                last = now; toks += 1
                if abort_after and toks >= abort_after:
                    aborted = True; break
        c.close()
    except Exception as e:  # a transport failure is a row, never a skipped one
        err = f'{type(e).__name__}: {e}'
    with lock:
        w.writerow([kind, worker, f'{t0:.1f}', status, f'{ttft:.2f}' if ttft else '', toks, f'{gap:.2f}',
                    f'{time.time() - t0:.1f}', int(aborted), err[:200]]); rows.flush()

def worker(i):
    while time.time() < stop_at:
        abort_after = random.randint(3, 40) if random.random() < a.abort_ratio else 0
        one('load', i, prompt(a.prompt_tokens), a.max_tokens, abort_after)

def canary():
    while time.time() < stop_at:
        one('canary', -1, f'[{random.getrandbits(32):x}] Reply with the single word: ok', 1, 0)
        time.sleep(60)

ts = [threading.Thread(target=worker, args=(i,), daemon=True) for i in range(a.workers)]
ts.append(threading.Thread(target=canary, daemon=True))
for t in ts: t.start()
for t in ts: t.join()
print('done', a.out)

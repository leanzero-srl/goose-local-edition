#!/usr/bin/env python3
"""Correctness ladder (research D33): a needle at ~400 / 2.1k / 2.7k / 4.5k / 7.3k / 15k / 30k prompt tokens,
temperature 0, the answer checked. Throughput is never the verdict — a split that returns garbage past
~2k tokens while decoding fast fails here.

usage: ladder.py <base-url> <model> <out.csv> [--sizes 400,2100,...]
<base-url> is an OpenAI-compatible root (http://127.0.0.1:<port> for the split's rank 0, or the relay
base from ~/.local/state/goose/mlx-remote-route.json). Appends one row per size.
"""
import argparse, csv, json, random, time, urllib.request

ap = argparse.ArgumentParser()
ap.add_argument('base'); ap.add_argument('model'); ap.add_argument('out')
ap.add_argument('--sizes', default='400,2100,2700,4500,7300,15000,30000')
a = ap.parse_args()
WORDS = open('/usr/share/dict/words').read().split()
w = csv.writer(open(a.out, 'a', newline=''))
for size in [int(s) for s in a.sizes.split(',')]:
    code = f"{random.randint(100000, 999999)}"
    filler = [random.choice(WORDS) for _ in range(int(size / 2.1))]  # measured: 2.1 tokens per dictionary word (400→880, 2100→4447 at 1.3)
    pos = random.randint(len(filler) // 4, 3 * len(filler) // 4)
    filler.insert(pos, f"(The secret code is {code}.)")
    msg = "Read this text.\n\n" + ' '.join(filler) + "\n\nWhat is the secret code? Answer with the six digits only."
    # Thinking off: a reasoning turn would spend the budget before the digits and read as a wrong answer.
    body = json.dumps({'model': a.model, 'temperature': 0, 'max_tokens': 32,
                       'chat_template_kwargs': {'enable_thinking': False},
                       'messages': [{'role': 'user', 'content': msg}]}).encode()
    t0 = time.time(); answer = ''; err = ''; ptoks = ''
    try:
        req = urllib.request.Request(a.base.rstrip('/') + '/v1/chat/completions', body, {'Content-Type': 'application/json'})
        d = json.load(urllib.request.urlopen(req, timeout=1800))
        answer = (d['choices'][0]['message'].get('content') or '').strip()
        ptoks = d.get('usage', {}).get('prompt_tokens', '')
    except Exception as e:
        err = f'{type(e).__name__}: {e}'
    ok = code in answer
    w.writerow([time.strftime('%Y-%m-%dT%H:%M:%S'), size, ptoks, code, answer[:40].replace('\n', ' '), int(ok), f'{time.time()-t0:.1f}', err[:160]])
    print(f"{size:>6} tokens (prompt {ptoks}): {'OK ' if ok else 'BAD'} {answer[:40]!r} {err[:80]}", flush=True)

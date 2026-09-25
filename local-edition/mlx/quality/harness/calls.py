#!/usr/bin/env python3
"""calls.py <out-dir> — until killed: one row per goose provider call, deduped by the response id.

goose keeps only the last few calls in ~/.local/state/goose/logs/llm_request.<n>.jsonl and rotates them
(0 → 1 → 2), so a reader keyed on file mtime re-counts the same call as it moves (r1.mjs's calls.tsv did).
This polls every second and keys on the stream's chatcmpl id. Per call: when seen, what it is (the first words
of its system prompt), tool count, message count, input, output, cache_read, and whether the model thought.
"""
import csv, glob, json, os, sys, time

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
path = os.path.join(out, 'calls.csv')
new = not os.path.exists(path)
f = open(path, 'a', newline='')
w = csv.writer(f)
if new:
    w.writerow(['seen', 'id', 'kind', 'tools', 'msgs', 'input', 'output', 'cache_read', 'thinking_chunks', 'text_chunks'])
seen = set()
LOGS = os.path.expanduser('~/.local/state/goose/logs')
while True:
    for p in glob.glob(os.path.join(LOGS, 'llm_request.[0-9].jsonl')):
        try:
            lines = open(p).read().splitlines()
            if len(lines) < 2:
                continue
            last = json.loads(lines[-1])
            if not last.get('usage'):
                continue
            first_data = json.loads(lines[1]).get('data') or {}
            cid = first_data.get('id') or f'{p}:{len(lines)}:{last["usage"]}'
            if cid in seen:
                continue
            seen.add(cid)
            req = json.loads(lines[0])['input']
            msgs = req.get('messages', [])
            sysm = next((m for m in msgs if m.get('role') == 'system'), {})
            c = sysm.get('content')
            c = c if isinstance(c, str) else json.dumps(c)
            kind = ' '.join((c or '').split()[:8])
            think = text = 0
            for l in lines[1:]:
                d = (json.loads(l).get('data') or {})
                for part in d.get('content', []) if isinstance(d, dict) else []:
                    if part.get('type') == 'thinking':
                        think += 1
                    elif part.get('type') == 'text':
                        text += 1
            u = last['usage']
            w.writerow([time.strftime('%H:%M:%S'), cid, kind, len(req.get('tools') or []), len(msgs),
                        u.get('input_tokens'), u.get('output_tokens'), u.get('cache_read_input_tokens'), think, text])
            f.flush()
        except Exception:
            continue
    time.sleep(1)

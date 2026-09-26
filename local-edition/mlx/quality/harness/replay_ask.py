"""ask.py <prompts.jsonl> <out.jsonl> [samples] [model] [reasoning] — each prompt row {id, system, user, ...}
asked `samples` times of a model; every reply (or its error) is written, never dropped.

reasoning: `on` (default, what Q-90's baselines measured) or `off` — what goose sends since Q-132: the
helper path's reasoning switch and the verdict's envelope (max_tokens = the prompt's UTF-8 bytes,
`turn_assessment::verdict_envelope`). The model is OpenRouter's unless REPLAY_ENDPOINT names an
OpenAI-compatible chat URL (a local MLX engine, e.g. http://127.0.0.1:8091/v1/chat/completions); off is
`reasoning: {effort: none}` on OpenRouter and `chat_template_kwargs: {enable_thinking: false}` on an
engine — each is what goose's own provider for that endpoint sends."""
import concurrent.futures
import json
import os
import sys
import time
import urllib.request

src, out = sys.argv[1], sys.argv[2]
samples = int(sys.argv[3]) if len(sys.argv) > 3 else 1
model = sys.argv[4] if len(sys.argv) > 4 else 'qwen/qwen3.8-27b'
reasoning = sys.argv[5] if len(sys.argv) > 5 else 'on'
if reasoning not in ('on', 'off'):
    sys.exit(f'reasoning must be on or off, not {reasoning!r}')
endpoint = os.environ.get('REPLAY_ENDPOINT')
key = None
if endpoint is None:
    endpoint = 'https://openrouter.ai/api/v1/chat/completions'
    for line in open('/Users/mihaiperdum/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env'):
        if line.startswith('OPENROUTER_API_KEY='):
            key = line.split('=', 1)[1].strip().strip('"').strip("'")
rows = [json.loads(l) for l in open(src)]
jobs = [(row, k) for row in rows for k in range(samples)]


def request(row):
    body = {
        'model': model,
        'messages': [{'role': 'system', 'content': row['system']}, {'role': 'user', 'content': row['user']}],
    }
    if reasoning == 'off':
        body['max_tokens'] = len(row['system'].encode()) + len(row['user'].encode())
        if key is not None:
            body['reasoning'] = {'effort': 'none'}
        else:
            body['chat_template_kwargs'] = {'enable_thinking': False}
    return json.dumps(body).encode()


def ask(job):
    row, k = job
    body = request(row)
    headers = {'Content-Type': 'application/json'}
    if key is not None:
        headers['Authorization'] = f'Bearer {key}'
    err = None
    for attempt in range(4):
        try:
            req = urllib.request.Request(endpoint, data=body, headers=headers)
            data = json.load(urllib.request.urlopen(req, timeout=900))
            msg = data['choices'][0]['message']
            content = msg.get('content') or ''
            if '{' not in content:
                content = content + '\n' + (msg.get('reasoning') or msg.get('reasoning_content') or '')
            return {**{x: row[x] for x in row if x not in ('system', 'user')}, 'sample': k, 'reply': content,
                    'usage': data.get('usage'), 'finish': data['choices'][0].get('finish_reason')}
        except Exception as e:
            err = repr(e)
            time.sleep(5 * (attempt + 1))
    return {**{x: row[x] for x in row if x not in ('system', 'user')}, 'sample': k, 'reply': '', 'error': err}


with concurrent.futures.ThreadPoolExecutor(1 if key is None else 12) as pool:
    results = list(pool.map(ask, jobs))
with open(out, 'w') as f:
    for r in results:
        f.write(json.dumps(r) + '\n')
errors = [r for r in results if r.get('error')]
print(len(results), 'replies,', len(errors), 'errors', [e['error'][:120] for e in errors[:3]])

"""ask.py <prompts.jsonl> <out.jsonl> [samples] [model] — each prompt row {id, system, user, ...} asked
`samples` times of a cloud model on OpenRouter; every reply (or its error) is written, never dropped."""
import concurrent.futures
import json
import sys
import time
import urllib.request

src, out = sys.argv[1], sys.argv[2]
samples = int(sys.argv[3]) if len(sys.argv) > 3 else 1
model = sys.argv[4] if len(sys.argv) > 4 else 'qwen/qwen3.8-27b'
key = None
for line in open('/Users/mihaiperdum/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env'):
    if line.startswith('OPENROUTER_API_KEY='):
        key = line.split('=', 1)[1].strip().strip('"').strip("'")
rows = [json.loads(l) for l in open(src)]
jobs = [(row, k) for row in rows for k in range(samples)]


def ask(job):
    row, k = job
    body = json.dumps({
        'model': model,
        'messages': [{'role': 'system', 'content': row['system']}, {'role': 'user', 'content': row['user']}],
    }).encode()
    err = None
    for attempt in range(4):
        try:
            req = urllib.request.Request('https://openrouter.ai/api/v1/chat/completions', data=body,
                                         headers={'Authorization': f'Bearer {key}', 'Content-Type': 'application/json'})
            data = json.load(urllib.request.urlopen(req, timeout=900))
            msg = data['choices'][0]['message']
            content = msg.get('content') or ''
            if '{' not in content:
                content = content + '\n' + (msg.get('reasoning') or '')
            return {**{x: row[x] for x in row if x not in ('system', 'user')}, 'sample': k, 'reply': content,
                    'usage': data.get('usage')}
        except Exception as e:
            err = repr(e)
            time.sleep(5 * (attempt + 1))
    return {**{x: row[x] for x in row if x not in ('system', 'user')}, 'sample': k, 'reply': '', 'error': err}


with concurrent.futures.ThreadPoolExecutor(12) as pool:
    results = list(pool.map(ask, jobs))
with open(out, 'w') as f:
    for r in results:
        f.write(json.dumps(r) + '\n')
errors = [r for r in results if r.get('error')]
print(len(results), 'replies,', len(errors), 'errors', [e['error'][:120] for e in errors[:3]])

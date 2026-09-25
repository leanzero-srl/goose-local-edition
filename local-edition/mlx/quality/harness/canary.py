#!/usr/bin/env python3
"""R3/R6 canary: a 1-token completion through goose's relay every <interval> s, one CSV row each —
t, ok, status, latency_s, relay_port, error. The relay's base is re-read from the route record on every
call, so a route re-established on a new port is followed. Runs until killed or --minutes.

usage: canary.py <out.csv> [--interval 2] [--minutes 30] [--timeout 120]
"""
import argparse, csv, http.client, json, os, time, urllib.parse
ap = argparse.ArgumentParser(); ap.add_argument('out'); ap.add_argument('--interval', type=float, default=2)
ap.add_argument('--minutes', type=float, default=30); ap.add_argument('--timeout', type=float, default=120)
a = ap.parse_args(); w = csv.writer(open(a.out, 'a', newline=''))
end = time.time() + a.minutes * 60
REC = os.path.expanduser('~/.local/state/goose/mlx-remote-route.json')
while time.time() < end:
    t0 = time.time(); ok = 0; status = ''; err = ''; port = ''
    try:
        r = json.load(open(REC)); base = urllib.parse.urlparse(r['base_url']); port = base.port
        c = http.client.HTTPConnection(base.hostname, base.port, timeout=a.timeout)
        body = json.dumps({'model': r['served_model_id'], 'max_tokens': 1, 'temperature': 0,
                           'chat_template_kwargs': {'enable_thinking': False},
                           'messages': [{'role': 'user', 'content': f'[{int(t0)}] Say ok.'}]})
        c.request('POST', base.path + '/v1/chat/completions', body, {'Content-Type': 'application/json'})
        resp = c.getresponse(); status = resp.status; data = resp.read()
        if resp.status == 200: ok = 1
        else: err = data[:200].decode(errors='replace').replace('\n', ' ')
    except FileNotFoundError:
        err = 'no route record'
    except Exception as e:
        err = f'{type(e).__name__}: {e}'[:200]
    lat = time.time() - t0
    w.writerow([time.strftime('%H:%M:%S', time.localtime(t0)), ok, status, f'{lat:.2f}', port, err]); 
    print(time.strftime('%H:%M:%S', time.localtime(t0)), ok, status, f'{lat:.2f}s', port, err[:120], flush=True)
    time.sleep(max(0, a.interval - lat))

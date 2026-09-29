#!/bin/zsh
# runwatch.sh <run-dir> — start it with run_in_background beside EVERY E2E/load run; it EXITS (and so wakes the
# coordinator) the moment a stop rule fires, independent of the cron heartbeat. 2026-09-27: the heartbeat did not
# fire from 00:35 to 07:50 and E2E #3e ran 6.9 h into a runaway answer, a killed split and 29 dead turns unseen.
# Fires on: a turn row that ended notice/hang · a LIVE finding in events.log · the driver (r1.mjs/load.py) gone ·
# one agent call writing more than RATIO × the median output of the calls before it (the runaway shape) · the
# split's /v1/status unreachable while the driver still runs.
d=${1:?run dir}
RATIO=${RATIO:-20}   # ratio: of the run's own median call output
seen_live=$(cat $d/events.log 2>/dev/null | grep -c ' LIVE ')  # grep -c already prints 0; '|| echo 0' made it "0\n0"
while true; do
  if awk -F'\t' 'NR>1 && ($5=="notice" || $5=="hang") {f=1} END {exit !f}' $d/turns.tsv 2>/dev/null; then
    echo "STOP: a turn ended notice/hang"; awk -F'\t' 'NR>1 {print $1, $4, $5, $8}' $d/turns.tsv | tail -3; exit 0; fi
  now_live=$(cat $d/events.log 2>/dev/null | grep -c ' LIVE ')
  if [ "$now_live" -gt "$seen_live" ]; then echo "LIVE finding:"; grep ' LIVE ' $d/events.log | tail -1 | cut -c1-300; exit 0; fi
  # The round's end: r1 writes every card still open and every note not delivered as acted into round.json
  # `fails` (owner 2026-09-29: nothing stays pending) — printed here so the wake carries them.
  if ! pgrep -f "(r1.mjs|load.py) $d" >/dev/null; then echo "driver gone"; tail -3 $d/turns.tsv 2>/dev/null
    python3 -c "
import json,sys
try: r=json.load(open('$d/round.json'))
except Exception as e: print('round.json unreadable:', e); sys.exit()
if 'fails' not in r: print('round.json has no fails list: r1 did not reach its round end'); sys.exit()
for f in r['fails']: print(f)
if not r['fails']: print('round end: no open card, no undelivered note')
for w in r.get('notesWaiting', []): print('WAITING:', w)"
    exit 0; fi
  if ! curl -s -m 5 127.0.0.1:8091/v1/models >/dev/null 2>&1 && ! curl -s -m 5 127.0.0.1:8090/v1/models >/dev/null 2>&1; then
    echo "engine unreachable while the driver runs"; exit 0; fi
  run=$(curl -s -m 5 127.0.0.1:8091/v1/status 2>/dev/null | python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit()
rs=d.get('requests',[]) or [{}]
r=max(rs, key=lambda r: r.get('completion_tokens') or 0)
print(r.get('completion_tokens') or 0, r.get('prompt_tokens') or 0)" 2>/dev/null)
  prompt=${run#* }; run=${run%% *}
  med=$(python3 -c "
import csv,statistics,sys
try: rows=[int(r['output']) for r in csv.DictReader(open('$d/calls.csv')) if r['kind'].startswith('You are a general-purpose')]
except Exception: rows=[]
print(int(statistics.median(rows)) if rows else 0)" 2>/dev/null)
  if [ -n "$run" ] && [ "${med:-0}" -gt 0 ] && [ "$run" -gt $(( med * RATIO )) ]; then
    echo "SIZE ALARM (summons a reader, not a verdict — read the words first; E2E #5b: a legit 12 KB VENDORED.md tripped it): a call has written $run tokens, ${RATIO}× the run's median agent-call output ($med)"; exit 0; fi
  # Before any median exists: an answer longer than its own prompt is the runaway shape (#3e: 221k tokens on a
  # 40k prompt; a healthy first answer is a few hundred to a few thousand). NOTE: #3d's 24k on 40k would NOT
  # trip this — only the median rule, from the second agent call on, sees that size.
  if [ -n "$run" ] && [ "${med:-0}" -eq 0 ] && [ "${prompt:-0}" -gt 0 ] && [ "$run" -gt "$prompt" ]; then
    echo "RUNAWAY: the first call has written $run tokens on a $prompt-token prompt"; exit 0; fi
  # The words: a live answer whose tail is one short span repeated is a loop, whatever its size (E2E #3f:
  # `!\n</parameter>\n</function>\n` over and over at 1,480 tokens — far under the size rules above).
  loop=$(curl -s -m 5 127.0.0.1:8091/v1/status 2>/dev/null | python3 -c "
import json,sys
REPEATS=8  # a span seen 8 times back to back at the end of the tail is a loop, not prose
try: d=json.load(sys.stdin)
except Exception: sys.exit()
for r in d.get('requests',[]):
    t=((r.get('stream') or {}).get('tail') or '')
    n=len(t)
    for p in range(1, n//REPEATS + 1):
        span=t[n-p:]
        if span.strip() and t.endswith(span*REPEATS):
            print(repr(span)[:120]); sys.exit()
" 2>/dev/null)
  # A repeat must SURVIVE the next sample to be a loop: #3r turn 5 (2026-09-28) tripped on a code comment rule
  # `// ── Helpers ────…` — the stream moved on 30 s later. A loop still ends in the same span one sample on.
  if [ -n "$loop" ] && [ "$loop" = "${prev_loop:-}" ]; then echo "RUNAWAY (words): the live tail ended in 8+ copies of $loop on two samples in a row — read /v1/status stream.tail"; exit 0; fi
  prev_loop=$loop
  sleep 30
done

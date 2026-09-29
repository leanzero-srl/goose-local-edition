# demo/ — the owner's strategy demo drivers (2026-09-28, DEMO-2026-09-28-strategy)
CDP on 9333 (the installed app). lib.mjs: page()/shot()/txt(); DIR = the demo's evidence folder.
- openchat.mjs — register DIR as a project and open "New session here".
- turn.mjs <label> <msg> — send one message in the open chat (SESSION=<id> to open one first; NOWAIT=1 to only
  send) and log every surface change (composer line, engine glance, delegate/loader lines) to DIR/turns.jsonl.
- monitor.mjs — polls the engine glance to DIR/monitor.jsonl. cycles.sh/waitturn.sh — the 3-cycle switch loop.
- summ.py — switch/load timings from the logs.
Moved here from a session scratchpad (the skill: reusable tooling lives in the repo).

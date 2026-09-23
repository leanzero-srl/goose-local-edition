# phase1-cold-lz1

cold replays (POST /v1/cache/clear before each; 0 entries verified) on v0.14.3-lz.1 · nonce `phase1-identity` · seed 7 · usage from `2026-09-23-phase1-lz1-seeded/results.json`

| target | attempt | TTFT s | cache saved | == lz.1 warm (control, seeded) | == lz.2 marker OFF | == lz.2 marker ON (unseeded, seeded) | tool call / content head |
|---|---|---|---|---|---|---|---|
| a:0:1 | 0 | 1.4 | 0 | NO, NO | NO | NO, NO | `The dominant cost for a long prompt is the prefill phase, wh` |
| a:0:1 | 1 | 1.0 | 0 | NO, NO | NO | NO, NO | `The dominant cost for a long prompt is the prefill phase, wh` |
| c:2:2 | 0 | 250.5 | 0 | yes, yes | yes | NO, NO | `[{"name": "shell", "arguments": "{\"command\": \"ls /Users/operator/project/src\"}"}]` |
| c:2:2 | 1 | 251.5 | 0 | yes, yes | yes | NO, NO | `[{"name": "shell", "arguments": "{\"command\": \"ls /Users/operator/project/src\"}"}]` |

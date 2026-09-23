# phase1-cold-lz1-b

cold replays (POST /v1/cache/clear before each; 0 entries verified) on v0.14.3-lz.1 · nonce `phase1-identity` · seed 7 · usage from `2026-09-23-phase1-lz1-seeded/results.json`

| target | attempt | TTFT s | cache saved | == lz.1 warm (control, seeded) | == lz.2 marker OFF | == lz.2 marker ON (unseeded, seeded) | tool call / content head |
|---|---|---|---|---|---|---|---|
| c:2:4 | 0 | 279.6 | 0 | yes, yes | yes | NO, NO | `[{"name": "shell", "arguments": "{\"command\": \"rg -n 'handle_request' /Users/operator/project/src/server/han` |
| c:0:2 | 0 | 252.1 | 0 | yes, yes | yes | yes, yes | `[{"name": "shell", "arguments": "{\"command\": \"ls -la /Users/operator/project/src/\"}"}]` |

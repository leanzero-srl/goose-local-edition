# KV-cache compression — reference `bf16`

## quality: int8 vs bf16

greedy token agreement 28.0% · identical answers 5/13 · median top-1 margin at divergence 0.125 (max 0.25)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 66/256 | @66: ' calculates' → ' performs' (margin 0.250, rank 2) |  |
| jql | 150/150 | identical |  |
| confluence | 47/256 | @47: ' the' → ' primary' (margin 0.000, rank 2) |  |
| python | 20/256 | @20: ' duration' → ' a' (margin 0.125, rank 2) |  |
| rust | 47/156 | @47: ' management' → ' tables' (margin 0.125, rank 3) |  |
| arithmetic | 0/76 | @0: '1' → '5' (margin 0.000, rank 1) |  |
| jira-admin | 6/219 | @6: ' first' → ':' (margin 0.125, rank 2) |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 27/39 | @27: '/mod' → '/h' (margin 0.125, rank 2) | tools ≠ ['shell'] |
| b-long-32k | 2/200 | @2: ' frequently' → ' is' (margin 0.125, rank 2) |  |
| retrieval-31k | 8/8 | identical | found |

## memory and speed (streaming, MTP on, no logprobs)

| config | context | prompt tok | TTFT s | prefill tok/s | decode tok/s | Δactive decode GB | Δactive prefill-peak GB | 128k answer |
|---|---|---|---|---|---|---|---|---|
| bf16 | 8192 | 8207 | 49.5 | 166 | 14.6 | 3.77 | 4.02 |  |
| bf16 | 32768 | 32761 | 199.1 | 165 | 8.9 | 12.19 | 8.52 |  |
| bf16 | 131072 | 130650 | 1069.8 | 122 | 8.9 | 28.01 | 24.57 | KESTREL-4471 / Ioana Marinescu |
| bf16 | slope | | | | | 157.8 KiB/token (last two contexts) | | |

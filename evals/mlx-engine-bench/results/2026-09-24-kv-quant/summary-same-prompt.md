# KV-cache compression — reference `bf16-same`

## quality: bf16-same2 vs bf16-same

greedy token agreement 100.0% · identical answers 13/13 · median top-1 margin at divergence None (max None)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 256/256 | identical |  |
| jql | 150/150 | identical |  |
| confluence | 256/256 | identical |  |
| python | 256/256 | identical |  |
| rust | 212/212 | identical |  |
| arithmetic | 81/81 | identical |  |
| jira-admin | 256/256 | identical |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 44/44 | identical | tools = ['shell'] |
| b-long-32k | 175/175 | identical |  |
| retrieval-31k | 8/8 | identical | found |

## quality: int8-same vs bf16-same

greedy token agreement 62.4% · identical answers 8/13 · median top-1 margin at divergence 0.0 (max 0.25)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 15/256 | @15: ' model' → ' full' (margin 0.000, rank 1) |  |
| jql | 150/150 | identical |  |
| confluence | 256/256 | identical |  |
| python | 256/256 | identical |  |
| rust | 17/212 | @17: ' few' → ' rare' (margin 0.000, rank 1) |  |
| arithmetic | 81/81 | identical |  |
| jira-admin | 72/256 | @72: ' create' → ' Review' (margin 0.000, rank 2) |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 14/44 | @14: 'sed' → 'rg' (margin 0.125, rank 2) | tools ≠ ['shell'] |
| b-long-32k | 147/175 | @147: ' fragments' → ' log' (margin 0.250, rank 2) |  |
| retrieval-31k | 8/8 | identical | found |

## quality: int4-same vs bf16-same

greedy token agreement 22.1% · identical answers 4/13 · median top-1 margin at divergence 0.125 (max 0.625)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 15/256 | @15: ' model' → ' full' (margin 0.000, rank 1) |  |
| jql | 54/150 | @54: 'iss' → 'AND' (margin 0.000, rank 2) |  |
| confluence | 113/256 | @113: ' list' → ',' (margin 0.250, rank 2) |  |
| python | 16/256 | @16: 'Convert' → '\n' (margin 0.375, rank 2) |  |
| rust | 17/212 | @17: ' few' → ' rare' (margin 0.000, rank 1) |  |
| arithmetic | 1/81 | @1: '6' → ' hours' (margin 0.250, rank 2) |  |
| jira-admin | 19/256 | @19: ' so' → ' a' (margin 0.625, rank 2) |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 14/44 | @14: 'sed' → 'rg' (margin 0.125, rank 2) | tools ≠ ['shell'] |
| b-long-32k | 31/175 | @31: 'scheduler' → 'kernel' (margin 0.125, rank 2) |  |
| retrieval-31k | 8/8 | identical | found |

## quality summary

| config vs reference | agreement to first divergence | identical answers | median / max reference margin at divergence (nats) | fact at 31k | arithmetic final (right: 30; reference said 360) | agent tool calls equal |
|---|---|---|---|---|---|---|
| bf16-same2 vs bf16-same | 100.0% | 13/13 | None / None | found | 360 | 4/4 |
| int8-same vs bf16-same | 62.4% | 8/13 | 0.0 / 0.25 | found | 360 | 3/4 |
| int4-same vs bf16-same | 22.1% | 4/13 | 0.125 / 0.625 | found | 30 | 3/4 |

## memory and prefill (streaming, MTP on, no logprobs; one fresh engine per configuration)

Peak = the engine's Metal peak after the request (process-wide, contexts run in ascending order).
Prefix cache = the engine's retained entries after the request, within its fixed memory budget.

| config | context | prompt tok | TTFT s | prefill tok/s | peak GB | Δ peak vs bf16 | prefix cache after (entries, GB) | 128k answer |
|---|---|---|---|---|---|---|---|---|

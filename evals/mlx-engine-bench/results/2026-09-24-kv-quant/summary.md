# KV-cache compression — reference `bf16`

## quality: bf16-repeat vs bf16

greedy token agreement 45.3% · identical answers 5/13 · median top-1 margin at divergence 0.125 (max 0.25)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 183/256 | @183: ' However' → ' If' (margin 0.250, rank 2) |  |
| jql | 150/150 | identical |  |
| confluence | 70/256 | @70: ' details' → ' metrics' (margin 0.000, rank 2) |  |
| python | 93/256 | @93: ' does' → ' is' (margin 0.125, rank 2) |  |
| rust | 47/156 | @47: ' management' → ' tables' (margin 0.125, rank 3) |  |
| arithmetic | 1/76 | @1: '/' → '2' (margin 0.000, rank 1) |  |
| jira-admin | 98/219 | @98: ' field' → ' custom' (margin 0.125, rank 2) |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 14/39 | @14: 'rg' → 'sed' (margin 0.000, rank 2) | tools ≠ ['shell'] |
| b-long-32k | 8/200 | @8: ' particularly' → ' most' (margin 0.125, rank 2) |  |
| retrieval-31k | 8/8 | identical | found |

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

## quality: int4 vs bf16

greedy token agreement 18.3% · identical answers 4/13 · median top-1 margin at divergence 0.125 (max 1.125)

| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |
|---|---|---|---|
| a-short-chat | 15/256 | @15: ' model' → ' full' (margin 0.000, rank 1) |  |
| jql | 54/150 | @54: 'iss' → 'AND' (margin 0.000, rank 1) |  |
| confluence | 14/256 | @14: '##' → '-' (margin 0.250, rank 4) |  |
| python | 45/256 | @45: ' Args' → ' Raises' (margin 1.125, rank 2) |  |
| rust | 17/156 | @17: ' rare' → ' few' (margin 0.250, rank 2) |  |
| arithmetic | 0/76 | @0: '1' → '3' (margin 0.000, rank 4) |  |
| jira-admin | 19/219 | @19: ' so' → ' a' (margin 0.375, rank 2) |  |
| c-agent-turn1 | 38/38 | identical | tools = ['tree'] |
| c-agent-turn2 | 33/33 | identical | tools = ['shell'] |
| c-agent-turn3 | 39/39 | identical | tools = ['shell'] |
| c-agent-turn4 | 14/39 | @14: 'rg' → 'sed' (margin 0.000, rank 2) | tools ≠ ['shell'] |
| b-long-32k | 19/200 | @19: ' are' → ' appear' (margin 0.125, rank 2) |  |
| retrieval-31k | 8/8 | identical | found |

## quality summary

| config vs reference | agreement to first divergence | identical answers | median / max reference margin at divergence (nats) | fact at 31k | arithmetic (30) | agent tool calls equal |
|---|---|---|---|---|---|---|
| bf16-repeat vs bf16 | 45.3% | 5/13 | 0.125 / 0.25 | found | correct | 3/4 |
| int8 vs bf16 | 28.0% | 5/13 | 0.125 / 0.25 | found | WRONG | 3/4 |
| int4 vs bf16 | 18.3% | 4/13 | 0.125 / 1.125 | found | correct | 3/4 |

## memory and prefill (streaming, MTP on, no logprobs; one fresh engine per configuration)

Peak = the engine's Metal peak after the request (process-wide, contexts run in ascending order).
Prefix cache = the engine's retained entries after the request, within its fixed memory budget.

| config | context | prompt tok | TTFT s | prefill tok/s | peak GB | Δ peak vs bf16 | prefix cache after (entries, GB) | 128k answer |
|---|---|---|---|---|---|---|---|---|
| bf16 | 8192 | 8207 | 50 | 166 | 36.73 | +0.00 | 4, 3.0 |  |
| bf16 | 32768 | 32761 | 199 | 165 | 46.20 | +0.00 | 6, 8.4 |  |
| bf16 | 131072 | 130650 | 1070 | 122 | 67.25 | +0.00 | 1, 8.7 | KESTREL-4471 / Ioana Marinescu |
| int8 | 8192 | 8205 | 44 | 186 | 36.44 | -0.29 | 4, 2.4 |  |
| int8 | 32768 | 32759 | 190 | 172 | 41.34 | -4.86 | 6, 6.0 |  |
| int8 | 131072 | 130648 | 1144 | 114 | 57.74 | -9.51 | 3, 11.7 | KESTREL-4471 / Ioana Marinescu |
| int4 | 8192 | 8205 | 43 | 190 | 36.29 | -0.44 | 4, 2.2 |  |
| int4 | 32768 | 32759 | 183 | 179 | 40.47 | -5.73 | 6, 4.7 |  |
| int4 | 131072 | 130648 | 1031 | 127 | 54.14 | -13.11 | 8, 10.6 | KESTREL-4471 / Ioana Marinescu |

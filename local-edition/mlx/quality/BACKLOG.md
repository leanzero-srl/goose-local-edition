# MLX quality loop — BACKLOG (one row per open item; rewritten each tick; closed rows live in FINDINGS-LEDGER.md)

Updated 2026-09-26 19:1x. Ratio today: 29 fixed (Q-111..Q-137) · 8 proven live · 21 awaiting live prove · 2 cutting · 6 open.

## 1. Awaiting LIVE proof on the next build (prove in the named run, then mark "PROVEN LIVE <run>")
| id | what | proven by |
|---|---|---|
| Q-136 | split starts on the first try (readiness 503 + formation handshake) | split-start.mjs on 3.0.51 |
| Q-137 | Link connects over the tailnet while the Funnel is dead | Link tab + route shown on 3.0.51 |
| Q-114 | tensor split passes 10.5k generated tokens (buffer leak) | E2E #3c turn 0 / a 21k-token direct gen |
| Q-135 | split turn 0 ≈ Studio (no xhigh thinking) | E2E #3c turn 0 secs vs #4 (170 s) |
| Q-132 | checker runs reasoning-off, never blocks the next turn | E2E #3c calls.csv (checker rows) |
| Q-121/122 | split-stop notice survives relaunch; cut answer says split stopped | relaunch after any split stop |
| Q-112 | Run switch cancels the load it replaces | switch Studio→split during a restore |
| Q-115 | goosed disk writes ~0 under a real run | harness/diskio.py during E2E |
| Q-119/120/125 | Run it follows the picker; fits-once-stopped wording | walk Engine tab with Flash picked |
| Q-123/124/129 | one run history; chip = this chat only; 303 runs in the estimate | critic walk + tile after relaunch |
| Q-126 | no foreignEngines refusal from goose's own probes | pick model + Run at once |
| Q-127 | Flash rank cache ≈ budget − plan − scores | rank0 log guardrails line (seen 12.1 GB on 3.0.49) |
| Q-128/131 | chat follows the served model; engines answer every name of it | E2E #5b turn 0 + curl with the HF id |
| Q-133 | undeclared tool name → failed call named, model retries | E2E #5b (Flash) |
| Q-134 | pipeline admits a 2nd request mid-generation (decode −46% during join) | load.py 3 workers on Flash |
| Q-116/117/118 | runner self-update; keyless search; bundled MCP paths | seen live — re-check once on 3.0.51 |

## 2. Cutting now
| id | what | owner |
|---|---|---|
| Q-138 | orphaned bundled MCPs spin 100% forever; goose never reaps them | worktree agent |
| (CI) | leanzero-link tests flaky under parallel load | worktree agent |

## 3. Open (next to dispatch, in this order)
| id | what | next step |
|---|---|---|
| Q-130 | Add node names LM Studio; promises an engine repoint it doesn't do | desktop agent after Q-138 merges |
| Q-107 | tool deferral hides web search for minutes (E2E #4b) | E2E pair deferral on/off on the Studio route |
| Q-103 | short requests wait behind prefills on the single engine (MTP verifier one-at-a-time) | re-measure after Q-134's approach; fork design |
| Q-109 | reply-check step audit skipped once in 20 | replay with Q-132's reasoning-off checker |
| Q-110 | Studio single engine OOM under opened admission | only if admission is reopened |
| Q-19 | cache-size arms | only if a round shows cold calls |

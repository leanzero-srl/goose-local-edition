# E2E runs — the real-use loop (one row per turn per run; comparable numbers)

| run | build | config | brief | turn | secs | tools offered | agent calls: input / cache_read | helper labels (output tok) | deliverable |
|---|---|---|---|---|---|---|---|---|---|
| E2E #1 | 3.0.40 | split tensor, 141,568 window (a test engine held 30 GB at start) | jira-migration-readiness | 0 | 1220 | 79 | 28–55k; 2 of ~6 cold (49,292 / 0) | 100–219 (thinking) | kickoff.md ✓ all facts |
| E2E #1 | 3.0.40 | ″ | ″ | 1 | 507 | 79 | 55,977 cold mid-turn (Q-73) | 97–145 | 2 memories saved ✓ |
| E2E #1 | 3.0.40 | ″ | ″ | 2 | 3207 | 79 | 5 of 9 cold (62,154 / 0); a label ran to 3,383 thinking tokens | up to 3,383 | EOL dates + JCMA answer from atlassian.com ✓ |
| E2E #2 | 3.0.41 | split tensor, 262,144, GOOSE_TOOL_DEFERRAL | ″ | 0 | 551 (−55%) | 27 | 30–35k; first cold, then 29,295 / 33,319 / 33,890 / 34,489 reused (≈98%) | 5–7 (no thinking) | kickoff.md ✓ all facts |
| E2E #2b | 3.0.41 | Studio single over Link, 262,144, deferral | ″ | 0 | 300 | 27 (+2 load_tools) | 3 of 32 agent calls cold across turns 0–2 | — | kickoff.md ✓ |
| E2E #2b | 3.0.41 | ″ | ″ | 1 | 43 | ″ | ″ | — | 2 memories saved — to ~/.goose/memory/working-agents.txt (category "working-agents") |
| E2E #2b | 3.0.41 | ″ | ″ | 2 | 573 (split #1: 3207) | ″ | ″ | — | 4 web searches + 28 shell |

## Tool-call failure census (2026-09-25, sessions.db)
| run | engine | calls | failed | causes |
|---|---|---|---|---|
| E2E #1 | split (mlx_lm) | 23 | 3 | 2 × web page 404 (model-guessed URLs), 1 shell |
| E2E #2 | split (mlx_lm) | 11 | 2 | 2 × edit old_str mismatch (model; goose offered "Did you mean") |
| E2E #2b | Studio single (Rapid-MLX, qwen3_coder_xml parser) | 71 | 36 | ~22 `</parameter>\n!` appended to shell args + 3 × write "missing field `path`" (Q-85, engine parser) · cascades (files never written) · model: `cat -A` on macOS, analyze on .md |

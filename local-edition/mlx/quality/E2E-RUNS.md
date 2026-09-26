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
| E2E #3 | 3.0.45 | split tensor, 262,144, deferral | jira-migration-readiness | 0 | 1021 — split DIED mid-answer | 32 | first call 30,735 cold, 7k+ thinking tokens at 11.5 tok/s (E2E #2's same call: 3,367) | — | none: at 10:07:37 both ranks stalled (rank 0 R spinning in a collective, rank 1 S 0% CPU), the hang rule killed both at 10:07:57 (Q-114); TB/GPU/memory/build refuted |
| E2E #4 | 3.0.46 | Studio single over Link, 262,144, deferral | ″ | 0 | 170 (#2b: 300) | 27 | 31,552 / 32,428 cached | — | kickoff ✓ |
| E2E #4 | 3.0.46 | ″ | ″ | 1 | 81 (#2b: 43) | ″ | ″ | — | 2 memories saved (global ISO+British rule; project zero-npm rule) |
| E2E #4 | 3.0.46 | ″ | ″ | 2 | 1333 (#2b: 573) | ″ | 37,649 / 38,173 → 63,552 / 64,021 | — | 77 tool calls, no answer: web search returned empty results `serper-failed-no-fallback` (no key, browser engines off — Q-117); the model pip-installed a package and ran a 300 s find. Run stopped here for the 3.0.47 install (Q-115). Studio cache bounded 20.1/22.4 GB, footprint 45–66 GB |

## Tool-call failure census (2026-09-25, sessions.db)
| run | engine | calls | failed | causes |
|---|---|---|---|---|
| E2E #1 | split (mlx_lm) | 23 | 3 | 2 × web page 404 (model-guessed URLs), 1 shell |
| E2E #2 | split (mlx_lm) | 11 | 2 | 2 × edit old_str mismatch (model; goose offered "Did you mean") |
| E2E #2b | Studio single (Rapid-MLX, qwen3_coder_xml parser) | 71 | 36 | ~22 `</parameter>\n!` appended to shell args + 3 × write "missing field `path`" (Q-85, engine parser) · cascades (files never written) · model: `cat -A` on macOS, analyze on .md |

# E2E runs — the real-use loop (one row per turn per run; comparable numbers)

| run | build | config | brief | turn | secs | tools offered | agent calls: input / cache_read | helper labels (output tok) | deliverable |
|---|---|---|---|---|---|---|---|---|---|
| E2E #1 | 3.0.40 | split tensor, 141,568 window (a test engine held 30 GB at start) | jira-migration-readiness | 0 | 1220 | 79 | 28–55k; 2 of ~6 cold (49,292 / 0) | 100–219 (thinking) | kickoff.md ✓ all facts |
| E2E #1 | 3.0.40 | ″ | ″ | 1 | 507 | 79 | 55,977 cold mid-turn (Q-73) | 97–145 | 2 memories saved ✓ |
| E2E #1 | 3.0.40 | ″ | ″ | 2 | 3207 | 79 | 5 of 9 cold (62,154 / 0); a label ran to 3,383 thinking tokens | up to 3,383 | EOL dates + JCMA answer from atlassian.com ✓ |
| E2E #2 | 3.0.41 | split tensor, 262,144, GOOSE_TOOL_DEFERRAL | ″ | 0 | 551 (−55%) | 27 | 30–35k; first cold, then 29,295 / 33,319 / 33,890 / 34,489 reused (≈98%) | 5–7 (no thinking) | kickoff.md ✓ all facts |

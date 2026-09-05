# Archive — cloud sb-7 campaign harness and records (2026-08-23, from codex/salvage-benchmarks)

Preserved verbatim when the codex salvage branches were deleted on 2026-09-05. This is the harness that ran
sb-7 against cloud entrants (Kimi K3, MiniMax M3, Grok 4.6, Qwen 3.8 Max) plus the incident and audit notes of
that day. It is a RECORD, not wired into the current scorer: `cloud_sb7.py` imports modules as they were on
that branch. The Rust pieces that branch also carried (benchmark_guard, benchmark_budget, provider_lifecycle,
provider fixtures) were NOT taken — they modify providers the golden engine does not use. The branch tip is
tag `archive/codex-salvage-benchmarks`.

# HANDOFF — compaction-fix (2026-10-30)

Worktree ~/Projects/goose-wt-compaction, branch compaction-fix (pushed), base origin/main 4f25949111.
Commits as `git -c user.name=leanzero.srl -c user.email=office@leanzero.net`, NO AI trailers.

## Findings (evidence)
- Flash-Next ple.6 Gauntlet (runs/2026-10-29-flash-vision/attempt-2/mlx-engine/*.log): summary request = agent call 96/151,
  289,943 prompt (282,774 cached), same model, reasoning_effort medium, 4,316 out, 299 s. Growth 1.87k/call before,
  1.28k after; 55 calls left would need 360-394k -> compaction was needed anyway (~call 130). After: median out 344 vs 1,144.
- Stopped run summary (stopped-run-1/.../.openai-cloud-39b73921-...-runtime/goose/state/logs/llm_request.9.jsonl):
  model part = one line "I will now proceed to document the design decisions and run the final verification sequence."
  13 of the next 17 calls re-read docs/own code.

## Fix (crates/goose/src/context_mgmt/{mod,pillars}.rs, agents/{agent,compaction_run,moim}.rs)
1. compaction_threshold: limit - (max_output + limit/12), >= 0.8; GOOSE_AUTO_COMPACT_THRESHOLD wins;
   GOOSE_COMPACTION_RESERVE_TOKENS overrides; swarm workers: agent.worker_compaction_threshold (configured ratio).
2. WRITTEN_PARTS 5 sections; a non-summary is re-asked once on the same prefix, then transcript fallback.
3. Chat tail: tail_that_fits(kept_budget_chars), request placed before the tail; summary request sends all messages.
4. pillars named_documents: files under the chat folder the person names, non-hidden, loaded into leftover room.

## Gate
`~/Projects/machine-sync/bin/node-run --host workhorse --timeout 5400 -- <scratchpad>/cf/gate.sh` (fmt, clippy -p goose
-D warnings, cargo test -p goose --lib; CARGO_TARGET_DIR=~/Projects/goose-wt-mimo/target). Known unrelated flake:
providers::key_connection::tests::only_a_refused_loopback_connect_reads_as_a_server_that_is_not_started.

## Remaining
- Green gate -> squash-free final commit with trace -> push -> merge into main (check origin/main for flash-pld-bench first).
- UI AlertBox shows "Auto compact at 80%" when unset (now the floor, not the point): display gap, not fixed.
- Skill lines added: local-edition/skills/goose-mlx-inference/SKILL.md (this branch), ~/.claude/skills/goose-benchmark-iteration.

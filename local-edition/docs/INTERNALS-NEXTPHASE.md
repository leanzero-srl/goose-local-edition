# Verified internals & edit-points for next-phase work

Mapped + adversarially verified against Goose v1.39 source (workflow, 2026-06-25). Use these as the
implementation reference. File:line anchors are approximate to that checkout — re-confirm before editing.

## Subagent dispatch (the swarm substrate)
- `delegate(source, instructions, async:true, model:<id>, working_dir:<path>)` → spawns a subagent
  (`tokio::spawn`, cap `GOOSE_MAX_BACKGROUND_TASKS` default 5), bound to a PER-TASK provider/model
  (= a device via LM Link), isolated context, own `working_dir`. Returns result text OR an error string
  (e.g. "Server error: Model is unloaded") to the parent. `crates/goose/src/agents/platform_extensions/summon.rs`
  (sync handler ~1257-1304, async ~1801-1811; error returned to planner ~1300-1304).
- Provider auto-retries `ServerError` ~3x w/ backoff (`crates/goose-providers/src/retry.rs:87`) but does
  NOT respawn a dead subagent — re-dispatch is the caller's job.
- Typed plan: recipe `response.json_schema` → planner emits schema-validated JSON via `final_output`
  (`recipe/mod.rs`, `final_output_tool.rs`). PROVEN.

## Item 1 — Per-turn proactive compaction (VERDICT: SAFE TO DO NOW, medium risk, core change)
- Today proactive compaction runs ONCE before the loop (`agent.rs:1702`); inside the multi-turn loop it
  only compacts REACTIVELY on `ProviderError::ContextLengthExceeded` (`agent.rs:2334-2391`).
- INSERT a post-turn proactive check AFTER `conversation.extend(messages_to_add);` (`agent.rs:~2620`) and
  BEFORE the `if exit_chat` branch (`~2622`). Verified safe at THIS point only:
  - tool-pair summarization task is already joined via `.await` (~2569-2598) and works on its OWN
    `conversation.clone()` snapshot (taken ~1996) → no race/desync.
  - `update_session_metrics` (~2024) writes synchronously per chunk (`reply_parts.rs:545-552`) → a fresh
    `session_manager.get_session(&id, false)` reads the final persisted `usage.total_tokens`.
- Guard with the EXISTING `did_recovery_compact_this_iteration` (declared/reset per iter ~2005); skip if
  `exit_chat` or cancelled. Then `if check_if_compaction_needed(provider, &conversation, None, &check_session).await?`
  → yield Inline+Thinking notifications (mirror ~1731-1743) → `compact_messages(...)` → `replace_conversation`
  → `update_session_metrics(..., true)` → `conversation = compacted` → yield HistoryReplaced. On Err: yield
  message, do NOT break. `check_if_compaction_needed` already honors `GOOSE_LOCAL_CONTEXT_CAP` (mod.rs:219-222)
  + `GOOSE_AUTO_COMPACT_THRESHOLD`. Borrow pattern is the one already used at 2367/2375.

## Item — git-worktree write-isolation (HIGH risk, core change; for concurrent code edits)
- No worktree code exists today; every subagent inherits the parent `working_dir` and writes to the SAME fs.
- New `crates/goose/src/agents/worktree.rs`: `is_git_repo` (`git rev-parse --show-toplevel`),
  `create_worktree(repo_root,&id,ref)` (`git worktree add --detach <repo>/.goose/worktrees/<subagent_session_id> <HEAD>`),
  `remove_worktree` (`git worktree remove --force` + `git worktree prune`).
- Pin the ref to the parent's CURRENT HEAD at delegate time (resolve once in `build_task_config`) so
  subagent 2 isn't affected by subagent 1's merge. Integrate in summon.rs both handlers AFTER create_session,
  BEFORE run_subagent_task; if creation fails, FAIL the delegate (no silent fallback). Track the worktree
  path in `TaskConfig` (`subagent_task_config.rs:13-20`) / BackgroundTask.
- DANGER ZONE: async path — worktree must outlive the spawned future; remove in `cleanup_completed_tasks`
  (~1695-1744) AND on cancel/panic (guard); `git worktree prune` reaps orphans next run.
- Reducer (Phase B): a new recipe/subagent the planner invokes AFTER parallel worktrees finish; sequences
  merges/cherry-picks with a verify gate.

## Item — device pool robustness (LOW risk, NO core change) — stepping stone, see scheduler design
- Parameterize `swarm_v3.yaml` worker pool via a `worker_models` recipe param (CSV default = current 3 ids;
  rendered by `template_recipe.rs:92-115` BEFORE YAML parse → keep value a plain CSV, no quotes/colons).
- Planner DISCOVERY step: `curl -s http://localhost:1234/v1/models | jq -r '.data[].id'`; fall back to
  `{{ worker_models }}`. Planner-level RETRY: on a delegate result containing "Model is unloaded"/"Server error",
  wait ~5s and re-issue the same delegate once.
- `preload-swarm.sh` reads `SWARM_WORKER_MODELS` (same default) so pre-warm set + recipe pool share one source.
- NOTE: this is superseded by the weighted work-queue SCHEDULER (see scheduler design) for weights /
  work-stealing / locking / shared-context — the recipe approach is round-robin only.

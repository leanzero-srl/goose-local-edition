# Architecture

## Goal
A CLI coding agent for local AI on Apple Silicon: always-plan, split independent subtasks across
multiple local devices (swarm) via LM Studio LM Link, and keep only meaningful context for hybrid
MLX models (quality-first). Thin additive fork of Goose; track upstream tags.

## Fleet & transport (LM Link)
- 3 nodes linked via LM Link (Tailscale `tsnet`, single OpenAI endpoint `:1234` on the control node).
- A request for model-id X is transparently routed to whichever linked device holds X.
- MacBook M4 Max = control. workhorse M3 Ultra = `qwen/qwen3.6-27b`. mac.lan M3 Max 64GB = `qwen/qwen3.6-35b-a3b`.
- LM Link is whole-model task-parallel (NOT tensor-parallel; that is the separate JACCL cluster).

## Models → roles
- **27B dense** = planner / lead / hard-or-critical subtasks / final verifier (smartest).
- **35B-A3B MoE** = fast parallel workers (3–4× faster, ~3B active) for bulk/simple/parallel subtasks.

## Swarm topology: supervisor + map-reduce (rides Goose primitives)
1. **Plan (27B):** decompose into INDEPENDENT subtasks, each with file/region ownership, deps, and a
   difficulty/criticality tag. (Typed via recipe `response.json_schema`.)
2. **Route:** hard/critical → 27B; bulk/parallel → 35B-A3B. Device = `settings.goose_model` per sub-recipe.
3. **Dispatch:** independent subtasks → `delegate(source, instructions, async:true)` → parallel subagents
   (`tokio::spawn`, cap `GOOSE_MAX_BACKGROUND_TASKS`). Each subagent has isolated context + its own provider/model.
4. **Write-isolation (planned):** code-writing subtasks each get a git worktree (pin via subagent
   `TaskConfig.parent_working_dir`); read-only subtasks skip worktrees.
5. **Reduce (27B):** integrate, run a Verifier + CI gate, route conflicts/failures back for re-dispatch.

## Verified mechanism (Goose source v1.39, `~/Projects/goose`)
- Recipe schema: `crates/goose/src/recipe/mod.rs` → `Recipe{settings,response,sub_recipes,retry,parameters,extensions,instructions,prompt}`;
  `Settings{goose_provider,goose_model,temperature,max_turns}`; `Response{json_schema}`;
  `SubRecipe{name,path,values,sequential_when_repeated,description}`.
- Dispatch: Summon extension tools `delegate`/`load` (`crates/goose/src/agents/platform_extensions/summon.rs`);
  per-subagent provider/model from `resolve_provider`/`resolve_model_config`; applied via
  `crates/goose/src/agents/subagent_handler.rs update_provider`; per-task config in
  `crates/goose/src/agents/subagent_task_config.rs TaskConfig`.
- Planner decoupling: `GOOSE_PLANNER_PROVIDER/MODEL`, lead/worker. Context knobs:
  `GOOSE_AUTO_COMPACT_THRESHOLD`, `GOOSE_TOOL_CALL_CUTOFF`, `GOOSE_CONTEXT_STRATEGY`, editable `compaction.md`.

## Context strategy (single, quality-first, hybrid)
- Both targets are Gated-DeltaNet hybrids → ONE strategy (no pure-attention path).
- Keep context small + high-signal (repo map + agentic retrieval, not vector RAG); append-only;
  compact rarely + decisively (prefer a fresh small session over editing earlier turns, which desyncs
  hybrid recurrent state). Lean on LM Studio's hybrid-aware prefix cache (validated safe for tool-calling);
  do NOT build our own KV layer. The swarm (parallel small-context subagents) is the primary speed lever.

## Mapping to Claude-Code concepts (Goose already has these)
- Skills → Agent Skills (`SKILL.md`; dirs `~/.agents/skills/`, project `.agents/skills/`, plus `~/.claude/skills/` back-compat).
- Static memory → `.goosehints` (global + per-project). Dynamic memory → Memory extension (MCP).
- Subagents/orchestration → recipes + sub-recipes + Summon delegate.

## Fork posture
Thin additive fork: providers/skills/.goosehints/recipes/condenser are zero-core; the only core surface is
the typed-plan + difficulty router + reducer (+ optional thin `RemoteExecutor` trait), kept small. Pin to
release tags; strip the Electron desktop (`ui/`) for CLI-only.

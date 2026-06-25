# Key decisions & rationale

(Ranked by confidence/correctness risk, never by effort — per project convention.)

1. **Fork base = Goose** (Apache-2.0, LF/AAIF-governed, Rust, ~weekly releases). HIGH confidence.
   Only candidate that already had: planner/executor decoupling, native LM Studio provider, concurrent
   context-isolated subagents with per-subagent `Arc<dyn Provider>`, and Claude-Code-style skills/memory.
   Runners-up: OpenCode (TS, no-core-fork) / Forge (Rust, parallel) / Cline (Plan-Act, worktrees).

2. **Posture = thin additive fork, track upstream tags.** HIGH confidence. Keep our code additive
   (recipes/skills/providers/condenser); minimal isolated core surface (typed-plan + router + reducer).

3. **Engine = LM Studio (not raw mlx-lm/omlx).** HIGH confidence — it is the engine with a hybrid-aware
   prefix cache that keeps tool-calling intact on Qwen3.6 (validated). Also the only one with LM Link.

4. **Transport = LM Link single endpoint.** HIGH confidence (validated live). Device routing = model-id.
   Whole-model task-parallel, not tensor-parallel.

5. **Models = Qwen3.6-27B (planner/quality) + 35B-A3B (fast workers).** HIGH confidence. Both hybrids.

6. **Context = single quality-first hybrid strategy** (the "model-aware (both)" choice collapsed because
   there is no pure-attention model in scope). MEDIUM/LOWER confidence on the condenser details — this is
   the novel, bug-prone part (what to prune, when compaction is safe vs desyncs recurrent state).

7. **Swarm = pure recipe config where possible** (Summon delegate + per-sub-recipe `settings.goose_model`).
   HIGH confidence (PoC proven). Reducer / auto-merge of concurrent code edits is the LOWER-confidence part
   → gate behind git worktrees + CI/Verify + human review initially.

## Deferred (need user input when we get there)
- Project name (working name: `local-swarm-agent`).
- GitHub fork: create now vs later, account/org, public/private. (`gh` not yet installed.)

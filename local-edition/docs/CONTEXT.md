# Context optimization — quality-first, hybrid (pillar 3)

Goal: keep only *meaningful* context for the Qwen3.6 hybrids, prioritizing answer quality, and
compact only when it's safe. The binding constraints are NOT the window size (262K is huge) but:
(a) linear-attention (Gated DeltaNet) **recall degrades** as context bloats, and (b) **prefill cost**.
Both point to the same answer: keep context **small and high-signal**.

## Verified Goose hooks (source v1.39, `crates/goose/src/context_mgmt/mod.rs`, `agent.rs`)
- `manages_own_context()` default = **false** (`goose-providers/src/base.rs:548`); lmstudio inherits it →
  **Goose owns compaction** (we control it; not deferred to LM Studio).
- Auto-compaction: `check_if_compaction_needed` compares `total_tokens / context_limit` vs
  `GOOSE_AUTO_COMPACT_THRESHOLD` (default **0.8**). `compact_messages` = summarize-and-replace:
  old msgs → agent-invisible, a summary (rendered from `crates/goose/src/prompts/compaction.md`) is
  inserted, the latest user message is preserved; progressive middle-out tool-response removal on overflow.
- Tool-pair summarization: `maybe_summarize_tool_pairs` condenses OLD tool call/response pairs in batches
  of 10, protecting the last N (`GOOSE_TOOL_PAIR_SUMMARIZATION`, default on). Cutoff scales:
  `compute_tool_call_cutoff = (3 * limit*threshold / 20000).clamp(10,500)`.
- `get_context_limit` (`openai.rs:529`) uses `model_config.context_limit` if set, else probes/caches.
- **No repo-map / tree-sitter symbol map exists in Goose** → genuine new value to add.

## Strategy (single hybrid strategy — both targets are hybrids)
1. **Cap the EFFECTIVE context window well below 262K.** Set per-model `context_limit` to a lean working
   size (start ~32K, tune) so the agent is *forced* to stay high-signal and linear-attention recall stays
   sharp. We trade raw window for quality+speed deliberately. (262K is a safety ceiling, not a target.)
   - ⚠️ FINDING (runtime test #1): `GOOSE_CONTEXT_LIMIT` does NOT cap the lmstudio provider — it only sets a
     fallback; the provider's `get_context_limit` (openai.rs:529) returns the EXPLICIT `model_config.context_limit`
     (~200K from the canonical catalog/probe). So capping needs either overriding the canonical limit for
     qwen3.6 OR a small provider change: hard ceiling = `min(GOOSE_CONTEXT_LIMIT, probed)`. This is the first
     context change that touches the fork core. Until then, auto-compaction won't trigger in normal sessions.
2. **Repo-map (NEW).** Aider-style tree-sitter + PageRank symbol skeleton (~1K tokens), mtime-cached,
   injected at the FRONT (via `.goosehints` or a small extension). Highest-leverage quality lever for code.
   Ship initially as a skill/precompute that writes a `REPO_MAP.md` consumed via `.goosehints`.
3. **Agentic retrieval, not vector RAG.** Lean on grep/symbol-nav/read-precisely (developer extension) so
   only relevant code enters context; reads land as appended turns (cache-friendly).
4. **Compact reluctantly + decisively.** Keep `GOOSE_TOOL_PAIR_SUMMARIZATION=true` (condenses OLD tool
   outputs, keeps recent full). Set `GOOSE_AUTO_COMPACT_THRESHOLD` moderate (≈0.7) RELATIVE TO the capped
   window so full compaction is rare. When it fires, it's one decisive summarize-and-replace.
   - Hybrid note: compaction rewrites the prefix → a re-prefill. LM Studio's hybrid-aware engine handles
     correctness (validated: tool-calling survives), so compaction is SAFE, just not free. The win is
     avoiding it by staying lean.
5. **Durable facts in `.goosehints` / Memory extension**, never only in a volatile summary
   (`compaction.md` tuned to preserve decisions, constraints, conventions, and a file index).
6. **Swarm = the primary speed lever.** Each subagent gets its own small isolated context; parallel
   small-context workers beat one giant long-context session on a linear-attention model.

## Implementation plan (additive)
- Phase A (config only, testable now): set lean `context_limit` per model; set
  `GOOSE_AUTO_COMPACT_THRESHOLD`, keep tool-pair summarization on; tune `compaction.md` for quality.
- Phase B (skill): `repo-map` skill that precomputes a tree-sitter symbol map → `REPO_MAP.md` →
  referenced from `.goosehints`. (tree-sitter grammars; reuse Aider's tags.scm approach.)
- Phase C (optional core extension): a condenser variant that prunes oldest tool outputs more
  aggressively for hybrids while never editing the recent window.

## Validated (runtime, 2026-06-25)
- ✅ **Compaction-safety**: forcing `GOOSE_AUTO_COMPACT_THRESHOLD=0.0001` fired compaction 4× on the 35B and
  the task still completed correctly → tool-calling survives the post-compaction re-prefill on the hybrid.
- ✅ Threshold env is honored; effective `context_limit` is ~1M (YaRN, probed) so normal sessions never
  compact → the **window cap is required** to get lean context + sane compaction triggering.

## Implemented / next
- DONE: effective-window cap `GOOSE_LOCAL_CONTEXT_CAP` = `min(configured, probed)`, in the provider
  (`openai.rs::get_context_limit`) AND `check_if_compaction_needed`. VERIFIED the check sees the
  capped limit (debug print: `context_limit=3000`). Additive; no-op when unset.
- KEY FOLLOW-UP: Goose checks PROACTIVE auto-compaction only ONCE per `goose run` (at reply-start,
  before the tool-call loop grows context), so the cap doesn't drive mid-run compaction yet. Add a
  PER-TURN proactive check in the reply loop (`agent.rs`, around the tool-call iteration) gated by
  `check_if_compaction_needed`. Only reactive compaction (true model-window overflow) fires in-loop today.
- Then add the Aider-style repo-map (genuine new value; Goose has none).

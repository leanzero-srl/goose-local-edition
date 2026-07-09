# Evolve — 3 Goals: verified understanding + design (2026-07-09)

Source: 17-agent workflow `goose-evolve-understand` (wf_ddbef7a6-833). Every load-bearing claim
re-verified in-tree. Completeness critic verdict: **ARE WE GOOD? False** — two designs are
ship-ready at HIGH confidence; the third (self-built tools) is unproven and needs a spike.

## GOAL 1 — Evolvable tunables + one-click "Golden formula" preset — HIGH confidence

Tunables live in THREE non-overlapping layers:
1. `SwarmConfig` (config.yaml `swarm:` key) — ~28 fields, loaded/saved at swarm.rs:292/298, every field `#[serde(default)]`. Desktop panel surfaces ~17.
2. ~38 runtime-only `GOOSE_SWARM_*` env flags — pipeline gates + caps, resolved via `swarm_gate`/`resolve_gate` (swarm.rs:7597/7606). Exposed in NO UI. `GOOSE_SWARM_ASSURED` (swarm.rs:7582) flips the whole reliability bundle ON.
3. `GOOSE_LOCAL_*` — SwarmConfig SEEDS these at run start: context_cap→GOOSE_LOCAL_CONTEXT_CAP (:8813), endpoint→LMSTUDIO_HOST (:8811), max_tool_response_chars→GOOSE_MAX_TOOL_RESPONSE_SIZE (:8822).

Precedence: BEHAVIOR gates = explicit env > ASSURED bundle > default; CONFIG fields = per-run CLI arg > config.yaml > serde default. Desktop `useConfig().upsert` → `Config::global().set_param` → same global config.yaml the CLI reads (GLOBAL, not per-session).

**Real bug found:** panel `DEFAULTS.worker_timeout_secs = 420` but Rust default = 900 (swarm.rs:189). The 420 is a golden value, not the panel baseline.

**~11 SwarmConfig fields absent from the desktop panel:** planner_weight, max_replans, research_scouts, planner_timeout_secs, min_p, max_tool_response_chars, scout_budget_secs, homogeneous_models, speed_weights, worker_extensions, devices(edit).

**The golden formula** = `GOOSE_SWARM_ASSURED=1` (bundle: COMPLETE/GOALS/CONTRACTS/REVIEW/REVIEW_FANOUT/REVIEW_VERIFY/SINK_REVIEW/SMOKE/PARALLEL_TESTS) + `GOOSE_SWARM_REVIEW_REPRO=1` + `GOOSE_SWARM_REVIEW_FIX=1` (the wrapper's env) + the config: worker_timeout_secs 420, worker_max_turns 40, max_attempts 3, research_planning on, dynamic_replan on, parallel_planning on, best_of_n 1, allow_model_load false, endpoint localhost:1234, GOOSE_MODEL qwopus3.6-27b-coder → warm fleet.

**Design:** frontend-only for the config half. Define `const GOLDEN: Partial<SwarmConfig>` (golden.ts), a preset selector (custom control) that MERGES over prev (never naked-replace — would wipe devices[]/speed_weights). First increment (HIGH, zero backend): Wave-1 widen + 420→900 fix + config-half Golden preset. The gate half needs one Rust change (persist assured/review_repro/review_fix as config with a 4-way precedence unit test) — the one MEDIUM piece.

## GOAL 2 — Recipes vs loops — HIGH on the answer

**Three distinct concepts, not two:**
- RECIPE (recipe/mod.rs:43) = declarative ONE-SHOT session config (instructions/prompt, extensions, params, response schema, sub_recipes). NO control flow, NO carried state. Its only "loop" = bounded STATELESS retry that wipes conversation to initial_messages each attempt (retry.rs:104).
- SCHEDULE (scheduler.rs:105 ScheduledJob = recipe path + cron + params) = recurrence, but each fire spins a FRESH memoryless `Agent::new()` (scheduler.rs:875).
- LOOP = a genuine 3rd concept = iteration STATE threaded forward + a stop/convergence predicate + memory across iterations. Does NOT exist natively.

**Are loops recipes?** NO.
**Can goose author its own recipes via AI? YES — already partly exists (CONFIRMED):** `/recipe [filepath]` (input.rs:319) → `agent.create_recipe(session_id, messages)` (agent.rs:3107) runs `provider.complete(...)` over the conversation + prompts/recipe.md, parses model JSON → Recipe → saves YAML. First increment: expose to desktop via a new `on_create_recipe` ACP handler + "Make recipe from this session" chat action feeding CreateEditRecipeModal's existing `recipe` prop.
**Best way to build loops (the native ladder — critic CORRECTED the map here):**
1. `/grind` + `/goal` — NATIVE within-session self-driving continuation (agent.rs:259 grind field, 2568-2588 "keep working until fully done", set_goal/set_grind 2792/2800). The map WRONGLY said "the only loop is retry" — this is the highest-confidence native loop.
2. Recipe + cron + a Shell `SuccessCheck` as CONTINUE/STOP gate.
3. External Claude-Code harness (the proven evolve-loop we run now — multi-session, state-carrying).
**Loop section?** Warranted as a 3rd concept IF we build a native Loop — but there's a DECISION GATE first: native Loop vs keep the proven external harness.

## GOAL 3 — Builder vs Agent modes + self-learning + self-built tools — SPLIT confidence

**Mode arch (HIGH):** Builder|Agent = a NEW orthogonal axis, NOT a 5th GooseMode variant (GooseMode is exhaustively matched in 10+ ACP sites — a 5th variant explodes edits). Copy the `code_execution_mode` precedent (the one mode that both threads through SystemPromptBuilder reply_parts.rs:233 AND rewrites the system prompt system.md:8). Desktop toggle = a verbatim clone of EditionContext/EditionSelector.

**Self-learning loop (MEDIUM):** 3-phase, state lives in memory/skill FILES on disk (no native evolve loop). Substrate is real (memory extension writes categorized memories) but 3 concrete failure points: (a) confirm-before-saving contradiction; (b) global memories are preloaded ONE-SHOT at memory-server construction (memory/mod.rs:115) so mid-session writes don't re-enter standing context without a restart (moim/tom.rs re-injection is plausible but unwired); (c) NO convergence/stop predicate — an evolving run goes forever until toggled off.

**Self-built tools (THE FRONTIER — PARTIAL / LOW confidence on robust+safe first pass):**
- What EXISTS: `create_app` (apps.rs:236) lets the model author + SAVE HTML apps to disk (apps.rs:178, reused across sessions) — but the artifact is an APP (UI window), NOT a callable tool. `execute_typescript` (code_execution) runs model-authored TS ephemerally — result is text, code NOT saved.
- The GAP = ONE missing glue tool: `create_tool` = ExtensionConfig::InlinePython{code} (extension.rs:286) → add_extension (model-reachable, manage_extensions proves it) → config::set_extension (persists, auto-loads next session). Every individual call verified present; the ROUND-TRIP has NEVER been exercised.
- 4 UNVERIFIED gates: extension_malware_check.rs / tool_confirmation_router.rs / validate_extensions.rs behavior on SELF-authored code; and uvx/uv presence on the LM-Studio host (InlinePython launches via `uvx --with mcp python` — silent launch failure is a live risk).
- SECURITY: persisting auto-loading executable code in config.yaml that runs as an MCP subprocess every session is a materially different trust boundary than a sandboxed app — needs default-off, confirmation-gated, project-scoped contract (undesigned).

## Completeness critic — "are we good?" = NOT YET. To become good:
1. **Run the self-built-tool spike END-TO-END** (the single load-bearing unknown): Agent-mode CLI session → model authors a trivial InlinePython tool → add_extension registers live → call it this session → config::set_extension persists → a NEW session sees + calls it. Probe the 4 gates + a malformed-server failure to size the build-verify retry loop. Until this round-trips, Goal 3's headline is unproven.
2. **Re-read agent.rs goal/grind (2477-2588)** + correct the loops answer (done above — the native ladder).
3. **Unit-test the 4-way resolve_gate precedence** (explicit-env > config > assured&&bundle > default; explicit-off beats config-on) + enumerate the ~25 gate call sites before Goal 1's Rust gate-persistence.
4. **Re-open trusted-from-map seams** (Goal 2 recipe DTO/RPC/modal; Goal 1 desktop upsert→set_param chain).
5. **Define the "evolve" primitives** (stop predicate = reuse SuccessCheck::Shell; memory eviction guard; mid-session memory re-injection via moim/tom.rs) — none exist.
6. **Design the trust contract** for persisted InlinePython + the Agent-view config schema (target/voice/memory-scope/tools — no desktop analog today).

## Recommended sequencing (ranked by confidence, NOT effort)
- **SHIP NOW (HIGH, zero/low backend):** Goal 1 config-half Golden preset + tunable widening + 420→900 fix; Goal 2 Step 1 (expose AI recipe authoring to desktop).
- **VERIFY-THEN-SHIP (MEDIUM):** Goal 1 Rust gate-persistence (precedence unit test first); Goal 2 native Loop (state-threading A/B) — gated on the native-vs-external decision.
- **SPIKE-FIRST (LOW, no UI until proven):** Goal 3 — land the AgentMode spine + a `create_tool` prototype, run the end-to-end spike probing the 4 gates. Only then build the Agent view.

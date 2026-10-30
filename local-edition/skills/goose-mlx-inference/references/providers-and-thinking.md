# Providers and thinking controls

## Contents
- The Swarm provider and the provider surface (2026-09-05, owner's rule)
- Thinking controls (2026-09-23 — per model, OFF by default = send nothing)


Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## The Swarm provider and the provider surface (2026-09-05, owner's rule)
- **Only the defined providers exist in the local edition:** Goose Swarm (`swarm`) plus the swarm's four cloud
  families by REGISTRY id (aws_bedrock, zai, google, custom_deepseek). One allow-list, `LOCAL_EDITION_PROVIDER_IDS` in
  `ui/desktop/src/components/leanzero-swarm/cloudProviders.ts`, derived from CLOUD_PROVIDERS so it cannot drift; applied to the
  model picker, onboarding, /configure-providers and the hub's Cloud Providers tab. omlx/lmstudio stay REGISTERED in Rust (the
  sidecar path needs omlx) but are never offered; an active omlx/lmstudio provider is migrated once, loudly, to swarm. The edition
  defaults to LOCAL with nothing persisted — this fork IS Goose Swarm.
- **`swarm` has two model ids:** `swarm` = CHAT — the turn is served by an idle node of the configured pool through
  `crates/goose/src/providers/swarm_router.rs` (process-wide idle guard: pool re-read every turn; capacity = instances or the
  sidecar admission cap `goose_sidecar::engine::MAX_CONCURRENT_REQUESTS`; free = cap − max(leases, live in-flight); sticky per
  conversation, else most-free, else QUEUE with no timeout; zero servable → a named error; admission 503 → next node; the permit
  rides the stream; context limit = the pool's minimum window). `swarm-build` = the brief → `goose swarm run` (the run panel path,
  unchanged). A mesh PEER's sidecar is not a node (loopback-bound; the Link mlx proxy is the only door) — LM Link fans LM Studio
  models across machines. Never add a clock to the queue (gate 5); never let a probe failure read as "idle".

## Thinking controls (2026-09-23 — per model, OFF by default = send nothing)
- WHY: Rapid-MLX turns thinking OFF on every tool-bearing request (`service/helpers.py`
  `maybe_auto_disable_thinking_for_tools`) unless the request pins `chat_template_kwargs.enable_thinking`,
  top-level `reasoning_effort`/`reasoning_max_tokens`, or `tool_choice:"none"`. NOTE: `chat_template_kwargs.reasoning_effort`
  alone does NOT stand the auto-disable down — under Auto with tools the effort level is inert (Qwen3.8 only reads it
  while thinking is on).
- HISTORY REASONING WAS DROPPED (found 2026-10-15, T12 bake-off; fix `lz/reasoning-history` 1470c7834 on top of 26452cbe3).
  The engine's chat `Message` model had no `reasoning_content` field, so pydantic silently dropped the reasoning goose
  sends back on every assistant row. Qwen3.8's template has `preserve_thinking` on by default and renders EVERY history row
  as `<think>\n{reasoning_content}\n</think>`, so every past turn showed the model an EMPTY think block: a history in which
  it never reasoned. Every round-1 bake-off run (AREX-2, pi, JetBrains-blend, Smaug-Mini) ran under this; all stopped short.
  The `/v1/messages` adapter dropped echoed `thinking` blocks the same way (also fixed). `/v1/responses` reasoning items
  stay dropped on purpose (encrypted_content is opaque).
  - Measured, not assumed: `R12-T1F` (the tool-request auto-disable) fired 0 times in the bake-off engine logs, because
    goose pins `reasoning_effort=medium`. So thinking WAS on; only the history was hollow.
  - How to check any engine: `Message(role='assistant', content='x', reasoning_content='T').model_dump()` must contain
    `reasoning_content`. The proof is `tests/test_history_reasoning_reaches_template.py` (stock Qwen3.8 template fixture
    md5 519239a4); 5 of its 7 cases fail on 26452cbe3.
  - Side effect to expect: history now carries the reasoning, so prompts grow faster per turn (the intended Qwen3.8 mode),
    and the rendered history finally matches what was generated (better prefix-cache hits on the last turn).
  - HARDENED in 645843445 after an independent review (FIX-FIRST, 5 defects). Rules that must survive:
    reasoning is CLIENT TEXT, so the role-marker sanitiser covers `_CLIENT_TEXT_FIELDS` (content, reasoning_content,
    reasoning); the Qwen3.5 retain pass re-inserts think text after the render, so it must read the SANITISED list;
    non-text or blank reasoning is ABSENT (never a 422, never `''`, which stops Qwen3.5 splitting `<think>` out of
    content); `/v1/responses` reasoning items carry plain `summary_text` here and go onto the next assistant row.
    Test file now 14 cases (9 fail on 26452cbe3); the 6 failures in the 276-file related suite are pre-existing.
  - Live proof on Gabee (2026-10-15 15:33): the same 2-turn request = 496 prompt tokens with and without history
    reasoning on 26452cbe3, 496 / 549 on 1470c7834. Re-run that probe (`~/t12-bench/rh_probe.py <port> <model>`) after
    any engine bump.
- SPLIT TURNS - THE LIKELY STOP-SHORT CAUSE (found 2026-10-15 17:00; goose 8ef97235e on t12/local-bench-fidelity,
  engine e84498a4a on `lz/reasoning-history`). goose stores one streamed response as TWO assistant messages (the text,
  then the tool call carrying the reasoning) and the OpenAI formatter sent them as two rows. On Qwen3.8 every past turn
  rendered as `<think></think>Now let me write X:<|im_end|>` + a second turn `<think>R</think><tool_call>`: the model
  saw its narration END a turn on every turn, and every T12 stop-short (pi, Smaug, AREX-2, JetBrains-blend x3) ends
  exactly that way: narration with a colon, then end of turn, 30-90 tokens, finished normally, no call.
  - goose fix: `merge_narration_into_tool_call` in goose-provider-types formats/openai.rs (after the parallel-call
    merge). Needs a NEW APP BUILD to reach Gabee.
  - engine fix (covers the installed 3.0.111 and any client): `_join_split_assistant_turns` at the top of
    `apply_chat_template`, GATED per template by `_template_puts_narration_before_calls` (one probe render, cached):
    Qwen3.5/3.8, Qwen3-Coder-Next, Nemotron join; Gemma 4 (text renders after the tool result) and Llama 3.2 (drops
    tool-row text) stay split. Reasoning copied onto both rows is kept ONCE; parallel bare calls fold into the turn.
    BLANK ROWS JOIN TOO (f39d1d92, 2026-10-15 22:2x): goose 3.0.111 sends most calls as `{"content":"\n\n"}` then the
    call row - 40/40 calls on the control. The first version skipped blank narration, so every call still rendered
    after an EMPTY finished turn; join_probe checks both shapes now (blank split must equal blank joined, 489 = 489).
  - Live proof: `~/t12-bench/join_probe.py <port> <model>` - split and joined requests must have EQUAL prompt_tokens
    (494 = 494 on e84498a4a). Copies of both probes: `~/goose-builds/loop-state/t12-memguard/`.
  - How to see it in any run: read the request log (`.<run>-runtime/goose/state/logs/llm_request.*.jsonl`, the
    `input.messages`) - consecutive assistant rows, the first text-only, the second `content: null` + tool_calls.
- THOUGHT SPILLED INTO THE ANSWER AFTER 64 CHARS (found 2026-10-15 20:40; fix 1481b118f on `lz/reasoning-history`,
  in RC6 as 012a4ca08). goose passes `--reasoning-parser deepseek_r1` for Qwen3.x. Qwen3.8's template opens `<think>\n`
  in the generation prompt, so the stream has no opening tag; `DeepSeekR1ReasoningParser` had no `configure_request`,
  never learned the prompt was primed, and its 64-char `NO_TAG_CONTENT_THRESHOLD` routed everything after char 64 to
  CONTENT. Every turn of every T12 run: ~60 chars of reasoning, then the rest of the thought as the visible answer
  (control: 16 thinking deltas, 10,178 text deltas, split mid-number "is at 7" | "9%"). This is the "plans whole files
  in prose" symptom. Fix: the base parser takes `configure_request(prompt_thinking_active)` with the RENDERED priming
  answer (`_should_start_in_thinking(rendered=True)`: does the prompt END inside an open think block; explicit
  enable_thinking=False still wins).
  - Live proof: `~/t12-bench/think_probe.py <port> <model>` (copy in loop-state/t12-memguard): reasoning must be far
    over 64 chars and content only the answer (1481b118: 424 / 85, tool call emitted).
  - How to see it in any run: count the streamed `thinking` vs `text` items in `llm_request.*.jsonl`; a thought that
    stops at ~60 chars mid-sentence and continues in text is this bug.
- Capability record: `goose_sidecar::thinking::model_thinking_capabilities(dir)` parses the tool-use template ONCE per
  source (minijinja `unstable_machinery` AST, a port of rapid-mlx `detect_native_reasoning_effort_levels`). ORACLE for any
  change: run the engine's own function on the fixture — `~/.cache/uv/archive-v0/<hash>/bin/python -c "from
  rapid_mlx.utils.chat_template import detect_native_reasoning_effort_levels as d; print(d(open(F).read()))"`.
  Qwen3.8 = switch enable_thinking, levels xhigh/medium/low, default xhigh, preserve_thinking.
- Profile: `ModelProfile.thinking` (None=auto | on | off), `.reasoning_effort` (None=template default); no argv effect.
  Wire: profile `thinking`/`reasoningEffort`, modelsList `thinking{thinkingSwitch,effortLevels,defaultEffort,
  preserveThinking,budgetForcible}` / `thinkingError`.
- Request seam: `swarm_router::route_stream`, MlxSidecar nodes only, via `request_params.chat_template_kwargs`;
  captured per SESSION (SwarmProvider field) so a mid-session edit never voids the prefix cache. The bare `omlx`
  provider and goose-cli swarm lanes do NOT get it. Defaults = byte-identical request (test
  `default_choices_leave_the_mlx_request_byte_identical`).
- AUTO MUST MEAN THE SAME ON EVERY WAY (Q-135, 2026-09-26). Only Rapid-MLX has the auto-disable; mlx_lm.server
  (tensor) and the fork's `pipeline_qwen4 serve` hand an absent switch to the template, and Qwen3.8 reads
  undefined as ON at effort xhigh (+209-char "Reasoning effort is set to xhigh…" system line, `<think>\n` instead
  of `<think>\n\n</think>\n\n`). Measured: split turn 0 of the Jira brief 1021 s / 1195+ s, first agent call 7k /
  10.4k+ thinking tokens; single 170–178 s, 65–817 tokens, none thinking. Fix: `distributed/rank_thinking.py`
  (port of chat.py's effort-none → tools gate → casual gate → `_extract_thinking_from_request`), applied by
  rank_wrapper.py (`validate_model_parameters`, rank 0, before the request is shared) and pipeline_rank.py (goose's
  /v1/chat/completions over the fork's). Top-level graded `reasoning_effort` / `reasoning_max_tokens` /
  `reasoning{effort}` are REFUSED 400 `unsupported_parameter` on the split (the single engine translates them).
  Proof: `every_way_renders_the_same_prompt_for_the_same_setting` (launch.rs, pipeline env). Offline render
  recipe: tokenizer from the model dir on CPU (transformers, no weights) + `git archive <tag> rapid_mlx`.

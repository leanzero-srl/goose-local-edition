# HANDOFF: n-gram / prompt-lookup decoding lever (branch ngram-decoding)

Updated 2026-10-26 by the second agent (stopped at the 160k ctx-guard; the CLAUDE.md/AGENTS.md
reminders this repo injects cost ~60k on their own, so budget for that).

## TRAP (cost the first attempt)
Do NOT load the skills goose-mlx-inference / goose-feature-dev / node-dispatch with the Skill tool.
`grep -n` them: ~/.claude/skills/<name>/SKILL.md. Never cat engine.rs (5400 lines): grep -n + sed -n.

## DONE (commit 7e360e4223, step A Rust core)
- NEW crates/goose-sidecar/src/prompt_lookup.rs: `PromptLookupMode {On,Off}` (auto = None),
  `PromptLookupProfile {mode, min_ngram, max_ngram, max_draft}` (serde default, snake_case),
  `PromptLookupLane {Mtp, Suffix, Unavailable(reason)}`, `lane()`, `model_is_hybrid(dir)` (config.json:
  layer_types with linear_attention/mamba/ssm..., full_attention_interval>1, or model_type in a hybrid
  list incl. qwen3_5/qwen3_next/granitemoehybrid/nemotron_h/lfm2), `engine_env()` (MTP lane only:
  RAPID_MLX_MTP_PROMPT_LOOKUP=1/0 + _MIN_NGRAM(clamped>=2) _MAX_NGRAM(>=min) _MAX_TOKENS; Off sends only
  =0), `engine_flags()` (Suffix lane + On only: --suffix-decoding, --suffix-max-draft <max_draft>,
  --suffix-max-suffix-len <max_ngram>; min_ngram has no suffix-lane meaning). 10 unit tests.
- engine.rs: `ModelProfile.prompt_lookup: PromptLookupProfile` (non-Option, default = auto);
  `serves_mtp()` extracted from build_serve_command (same logic); build_serve_command appends
  engine_flags when not auto; `pub fn prompt_lookup_lane(settings, model_id)`;
  `pub fn build_serve_env(settings, model_id)`; `ManagerState::Running` carries `env`; mount reuse
  compares argv AND env; config.env = sidecar_spawn_env() + serve env; restart_required compares both.
  Test `prompt_lookup_rides_env_on_mtp_flags_on_pure_attention_and_nothing_on_a_hybrid`.
- Gate at 7e360e4223: `cargo clippy -p goose-sidecar --all-targets -D warnings` clean;
  `cargo test -p goose-sidecar` all green (lib 499 passed, 12 ignored; integration suites ok).

## OPEN
A2. ACP wire (not started):
  - crates/goose-sdk-types/src/custom_requests.rs `MlxModelProfileDto` (~line 2335): add
    `prompt_lookup: Option<MlxPromptLookupDto>` (camelCase: promptLookup {mode:"on"|"off", minNgram,
    maxNgram, maxDraft}), skip_serializing_if none. `MlxLocalModelDto` (~3010): add
    `prompt_lookup_lane: Option<String>` ("mtp"|"suffix"|"unavailable") + `prompt_lookup_unavailable_reason`.
    NOTE the lane depends on profile.speculative (MTP off -> suffix/unavailable), so the UI should
    recompute: speculative!="off" && hasMtpHead -> mtp; else hybrid? unavailable : suffix. Simplest: send
    `hasMtpHead: bool` + `hybrid: Option<bool>` + `hybridError` and compute lane in TS, mirroring `lane()`.
  - crates/goose/src/acp/server/mlx_engine.rs profile_to_dto (~345) / profile_from_dto (~416) map it;
    core_models_list (~870) fills the facts (inspect_model_dir(&dir).has_mtp_sidecar,
    prompt_lookup::model_is_hybrid(&dir)).
  - Regenerate: `cargo run -p goose --bin generate-acp-schema` then `cd ui/desktop && pnpm run build-goose-sdk`.
B. UI: ui/desktop/src/components/leanzero-swarm/MlxKvCacheFields.tsx is the pattern (used from
   MlxEngineView.tsx; types in ui/desktop/src/acp/mlx-engine.ts ~line 191 `kvCache?`). Add
   MlxPromptLookupFields.tsx: mode dropdown Auto/On/Off (custom DropdownMenu, NO native select), three
   number inputs (min n-gram MTP-only, max n-gram, max draft; hint 31/63/95 fill 32-row verify tiles),
   lane caption ("inside MTP", "suffix decoding, greedy only, batch 1", or disabled + reason). Vitest
   beside MlxEngineView.test.tsx. i18n keys in ALL 15 catalogs (round-trip JSON in file order,
   ensure_ascii=False, indent=2). Gate: pnpm install --frozen-lockfile; pnpm run lint:check; pnpm run test:run.
C. Measurement DONE (data + NOTES.md: /Volumes/AI-workhorse/runs/2026-10-26-pld-qwen38). Qwen3.8-27B-Q8,
   lz.14, greedy, N=3, decode tok/s median (client: (tokens-1)/first-to-last chunk):
   code_edit  MTP off 21.4 | MTP+PLD off 42.8 (MTP acc 85.7%) | MTP+PLD on 134.8 (PLD 1926/1940 = 99.3%) -> 3.15x
   doc_quote  22.1 | 33.3 | 32.4 (PLD 64/123 = 52%) -> no gain
   free_chat  22.0 | 37.3 | 37.1 (PLD never proposes) -> no gain
   PLD=1 explicit == default (family default ON). SHA: code_edit all 12 outputs identical across all
   configs; doc_quote/free_chat NOT identical, including repeat-to-repeat INSIDE MTP+PLD-off and a
   free_chat run where PLD drafted 0 -> nondeterminism is MTP's (adaptive K), not PLD's (cause unproven).
   Engine bug seen: streamed output drops the space before " [" / " &" tokens (4x per code_edit,
   all configs; non-stream correct on Mellum build) -> file for the engine owner.
   Mellum2.1-12B (lz/mellum-lz12 435781119): --suffix-decoding SLOWER (90.4 vs 99.5, 99.7 vs 109.4),
   6660 "non_trimmable_cache" skips, 8/96 accepted, free_chat output diverges -> suffix lane is not
   lossless/useful there; consider showing a warning or keeping it off by default (it is: auto = off).
D. Skill notes: local-edition/skills/goose-mlx-inference/references/prompt-lookup-ngram.md (+ one-line
   pointer in that SKILL.md) and a dated changelog line in ~/.claude/skills/goose-knob-turning/SKILL.md.

## Engine facts (verified in /Users/workhorse/Projects/Rapid-MLX-wt-offload @ v0.14.3-lz.14, READ-ONLY)
- spec_decode/mtp/generator.py `_effective_prompt_lookup_policy`: env read per request; MIN>=2,
  MAX>=MIN, MAX_TOKENS>=1; `_SAMPLED` is disable-only. `_prompt_lookup_is_enabled` needs
  model.mtp_prompt_lookup_supported (qwen3_5_inject.py and qwen4_exp_inject.py set it).
- qwen3_5_inject.py ~1009: family policy enabled_by_default=True, enabled_under_sampling=True,
  min 8, max 64, max_tokens MAX_COPY_DRAFT_TOKENS (=31, prompt_lookup.py:175). Legacy default: off.
- Suffix: cli.py ~12417 flags (store_true, defaults None -> drafter defaults max_draft 8, max_suffix_len 4,
  min_confidence 0.3, min_draft_len 2); scheduler.py `_install_suffix_decoding` ~3418 logs
  "[SuffixDecoding] disabled: model is hybrid" and returns (no crash) when !supports_spec_decode;
  greedy-only, batch 1. Status counters: prompt_lookup_{proposals,drafted_tokens,accepted_tokens,
  rejections,ev_declines,cache_fallthroughs,matched_suffix_tokens,mtp_sync_seconds}.

## Unverified / risks
- goose-side hybrid detection is a heuristic list; the engine's own `supports_spec_decode` is the authority
  (it degrades silently-with-a-log, not by refusing). Distributed / remote-single engines do NOT get the env.

## Rules
`source bin/activate-hermit`, then `export CARGO_TARGET_DIR=/Users/workhorse/Projects/goose-wt-ngram/target`.
Commits: `git -c user.name=leanzero.srl -c user.email=office@leanzero.net commit`, NO Co-Authored-By.
Push: `git push -u origin HEAD:ngram-decoding`. Port 8471 only; never pkill by pattern; heavy runs via
~/Projects/machine-sync/bin/node-run (workhorse or gabee, not workmule).

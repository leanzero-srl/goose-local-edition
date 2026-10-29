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

## DONE 2026-10-26 (third agent)
- A2 ACP wire 4bc16dcddd: MlxModelProfileDto.promptLookup (MlxPromptLookupDto, auto = absent),
  MlxLocalModelDto.hasMtpHead/hybrid/hybridError; profile_to/from_dto + core_models_list; acp-schema +
  ui/sdk generated regenerated; round-trip test in mlx_engine.rs tests.
- B UI b16fa1a3d4: MlxPromptLookupFields.tsx (+ .test.tsx), Segmented Auto/On/Off, lane chip, numbers
  (shortest n-gram MTP only), hybrid-without-MTP locked with reason, Auto reachable; NumericField exported
  (spec key widened); drafts promptLookup* in MlxEngineView (draftsEqual uses DRAFT_KEYS); i18n 16 catalogs.
  Gate: pnpm lint:check (typecheck+eslint+i18n) green, pnpm test:run 555 files / 5195 tests green.
- D skill bf9e236db0: references/prompt-lookup-ngram.md + SKILL.md pointer; ~/.claude/skills/goose-knob-turning
  changelog line (not git; projsync).
- TRAP: workhorse is macOS 27 now: every cargo needs `--config 'profile.dev.package."*".strip=false'`
  (else sqlx_macros "mis-aligned LINKEDIT").

## OPEN
- Live CDP check of a packaged build from this branch (saved profile -> engine env/argv) NOT done.
- Engine fix RELEASED 2026-10-29 (fork, NOT pinned anywhere yet): streaming whitespace bug (every streamed content
  delta holding < [ ] was .strip()-ed: " [--" -> "[--", "]\n\n" -> "]") + non-stream outer trim + LongCat tool parser.
  Tags: v0.14.3-lz.16 (= lz.15 + fix, branch lz/stream-whitespace b328262b8) and v0.14.3-lz.16-ple.1 (= lz.16 merged
  with lz.14-ple.1, branch lz/stream-whitespace-ple 8c9927ae3). Evidence /Volumes/AI-workhorse/runs/2026-10-29-stream-whitespace-fix/.
  Pin line for engine.rs ENGINE_LAUNCHER (main line / this branch):
    "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.16",
  Pin line for the ple-offload branch:
    "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.16-ple.1",
  When pinning: move the old pin into SUPERSEDED_ENGINE_LAUNCHERS and update the argv goldens in engine.rs.
- Flash-Next MTP (in-shard head detection + K 1-2) still with the engine owner.
- Merge ngram-decoding to main.

## RESULTS 2026-10-29 long context 128k-262k (Flash-Next 4-bit, PLE on, MTP k=2, PLD 0 vs 1, workmule, engine @450bd19e5 = lz.14-ple.1)
Run dir /Volumes/AI-workhorse/runs/2026-10-29-longctx-pld-256k (NOTES.md). One row per finding:
| finding | numbers |
|---|---|
| code_edit PLD speed-up shrinks with length | 1.44x 128k (35.5->51.2 tok/s), 1.26x 192k (29.7->37.6), 1.24x 256k (25.4->31.6); 8k/32k/64k were 1.85/1.74/1.66x |
| PLD acceptance stays high | 1,846-1,894 accepted of ~1,875 drafted per request (98-99 %) at every length |
| PLD on vs off byte-identical on code_edit | sha 5aea2712 at 128k/192k, 7bed0853 at 256k (both configs) |
| model error at 256k, PLD-independent | line 164 `a = a.parse_args()` for `a = ap.parse_args()`, with PLD on and off |
| doc_quote / free_chat no PLD gain | -10 % to +1 %, 0-10 tokens drafted |
| 262,144 fits with PLE | peak 81.1 / 84.6 / 92.07 GB at 128k / 192k / 256k, active 72.25, swap flat (1,255->1,259 MB) |
| prefill at long context | 845 / 672 / 561 tok/s, TTFT 155 / 292 / 461 s |
| memory guard prior calibrates down | 192 -> ~8.4-9.2 B/(chunk x ctx) in the first requests; chunks stay 2,048, no halvings |
| Gauntlet sb-7.2 local Flash-Next (PLE+MTP+PLD, 262k, thinking medium) | agent done ~5.5 h, 157 tool calls, peak 99 GB, swap flat; SCORING REFUSED "fire_d1_mutation:failed" after the candidate ledgerd hit EADDRINUSE; see /Volumes/AI-workhorse/runs/2026-10-29-longctx-pld-256k/HANDOFF.md |

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

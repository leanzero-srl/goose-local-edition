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
- MERGED 2026-10-30 (merge agent): fork tag v0.14.3-lz.17 (lz/release-lz17 5ab6da823) = ONE line again: lz.16
  stream fix + PLE offload + vision + YaRN + reclaim (= lz.16-ple.6 + the offload perf doc). Fork suite (CPU, 10 shards,
  vs lz.16): 157 vs 152 failures, the only new ids are the known 9 ple-line ones (test_qsa_block_sparse x5,
  test_qsa_indexed_splitk x3, test_mtp_cli_wiring x1; fused_gdn_decode x3 fail on both). lz.18 (pld-sampled) sits on it.
  goose main bc42efd43a = flash-pld-bench merged + pin v0.14.3-lz.18 + compaction label fix + FIFO test fix. Merge
  ngram-decoding / compaction work on top of that main. Pre-existing on main (identical on 430d85f003): key_connection
  loopback, process_groups teardown, 4 tests in tests/compaction.rs, test_claude_code_provider, vm_stat probe.
- 2026-10-30 PLD UNDER SAMPLING SHIPPED: fork tag v0.14.3-lz.18 = v0.14.3-lz.17 + 201b0bf62 (branch lz/qwen4-pld-sampled).
  qwen4_exp copy drafts now run at temp > 0 (were greedy-only: 0 proposals in the Gauntlet). Lossless (exact
  speculative sampling, statistical test in tests/test_qwen4_pld_sampled.py). Flash-Next temp 1.0 code_edit 8k on
  workmule: 99.4% accepted, 1.6x decode, 16/16 exact. Disable sampled route only: RAPID_MLX_MTP_PROMPT_LOOKUP_SAMPLED=0.
  Goose pin NOT bumped (ENGINE_LAUNCHER still ple.6). Evidence /Volumes/AI-workhorse/experiments/2026-10-30-qwen4-pld-sampled/.
- YaRN for Qwen3.8-Flash-Next RELEASED 2026-10-29: fork tag v0.14.3-lz.16-ple.5 = v0.14.3-lz.16-ple.4 + one commit
  (6579627b9, branch lz/qwen4-yarn). Opt-in; default rope byte-identical to ple.4 (greedy code_edit 8k f9dca5e1,
  doc_quote 8k db086b90, MTP off, both builds). Enable with the serve flag `--qwen4-rope-scaling yarn` (= Qwen card:
  factor 4, original 262144, ~1,048,576 advertised context), or env RAPID_MLX_QWEN4_ROPE_SCALING=yarn, or the card's
  rope_parameters in config.json; also `yarn:<factor>`, a JSON object, `off`. Maths = transformers 5.15.1 yarn
  (inv_freq rel err 1.3e-7, attention factor 1.13863 exact). Applies to QSA attention + its indexer (+ MTP head).
  PROVEN MAX CONTEXT (workmule, Jundot oQ4e, --qwen4-ple-offload, MTP k=2): production 400k (needle found,
  TTFT 1,048 s, peak 96.1 GB, swap flat). 512k = "fits, at the ceiling": needle found at 50 % and 10 % depth, peak
  114.68 GB = mule Metal ceiling, min free 9 %; NOT for a 5 h Gauntlet with tools/images. 90 % depth at 512k not run.
  CAVEAT: the 300k needle also passed with YaRN OFF (same sha eb646a1e) and doc_quote 300k was 7/7 verbatim on vs
  6/7 off (one sample each): YaRN's benefit over plain extrapolation is UNPROVEN at 300k; off at 400k/512k not run.
  Note the engine does not refuse >262k without YaRN (305k was admitted). Evidence + NOTES:
  /Volumes/AI-workhorse/experiments/2026-10-29-qwen4-yarn/.
  MERGE: lz/qwen4-vision-ple head is still f6cc404c8 (= ple.4), so lz/qwen4-yarn fast-forwards
  (`git merge --ff-only origin/lz/qwen4-yarn`); after later vision commits a plain merge is clean (git merge-tree
  checked; the change touches qwen4_exp rope construction + TextModelArgs parsing, a new qwen4_exp_rope.py, and two
  new cli.py helpers beside the qwen4 offload ones). Pin line:
    "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.16-ple.5",
  plus `--qwen4-rope-scaling yarn` in goose's Flash-Next argv and the context set to 400k (409,600).
  Pre-existing on ple.4 (not YaRN): 10 tests in tests/test_qsa_block_sparse.py, test_qsa_indexed_splitk.py,
  test_qwen4_fused_gdn_decode.py, test_mtp_cli_wiring.py fail (fakes lack the new rope_positions kwarg, etc.).
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

## 2026-10-29 Gauntlet sb-7.2: Qwen3.8-Flash-Next 4-bit, PLE+MTP+n-gram, workmule 256k (job 2 of the long-ctx/PLD run)
- PUBLISHED 0.0065 (0.65 %): https://leanzero.net/agentic-benchmarks/run/brun-fba0d58e-e975-4d02-95d7-4de034b8ec22,
  label "Qwen3.8-Flash-Next 4-bit · PLE+MTP+n-gram · workmule, 256k". Cloud qwen/qwen3.8-flash 0.6846 (brun-dce059c2).
  Caveats (in the doc's `notes`; the run page does not render notes): text-only (cloud sent 7 images); context 262,144
  = model max vs cloud peak 322,851, so local compacted; thinking medium; 32,768 output cap; stopped at the 150-call budget.
- WHY so low: a CANDIDATE fault, not the harness. B3 SIGKILLs ledgerd, reaps it (score_sb8.stop waits on the child,
  descendants killed) and restarts with the same flags (SB7 §10). The model's app/transport.py sets
  `server.allow_reuse_address = True` AFTER `ThreadingTCPServer(...)` has already bound, so SO_REUSEADDR is never
  applied; the restart dies on EADDRINUSE, nothing serves after sync 2, every UI/recovery row is 0 and the
  critical multiplier is 0.028 (inner 0.2645). Reproduced in 4 of 4 scorings; with the candidate's own transport
  the kill-and-restart fails as written and binds when the flag is set before bind (port-repro-*.py).
- The app had REFUSED it (fire_d1_mutation:failed). Harness changes on flash-pld-bench (pushed): d5fa44f640 + e2e456e903
  (gather records each ledgerd spawn's port and exit code; under SB7.2, CHARGE_LEDGERD_EXIT, the last ledgerd on the
  graded port exiting on its own charges the unavailable rows 0 and clears fire_d1_mutation:failed; controls refuse),
  7522c90b2a (that run publishes its raw graded clip), 6c05fbdddf (main.ts compares evidence_dir by realpath: a
  profile under /tmp refused its own re-score). Tests in evals/swarm-bench/bench/test_score_sb72.py LedgerdSelfExitTests.
  main.ts change NOT typechecked (no TypeScript in the worktree's node_modules). No board or local tree shows the
  same signature (50 sb-7.2 board runs scanned; workhorse/laptop/Gabee trees: no Errno 48 in scorer logs).
- Evidence: /Volumes/AI-workhorse/runs/2026-10-29-longctx-pld-256k/gauntlet/ (run tree, published scoring attempt,
  completions, repro scripts). Cleaned: dev app quit, :8291 engine gone, ~/goose-builds/flashpld and /tmp/gflash
  deleted, installed Goose Swarm restored on CDP 9897. KEPT on purpose: workmule ~/models-local/rapid-mlx/
  Qwen3.8-Flash-Next-4bit (98 GB) for the vision follow-up re-run.

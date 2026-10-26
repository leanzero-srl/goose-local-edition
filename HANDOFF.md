# HANDOFF: n-gram / prompt-lookup decoding lever (branch ngram-decoding)

Written 2026-10-26 by the first agent. It hit the 200k ctx-guard right after loading the three skills
(goose-mlx-inference + goose-feature-dev + node-dispatch are ~200k together). NOTHING was built yet.
The worktree is clean at 0925b96990 (= origin/main at start).

## TRAP for the next agent (cost the whole first attempt)
Do NOT load the full skills goose-mlx-inference / goose-feature-dev (they are huge). The facts you need are below.
If you must, `grep -n` the skill files for a keyword instead:
  ~/.claude/skills/goose-mlx-inference/SKILL.md, ~/.claude/skills/goose-feature-dev/SKILL.md,
  ~/.claude/skills/node-dispatch/SKILL.md

## Task (from the coordinator, verbatim intent)
A. Per-model lever: prompt lookup auto/on/off + min n-gram, max n-gram, max draft tokens (tunables).
   ModelProfile field + Settings UI with the app's custom controls (no native select; reuse the existing
   per-model profile UI, e.g. MlxKvCacheFields.tsx / FieldRow / DropdownMenu).
   Wiring: MTP models -> env RAPID_MLX_MTP_PROMPT_LOOKUP (0/1), _MIN_NGRAM, _MAX_NGRAM, _MAX_TOKENS,
   (_SAMPLED is disable-only) on the engine process (no engine bump). Non-MTP pure-attention models ->
   `--suffix-decoding` + `--suffix-max-draft --suffix-max-suffix-len --suffix-min-confidence
   --suffix-min-draft-len` when on. Hybrid non-MTP (Qwen3.5+/GatedDeltaNet) -> lever shown disabled with
   the reason (engine refuses suffix decoding there). Default = auto (engine family policy).
   Engine facts: PLD lives INSIDE MTP (copy-draft), suffix decoding is greedy-only, batch 1, exclusive with MTP.
B. Measure on /Volumes/AI-workhorse/models/goose/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx (31 GB, has
   mtp.safetensors): workloads code-edit (~150-line file, one-line change, full file back), summarise/quote a
   document, free chat. Configs: MTP off; MTP on + PLD off; MTP on + PLD on. temperature 0, N>=3, decode tok/s,
   acceptance (`prompt_lookup_*` in /v1/status or engine log), SHA-256 of outputs PLD on vs off.
   Pure-attention model on the SSD (gemma/llama/mellum JetBrains/Mellum2.1-12B-A2.5B-Thinking is pure attention
   per the skill) -> `--suffix-decoding` off vs on.
C. Unit tests for the argv/env builder; gate; live CDP check only if it does not disturb the owner's app.
D. Facts into local-edition/skills/goose-mlx-inference/references/prompt-lookup-ngram.md + one-line pointer in
   that SKILL.md; dated changelog line in ~/.claude/skills/goose-knob-turning/SKILL.md.

## Code locations (from the brief + first look)
- crates/goose-sidecar/src/engine.rs: `pub struct ModelProfile` at line 414 (fields: speculative, kv_cache,
  thinking...). Add e.g. `prompt_lookup: Option<PromptLookupProfile>` there. `build_serve_command` builds argv
  (MTP via --speculative-config when mtp.safetensors exists); engine spawned via Command at ~3608 / ~4462
  (env can be set there). Argv goldens live in engine.rs tests.
- Engine (READ-ONLY, another agent works there): /Users/workhorse/Projects/Rapid-MLX-wt-offload,
  package rapid_mlx: spec_decode/mtp/generator.py (`_effective_prompt_lookup_policy`), prompt_lookup.py,
  cli.py ~12417 (suffix flags), scheduler.py `_install_suffix_decoding` ~3418, aliases.json suffix_decoding_tier.
- UI: ui/desktop/src/components/mlx/ (MlxKvCacheFields.tsx is the per-model-field pattern); wire DTO in
  goose-sdk-types custom_requests (profile wire names camelCase, e.g. kvCache). Regenerate:
  `cargo run -p goose --bin generate-acp-schema` then `cd ui/desktop && pnpm run build-goose-sdk`.
  i18n keys must exist in all 15 catalogs (round-trip JSON in FILE order, ensure_ascii=False, indent=2).

## Rules
- `source bin/activate-hermit` first, THEN `export CARGO_TARGET_DIR=/Users/workhorse/Projects/goose-wt-ngram/target`.
- Gate: cargo fmt --all; cargo clippy --all-targets -- -D warnings (crates touched); cargo test -p goose-sidecar;
  ui/desktop: pnpm install --frozen-lockfile (fresh worktree), pnpm run lint:check, pnpm run test:run.
- Heavy builds/engine runs via ~/Projects/machine-sync/bin/node-run (host workhorse or gabee, NOT workmule).
- Engine by hand: port 8471 (never 8090 or others' engines), pinned launcher = ENGINE_LAUNCHER in engine.rs
  (`uvx --from "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.14" rapid-mlx serve ...`),
  argv exactly as build_serve_command prints it (write a throwaway test to print it).
- Outputs on /Volumes/AI-workhorse/runs/<dated dir> after `~/Projects/machine-sync/bin/ai-disk-guard --need-gb 1 --out <dir>`.
- Commits: identity leanzero.srl <office@leanzero.net>; NO Co-Authored-By (commit-msg hook rejects it).
  Push: `git push -u origin HEAD:ngram-decoding`.
- Never pkill by pattern; kill only pids you started.

## Suggested split (fresh agents, one step each, each writes back here)
1. Rust: ModelProfile field + env/argv builder + unit tests + ACP wire + schema regen. Commit/push.
2. UI: Settings control + vitest + i18n. Commit/push.
3. Measurement (workhorse via node-run): the 3 configs x 3 workloads x N=3, SHA proof; suffix-decoding on a
   pure-attention model. Write results to the reference file. Commit/push.
4. Skill notes (D) + knob-turning changelog line.

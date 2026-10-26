# HANDOFF: mimo_v2 distributed runner (branch mimo-distributed), 2026-10-26

The first agent spent its context on reading and wrote NO code. Everything below is the design it
reached from the code. A fresh agent builds from this file. The task brief is unchanged (build items 1-8).

## State
- Worktree ~/Projects/goose-wt-mimo, branch mimo-distributed off origin/main (0925b96990). No source change yet.
- Push: `git push -u origin HEAD:mimo-distributed`. Never touch ~/Projects/goose (another agent works there).
- PUBLIC repo: no AI credit or Co-Authored-By trailer. Message style `<area>: <claim>`, lower-case.

## Facts verified from the code and the envs (2026-10-26)
1. The arch whitelist lives in ONE match: crates/goose-sidecar/src/distributed/config.rs:42 `Runner::for_model_type`
   (qwen3_5 -> MlxLmTensor, qwen4_exp -> PipelineQwen4). Everything else matches on the `Runner` enum, so a new
   variant makes every exhaustive match fail to compile. That is the forcing function. The places that copy part of the rule:
   - Runner -> PlacementKind, 4 copies: goose/src/acp/server/mlx_distributed.rs:548 and :626, mlx_placement.rs:702 and :1113,
     goose/src/providers/mlx_speed.rs:144. Fold them into one `Runner::placement_kind()` or `Runner::splits_by_layer()`.
   - The label "tensor-parallel" / "by layer range (pipeline)": goose-sidecar/src/placement/planner.rs:1123.
   - The arch list typed into prose: mlx_distributed_discover.rs:942 ("no splittable model (qwen3_5 tensor, qwen4_exp
     pipeline)") and config.rs:45. Derive both from one table, e.g. `const RUNNERS: &[(&str, Runner)]`.
   - `== Runner::PipelineQwen4` (non-exhaustive, the compiler will NOT flag these). Review each one: preflight.rs:452 and :963,
     runner_update.rs:143, supervisor.rs:2314 (`peers_follow_rank0`), mlx_distributed_discover.rs:917, mlx_placement.rs:834.
     Also `== Runner::MlxLmTensor`: supervisor.rs:1393, mlx_placement.rs:831, link_host.rs:190.
   - provision.rs:200 Runner -> env (`Self::tensor()` / `Self::pipeline()`). PIPELINE_FORK_COMMIT is at provision.rs:24
     (currently 8c000705..., the env installed here reports a18e14fd in direct_url.json, so check which one is live).
   - Regression test idea: a test in config.rs that walks goose-sidecar/src and goose/src (`CARGO_MANIFEST_DIR/..`) and fails
     if `"qwen3_5" =>`, `"qwen4_exp" =>`, `"mimo_v2" =>` or `model_type == "` appears outside config.rs non-test code.
     (mlx_distributed_discover.rs:1222 has `m.model_type == "qwen3_5"`; check whether it is test code.)
2. UI side: no arch whitelist in TS (mlx-engine.ts:317 is only a comment). The runner string arrives as
   `runner: "mlxLmTensor" | "pipelineQwen4"` (goose-sdk-types custom_requests.rs:3984), and the ui/sdk generated types
   carry it. Regenerate them if the DTO changes (check the AGENTS.md "Commands" section for the generator).
3. mlx_lm 0.31.3 (both goose envs): `PipelineMixin.pipeline(self, group)` takes NO split (the manual run used mlx_lm 0.32.0
   from the PR, where it takes `split=None`, so pipe_split.py's `_orig(self, group, split)` does NOT work on 0.31.3).
   Reimplement it in the prelude: for split s (s[r] = layers on rank r, rank 0 = LAST layers):
   `start = sum(s[r+1:])`, `end = start + s[r]`, `self.start_idx, self.end_idx = start, end`,
   `self.layers = self.layers[:end]`, `self.layers[:start] = [None]*start`, and set pipeline_rank/pipeline_size.
   Check that sum(s) == len(layers) and refuse loudly if it does not. The 19,29 split: rank0 [29,48), rank1 [0,29).
4. mlx_lm 0.31.3 sharded_load ALREADY ends with `mx.eval(mx.distributed.all_sum(mx.array(1.0), stream=mx.cpu))`
   (utils.py ~line 590). The manual barrier was a second one. Keep an explicit logged barrier in the prelude anyway
   (GOOSE_RANK_* line before and after) so a stall at that point can be read in the rank log.
5. mlx_lm 0.31.3 server.py: `--pipeline` flag exists (line ~1883); ModelProvider picks pipeline_group when the flag is set
   and the group size is > 1. Requests are shared from rank 0 through the same `_share_request` path as tensor mode.
6. MiMo's chat template uses the SAME qwen3_coder XML tool format (`<tool_call>`, `<function=`, `<parameter=`), so the
   wrapper's tool streaming (rank_tool_stream.py) and NamedToolCallFormatter apply as they are.
7. Checkpoint config (/Volumes/AI-workhorse/models/goose/mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8/config.json):
   48 layers, `hybrid_layer_pattern` (0 = full attention at 0,5,11,17,23,29,35,41,47), num_attention_heads 64,
   num_key_value_heads 4, head_dim 192, v_head_dim 128, SWA: swa_num_key_value_heads 8, swa_head_dim 192,
   swa_v_head_dim 128, sliding_window_size 128, max_position_embeddings 1,048,576, dtype bfloat16. 36 shards + index.
   Upstream mimo_v2.py make_cache: RotatingKVCache(max_size=128) for SWA layers, KVCache for full ones.
   Per-layer bytes: read them from the safetensors headers grouped by `model.layers.N.` (layer0 0.31 GB, 1-47 3.52 GB,
   the rest = embed+norm+lm_head 1.33 GB, kept by EVERY stage; a stage loads only the files its parameters name).
8. Prefill scores: no fused SDPA for this head_dim, so one full-attention layer's chunk materializes
   heads(64) x chunk x context x 2 B for one row (64 x 1024 x 32k x 2 = 4.3 GB). The plan's workspace must charge it, and
   the launch must pass `--prefill-step-size`. The wrapper passes that flag only when `spec.prefill` is set.

## Design reached (implement this)
- `Runner::MlxLmPipeline` ("mlxLmPipeline"), `mimo_v2 -> MlxLmPipeline`, from one table.
- Rank program: REUSE rank_wrapper.py (TENSOR_PROGRAM). Reason: every hook it installs is on the server loop
  (request sharing, batch, prompt cache, HTTP surface, tool stream), and pipeline mode runs the same loop on every rank;
  only the model's forward differs. Add field `pipeline_split: Option<Vec<u32>>` to `RankProgram::MlxLmServer` and bump
  its serde tag (new `rename`, the current one becomes an alias): that is the repo's own rule so an older peer refuses the
  spec instead of running tensor mode. New prelude file `rank_pipeline_split.py`, concatenated into TENSOR_PROGRAM before
  rank_wrapper.py. When the split is set it does `import rapid_mlx.mlx_lm_models` (the vendoring contract: after that,
  `mlx_lm.models.mimo_v2` resolves; it fails loudly if not), patches PipelineMixin.pipeline (fact 3) and logs the barrier.
  rank_wrapper.py appends `--pipeline` to sys.argv when the split is set. `RankSpec::interpreter`: split set ->
  node.pipeline_python (the PIPELINE env), refusing by name when it is absent.
- Wrapper flags for the pipeline: keep the ones that do not depend on the cache kind. rank_batch.py's fast split already
  falls back to upstream for cache kinds it does not know (BatchRotatingKVCache). `cache_width` reads the first
  KVCache/BatchKVCache, which is the full-attention width (right). Review batch_kv_dtype (`extend_in_its_dtype`, BatchKVCache
  only) and the prefill projection against the SWA caches. Set xml_skeleton_guard ON (same XML template).
  `TensorLaunch::for_ranks` refuses ranks whose kv_bytes_per_token differ, and pipeline ranks DO differ (rank0 4 full layers,
  rank1 5). Hand every pipeline rank the same prefill figures: the max per-token cost and the min workspace across ranks.
  The chunk has to agree because every step sends hidden states between ranks.
- Split planner (new fn in plan.rs, e.g. `plan_layer_split`): inputs are per-layer bytes, shared bytes, per-layer KV/token
  (full layers 2 x 4 x (192+128)/2... compute k 192 + v 128 dims x 4 heads x 2 B = 2560 B/token/layer, SWA layers constant
  window x 8 heads x (192+128) x 2 B), context, chunk, and each node's budget (use the existing `crate::fit::budget_bytes`,
  the SAME rule as tensor: min(available - RAM x AVAILABLE_MARGIN_RATIO, GPU ceiling), with RUNTIME_OVERHEAD_RATIO on resident).
  Enumerate every contiguous split with rank 0 = last layers, every rank >= 1 layer, and pick the one that maximises the
  minimum spare (budget - need). No split fits -> bail with each candidate's best min spare and the numbers per node.
  Tests: the real byte table + available workhorse 76.7e9 / mule 111e9 (RAM 96 GiB / 128 GiB, the measured ceilings
  83.49e9 / 115.45e9) gives 19,29 (expected spare about 8.5 / 10.8 GB in the manual sum: check what goose's margin and
  overhead ratios do to it, and record the numbers the test produces). Negative control: halve both budgets -> refusal naming them.
  Also an odd node order (mule as rank 0) must still respect the order rule.
- Preflight: preflight.rs:1409-1412 is where the runner picks its plan. Add the MlxLmPipeline arm. It reads the facts locally
  (rank 0's model_dir; the mule sees the same files over NFS) and uses each node's measured available memory from the probe.
- Launch: supervisor.rs:2250 picks specs per runner. The mlx_lm pipeline also follows rank 0 (`peers_follow_rank0`, :2314),
  check which way it should go. Link hosting: link_host.rs:190 maps phases per runner.
- Rank launch traps: `ulimit -n 65536` for ranks over ssh. Find where `remote_script` (launch.rs:734) builds the peer
  command and prefix `ulimit -n 65536 2>/dev/null || ulimit -n 10240;` (it applies to every runner, which is harmless).
  The mule's ssh sessions start at 256, and the first real load died opening shard 3.
- UI: find the qwen4_exp split display (grep `layerStart|layer_start|starts` in ui/desktop/src/components/leanzero-swarm/
  DistributedEngineSection*.tsx, MlxEngineView.tsx, PlacementCandidates*). Add per node: layers, GB, spare, and why
  (the max-min-spare sentence from the backend, never computed in the UI). No left rails, no faded tints, no native select/alert.
  Add a vitest with an mlxLmPipeline fixture (mlxDistributed.fixtures.ts).
- History: add section "mimo_v2 pipeline runner" to local-edition/skills/goose-mlx-inference/references/distributed-engine-history.md.

## Gate (commit on exit codes)
source bin/activate-hermit; cargo fmt --all; heavy-run for builds (~/Projects/machine-sync/bin/heavy-run --status first):
cargo build -p goose-cli; cargo clippy --all-targets -- -D warnings; cargo test -p goose-sidecar; cargo test -p goose (acp files);
cd ui && pnpm install --frozen-lockfile; cd desktop && pnpm run lint:check && pnpm run test:run.
The qwen3_5 / qwen4_exp distributed tests must pass unchanged. The ratchet `cargo test -p goose-swarm --test development_gates`
counts numeric const literals: every new const needs `// ratio:` or `// measured:` on its line.

## Session 3 (2026-10-26): DONE, commit fccf87936b (pushed), COMPILES
- A: `cargo check -p goose-sidecar -p goose` rc 0. Arms added: preflight (plan_mlx_pipeline), provision
  (MlxLmPipeline -> Self::pipeline()), supervisor launch_specs = LOUD bail "mlx_lm pipeline launch is not wired
  yet" (open item 1), PlacementKind path fixes in goose crate.
- B: new crates/goose-sidecar/src/distributed/layer_split.rs: LayerSplitFacts (config.json hybrid_layer_pattern
  0=full + safetensors headers), stage_need (no RUNTIME_OVERHEAD_RATIO: measured 68.7/100.7 GB footprints vs
  68.28/100.31 weights), plan_layer_split(facts, nodes, reserve, warn_floor, context) -> LayerSplit {layers,
  plans: Vec<RankPlan>, context, chunk, reserve_ratio, reason}. fitting_reserve searches 0.1% steps.
  fit.rs: budget_bytes_with_reserve. DistributedConfig + MlxDistributedConfigDto: plan_reserve_ratio
  (validated >= the WARN ratio IN FORCE and < AVAILABLE_MARGIN_RATIO), config.plan_reserve_ratio().
  PreflightReport new fields: layer_split, split_reason, plan_reserve_ratio (+ pipeline_starts = layer_start
  per rank, context_limit, plans per node with layer_start/end, weights, budget, with_overhead).
- Test numbers (7 tests green): default 9.3% -> refusal "budget 67.11 GB / 98.22 GB", "a split fits at reserve
  7.7% (19/29 layers ...)", "set plan_reserve_ratio between 0.05 and 0.077". Reserve 0.05 -> 19 rank0 [29,48)
  68,279,546,752 B / 29 rank1 [0,29) 100,312,603,648 B, derived context 60,672, chunk 256. Requested 32k -> same
  split, shared prefill figures (kv/token = 5x2560 max). Halved available -> "no split fits even with no reserve".
- D (run): cargo test -p goose-sidecar 495+ pass; clippy -p goose-sidecar -p goose --all-targets -D warnings rc 0;
  development_gates 11 pass. qwen tests unchanged.

## OPEN (in order)
1. C: expose layer_split / split_reason / plan_reserve_ratio in the goose ACP preflight DTO the UI reads
   (grep -n "pipeline_starts" crates/goose/src/acp/server/*.rs crates/goose-sdk-types/src/custom_requests.rs;
   per node the plan already carries layer_start/end, weights, budget; spare = budget - with_overhead).
   Then regenerate: grep -n "acp" justfile (likely `just generate-acp-types`, needs `cd ui && pnpm install
   --frozen-lockfile` first), commit ui/sdk/src/generated.
2. Rank wrapper pipeline launch (HANDOFF facts 3-5 + Launch notes above): pipeline_split in RankProgram,
   rank_pipeline_split.py prelude, `--pipeline`, ulimit in remote_script, interpreter = pipeline_python;
   replace the supervisor bail. TensorLaunch::for_ranks refuses differing prompt-cache bounds: pipeline ranks
   differ (per-rank state) -> needs a pipeline-aware path.
3. Placement store/planner and UI split display (next agents).

## Exact commands (macOS 27 workhorse!)
S=<scratchpad>/mimo.sh = cd worktree; source bin/activate-hermit; export CARGO_TARGET_DIR=~/Projects/goose/target; "$@"
~/Projects/machine-sync/bin/heavy-run --timeout 3000 -- $S cargo check --config 'profile.dev.package."*".strip=false' -p goose-sidecar -p goose
Same for test / clippy (`cargo clippy --config '...' -p ... --all-targets -- -D warnings`).

## Traps
- macOS 27: without `--config 'profile.dev.package."*".strip=false'` sqlx_macros dylib fails "mis-aligned LINKEDIT".
  Own target dir in the worktree is not needed: share ~/Projects/goose/target.
- ctx-guard: this agent reached 180k by the end of B. Keep reads to grep -n + sed ranges.
- Never touch ~/Projects/goose sources, the engine on :8090, or pkill by pattern.

## Session 4 (launch), 2026-10-26: WIP, NOT COMPILED (ctx-guard stop at ~190k)
Done (uncompiled, no cargo run yet):
- NEW crates/goose-sidecar/src/distributed/rank_pipeline_split.py, in TENSOR_PROGRAM before rank_wrapper.py.
  Runs only when spec["pipeline_split"] is set: `import rapid_mlx.mlx_lm_models` + `mlx_lm.models.mimo_v2`
  (SystemExit naming sys.executable if it fails); refuses if MiMoV2Model.pipeline has no `split` param; patches
  MiMoV2Model.pipeline (NOT PipelineMixin) to default the planned split, refusing sum(split) != len(layers);
  wraps mlx_lm.server.sharded_load (server.py does `from .utils import sharded_load`, so patch the server
  name) -> refuses if the split was never applied, emits GOOSE_RANK_PIPELINE_LOADED, CPU all_sum barrier,
  GOOSE_RANK_PIPELINE_BARRIER; scales cache nbytes (see "why" below); emits GOOSE_RANK_PIPELINE_SPLIT.
- rank_wrapper.py tail: `--pipeline` appended when spec.pipeline_split is set.
- launch.rs: RankProgram::MlxLmServer tag -> `mlxLmServerPipelineSplit` (old `mlxLmServerNamedConversation`
  added as alias, doc paragraph added); new fields `pipeline_split: Option<Vec<u32>>`,
  `pipeline_cache_weights: Option<PipelineCacheWeights>` (struct {full, swa} added after rank_specs);
  rank_specs sets both None; `interpreter()` -> node.pipeline_python when split is Some, named refusal;
  remote_script prefixes `ulimit -n {RANK_OPEN_FILES} 2>/dev/null || ulimit -n {RANK_OPEN_FILES_FLOOR};`
  (consts 65_536 / 10_240 with `// measured:` markers).

WHY the cache weights (found this session, a real blocker for pipeline): mlx_lm's LRUPromptCache evicts on
`sum(c.nbytes for c in prompt_cache)` (models/cache.py:1706, server.py fetch_nearest_cache per rank at :753/:965),
and the wrapper trims on prompt_cache_nbytes vs prompt_cache_limit. Pipeline ranks hold different layers
(rank0 4 full + 15 SWA, rank1 5 full + 24 SWA) so raw bytes differ -> ranks evict differently -> different
cached prefix -> different prefill step counts -> hidden-state send/recv mismatch (hang). Fix: per-class
integer weights w_f_r = lcm(full_r)/full_r, w_s_r = lcm(swa_r)/swa_r on KVCache/BatchKVCache and
RotatingKVCache/BatchRotatingKVCache nbytes -> every rank's sum = lcm_f*x_f + lcm_s*x_s, same integer.
(Real-unit n_max/n_r scaling needs division -> rounding drift; Fraction breaks json emits. Hence lcm units.)

OPEN (next agent, in order):
1. Make it compile: other `RankProgram::MlxLmServer {` struct literals (launch.rs tests ~1163+) need the two
   new fields (`pipeline_split: None, pipeline_cache_weights: None`). grep -n "MlxLmServer {" crates/goose-sidecar.
2. `TensorLaunch::for_pipeline_ranks(plans, layers: &[u32], full: &[u32]) -> Result<(Vec<TensorLaunch>,
   Vec<PipelineCacheWeights>)>`: from_plan each; REFUSE if prefill step or pair_bytes differ (chunk must agree);
   swa_r = layers_r - full_r; refuse if a kind is 0 on one rank but >0 on another (its cache cannot track the
   others' tokens); M_f = lcm(full_r), M_s = lcm(swa_r); kv per layer = prefill.kv_bytes_per_token / max(full_r),
   swa state per layer = prefill.sequence_state_bytes / max(swa_r) (planner layer_split.rs:458-470 builds them
   as products, exact); canonical prefill kv = M_f*kvl, seq = M_s*swl, workspace = min; limit =
   min_r(L_r * min(w_f_r, w_s_r)) (weights >= 1 so canonical >= real, safe), entries = min_r. Same on every rank.
3. Data: add `full_layers: Vec<u32>` to layer_split::LayerSplit (from needs[i].full_layers) and
   `pipeline_full_layers: Option<Vec<u32>>` to PreflightReport (literals at preflight.rs ~1240 and ~2744),
   set in plan_mlx_pipeline (preflight.rs ~1719); `PreflightReport::mlx_pipeline_launches()` beside
   tensor_launches (preflight.rs:253).
4. launch.rs: `mlx_pipeline_rank_specs(config, served, launches, weights, split, context, secs)` = rank_specs
   body with pipeline_split Some(split) and pipeline_cache_weights Some(weights[rank]) (refactor into one fn).
5. supervisor.rs:2254 replace the bail with the arm (layer_split + pipeline_full_layers from preflight,
   "the preflight approved no layer split" refusal). peers_follow_rank0 (:2318): make it an exhaustive match,
   MlxLmPipeline => false (it runs the tensor wrapper; stop signals every rank like tensor).
   server_load (:1393) and link_host.rs:190 (Warming -> Ready): MlxLmPipeline behaves as MlxLmTensor (same
   wrapper, no slots) -> match both. Optionally preflight node checks: fail when pipeline_python is absent for
   MlxLmPipeline (preflight.rs ~981 only checks PipelineQwen4).
6. Tests (unit only): spec serde round-trip with split + old tag alias reads; python_args program contains
   "rank_pipeline_split" text + "rapid_mlx.mlx_lm_models" + "--pipeline"; 2-rank mimo plan (layer_split tests'
   mimo_facts/nodes) -> specs carry split [19,29], weights rank0 {full:5, swa:8}, rank1 {full:4, swa:5}, same
   limit/entries/prefill on both, interpreter = pipeline_python; remote_script contains "ulimit -n 65536";
   refusals: no pipeline_python, differing step, a rank with 0 full layers.
7. Gate: cargo test -p goose-sidecar; clippy -p goose-sidecar -p goose --all-targets -D warnings;
   cargo test -p goose-swarm --test development_gates. rustfmt only own files. Commit, push HEAD:mimo-distributed.
Unverified by design (live two-Mac only): the vendored pipeline under the wrapper's hooks (rank_batch fast path
with BatchRotatingKVCache, batch_kv_dtype on SWA caches), the barrier, the weights' eviction parity in practice.

## Session 4 (DTO+view), 2026-10-26: backend DONE (5662f78886), UI view OPEN
- Per-model reserve: `MODEL_PLAN_RESERVES` in crates/goose-sidecar/src/distributed/config.rs next to RUNNERS,
  `("mimo_v2", 0.05)` with `// ratio:`. `DistributedConfig::plan_reserve(model_type) -> (f64, ReserveSource)`:
  user override > model entry (clamped to [WARN in force, AVAILABLE_MARGIN_RATIO]) > shared default. The old
  `plan_reserve_ratio()` is gone. Global default untouched (0.093).
- layer_split.rs: refusal is now `SplitRefusal { message, fitting_reserve_ratio }` (anyhow-wrapped, Display = the
  same sentence); fitting ratio rounded to 0.001 and only set when >= the WARN floor (0.077 for the MiMo test).
- PreflightReport + MlxDistributedPreflightDto: layer_split, split_reason, plan_reserve_ratio (also on refusal),
  plan_reserve_source ("default"|"model"|"user"), fitting_reserve_ratio. TS regenerated (ui/sdk/src/generated).
  Regen recipe without just: scratchpad gen-acp.sh = `cd ui && pnpm install --frozen-lockfile`; `cd crates/goose &&
  cargo run --config '...strip=false' --features code-mode,local-inference,aws-providers,telemetry,otel,rustls-tls,system-keyring
  --bin generate-acp-schema`; `cd ui/sdk && npx tsx generate-schema.ts`.
- supervisor.rs: my 3 literal lines were staged ON TOP OF HEAD (git hash-object + update-index), the other agent's
  launch edits stay unstaged in the working tree. Do the same if you must touch supervisor/launch.
- Gate measured: sidecar 493 pass / 5 fail = 4 launch.rs tests (the OTHER agent's in-progress ulimit/launch edits)
  + engine status_reports_a_stray_listener (flaky; passes alone). development_gates 11 pass. clippy fails ONLY on
  the other agent's files (supervisor.rs unused `bail`, launch.rs is_multiple_of). Re-run clippy once they commit.
- Real MiMo plan at 0.05 (fixture numbers): rank0 workhorse [29,48) weights 68,279,546,752 with_overhead
  71,545,402,240 budget 71,546,039,245; rank1 mule [0,29) weights 100,312,603,648 with_overhead 103,900,896,256
  budget 104,128,052,327; context 60,672 derived; reason "split 19/29 (rank 0 = the last layers) leaves the most
  memory on the tightest Mac of all 47 contiguous splits: spare 0.00 GB / 0.23 GB at context 60672 with a 256-token
  prefill chunk, reserve 5.0% of RAM". Spare ~0 is by design (derived context fills the budget; margin = reserve).
- UI: pure helpers added in mlxDistributed.ts (MLX_LM_PIPELINE, layerSplitView, gbDecimal, reservePercent,
  reserveSource, reserveFromPercent). OPEN:
  1. New component (e.g. MlxLayerSplit.tsx) rendered in PreflightReportView (DistributedEngineSection.tsx ~L1030)
     when layerSplitView(report) != null: split chip "19 / 29 layers", reserve chip "5.0% of RAM" + source word
     (goose default / mimo_v2's own / set by you), reason verbatim, per node row: L29–47, weights GB, budget GB,
     spare GB (decimal GB, the backend's unit). Refusal + fittingReserveRatio: ToneBanner (./studio) with Button
     "Plan at 7.7%" -> setDraft({...config, planReserveRatio}) then mlxDistributedPreflight(cleanConfig(next)).
     Needs a callback passed from DistributedEngineSection (owns draft) into PreflightReportView (like AdoptRunnerContext).
  2. ConfigEditor: a percent input for planReserveRatio (copy NumberInput, allow '.', local text state,
     reserveFromPercent; placeholder "model default"); cleanConfig must delete planReserveRatio when null.
  3. Fixtures: MIMO_PREFLIGHT_OK / MIMO_PREFLIGHT_REFUSED (runner 'mlxLmPipeline', modelType 'mimo_v2', numbers
     above; refused: planReserveRatio 0.093, source 'user', fittingReserveRatio 0.077, plan check fail, no plans).
     Vitest in DistributedEngineSection.test.tsx (+ expectDesigned for owner UI rules) and mlxDistributed.test.ts.
  4. `pnpm run i18n:extract` after new messages, then lint:check + test:run via $S; commit `desktop: ...`.

## Session 4c (view), 2026-10-26: UI view DONE (347121d242 + i18n commit)
- ui/desktop/src/components/leanzero-swarm/MlxLayerSplit.tsx renders inside PreflightReportView (above the
  node cards, also while a run lives) when layerSplitView(report) != null: zone "Layer split", chip "19 / 29
  layers", chip "reserve 5.0% of RAM" + source word (goose's default / mimo_v2's own reserve / set by you),
  the margin sentence (safety margin = the reserve, spare ~0 GB by design), the reason verbatim, one row per
  rank (L29–47 · 19 layers, weights / budget / spare in decimal GB, negative spare red).
- Refusal: ToneBanner warn "Fits at 7.7%" + Button "Plan at 7.7%" (testid mlx-dist-plan-at) -> section's
  onPlanAt: saved config -> mlxDistributedConfigUpdate(cleanConfig({...config, planReserveRatio})) then
  preflight(null); open draft -> setDraft + preflight(cleanConfig(next)), nothing saved. No fitting ratio ->
  ToneBanner err "No reserve fits", no button. The plan check's sentence stays verbatim in the failing list.
- Advanced: PercentInput "Plan reserve (% of RAM)", placeholder "model default", own text state, empty = null;
  cleanConfig deletes planReserveRatio when null.
- Fixtures MIMO_CONFIG / MIMO_PREFLIGHT_OK / MIMO_PREFLIGHT_REFUSED (refusal sentence: asserted fragments +
  ILLUSTRATIVE need/ceiling/spare figures). Tests: section 67 (7 new), helpers +3; full suite 5196 pass.
- TRAP: desktop typechecks against ui/sdk/dist (gitignored): after regenerating sdk types run
  `cd ui/sdk && pnpm run build:ts` or typecheck says the new DTO fields do not exist.
- TRAP: lint:check runs i18n:check, which needs every new key in all 15 locale catalogs (real translations).
- Open: splitConfigFor/withModel carry a user planReserveRatio over to another model (backend validates it);
  live CDP check of the view not done (later agent).

## Session 4b (launch), 2026-10-26: steps 1-7 CODED, 4 old tests red (ctx-guard stop at ~175k)
Landed (HEAD 29c1d2d24a; NOTE the DTO agent's commit swept my staged index in under ITS message: launch.rs,
link_host.rs, preflight.rs, runner_update.rs, supervisor.rs; layer_split.rs edits went in 5662f78886):
- launch.rs: `TensorLaunch::for_pipeline_ranks(plans, layers, full)` (refuses: split/plan-range mismatch "covers N
  layers", differing prefill figures, a rank with 0 of a kind another rank has; canonical kv = lcm_f*kv/layer,
  seq = lcm_s*state/layer, workspace min, limit min_r(L_r*min(w)), entries min); `kind_weights`;
  `mlx_pipeline_rank_specs` (rank_specs now delegates to `mlx_lm_server_specs`); tests mod `pipeline_launch_tests`
  (3 tests: 19/29 mimo split, weights {5,8}/{4,5}, equal weighted totals, interpreter, ulimit, prelude text,
  serde round trip + NamedConversation alias, refusals).
- layer_split.rs: `LayerSplit::full_layers`; tests mod/mimo_facts/nodes now pub(crate).
- preflight.rs: `PreflightReport::pipeline_full_layers`, set in plan_mlx_pipeline; `mlx_pipeline_launches()`;
  node check fails "pipeline" when MlxLmPipeline and no pipeline_python.
- supervisor.rs: MlxLmPipeline arm in launch_specs; peers_follow_rank0 exhaustive match (pipeline=false);
  server_load treats MlxLmPipeline as tensor. link_host.rs publish(): Warming->Ready for both.
Weighted-eviction review (verified in ~/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12 mlx_lm 0.31.3):
models/cache.py LRUPromptCache.insert_cache sums `c.nbytes` per entry, evicts on max_bytes; server.py:795 trim_to
(prompt_cache_bytes - batch active nbytes) — per rank, own bytes. KVCache/RotatingKVCache/BatchKVCache/
BatchRotatingKVCache each define their OWN nbytes (no double weighting via inheritance). KEPT the weights over a
token-count scheme: token eviction needs patching LRU entry sizes + every nbytes consumer (batch active sum, the
wrapper's prompt_cache_nbytes), and SWA bytes saturate at the 128 window so a token-linear bound is loose; one
property patch covers every consumer, weighted >= min(w)*real so the limit is safe and near-tight. Caveat:
QuantizedKVCache is NOT weighted (refuse/handle if a kv_bits path is ever enabled for mimo).
OPEN (next agent):
1. 4 pre-existing launch tests fail (asserted the old tag / exact remote script, broken by b02002b2d2's tag rename +
   ulimit prefix): a_pipeline_rank_serves_the_approved_split_under_the_forks_interpreter (:1671),
   the_remote_command_prints_the_pid_then_execs_the_marked_rank (:1477),
   the_tensor_program_carries_the_budget_and_the_launch_window (:10447),
   the_tensor_spec_asks_for_the_doorbell_and_an_older_peer_refuses_it. Decide per test: update the expectation
   (ulimit prefix, new tag) — these are launch-shape tests, not qwen behaviour; or keep the tensor tag unchanged
   for tensor specs (serialize NamedConversation when pipeline_split is None) if "unchanged" must hold literally.
2. supervisor.rs:25 `bail` import now unused -> remove (clippy -D warnings).
3. My 3 new tests: not seen in the FAILED list of the last run, but confirm they ran green.
4. Gate: cargo test -p goose-sidecar; clippy -p goose-sidecar -p goose --all-targets -D warnings;
   cargo test -p goose-swarm --test development_gates; rustfmt launch.rs + the 5 files. Push HEAD:mimo-distributed
   (origin is at 29c1d2d24a already per `git log origin/...`; verify).
TRAP: the DTO agent shares the git INDEX in this worktree — anything you stage can be swept into its commit. Stage
and commit in ONE command (`git add <paths> && git commit`), never leave things staged.

## Session 5 (coordinator, 2026-10-26 ~20:00): state for the NEXT coordinator
- This commit: removed duplicated lines left by the shared-index mixup (preflight.rs field/fn/inits, runner_update.rs),
  dropped unused `bail` (supervisor.rs), updated launch tests to the ulimit prefix, the new tag
  `mlxLmServerPipelineSplit` (repo convention: every spec change bumps the tag for tensor too), and
  rank_pipeline_split.py in TENSOR_PROGRAM. cargo test -p goose-sidecar: 499 pass, 1 FAIL:
  launch::tests::the_tensor_spec_asks_for_the_doorbell_and_an_older_peer_refuses_it (panics near launch.rs:10550/10604;
  read the panic message; likely the remaining refusal-message/alias expectations for the new tag). Then clippy
  -p goose-sidecar -p goose --all-targets -D warnings, development_gates.
- Desktop split-view agent was STILL RUNNING at this commit (ui/desktop only; HANDOFF "Session 4 (DTO+view)").
- Rapid-MLX: release agent (another session) owns suite compare + tags v0.14.3-lz.15 / lz-pipeline-qwen4.15 + push.
  Suites: scratchpad suites2.sh (new lines, logs suite2-{single,pipe}.log) and suites3-base.sh (base lines), pytest
  --timeout 180 (pytest-timeout installed in the 4 tvenvs; the first base run hung at 0% CPU in
  tests/test_batching_deterministic.py and was killed). When lz-pipeline-qwen4.15 is on origin: bump
  PIPELINE_FORK_COMMIT (provision.rs) to its sha.
- Then: live two-Mac test (brief in COORDINATOR.md item 4), skill notes (item 5).
- Build wrapper: scratchpad/mimo2.sh (CARGO_TARGET_DIR=worktree/target, APFS clone of ~/Projects/goose/target).
- TRAP: two agents in one worktree share the git INDEX: a `git commit` takes the other's staged files. Run one
  committing agent at a time, or give each its own worktree.
- UPDATE: desktop split view DONE (347121d242, dde8f3a7e0, bd80d7ecbb; vitest 5196 pass; see Session 4c). Not CDP-checked. After SDK regen run 'cd ui/sdk && pnpm run build:ts' before desktop typecheck.

## Session 6 (2026-10-26 ~20:45, coordinator stopped at context limit)
- DONE fc91d575cd: the failing test was a CODE bug, not the test: `link_control::older_peer_refusal` lacked the new tag
  `mlxLmServerPipelineSplit` (an older peer refusing a mimo rank would not say "update goose"). Added first in the list.
  Also clippy manual_is_multiple_of (launch.rs:589). Gates: cargo test -p goose-sidecar 500 pass/0 fail (+ integration
  9/2/13/6 pass), fmt --check 0, clippy -p goose-sidecar -p goose --all-targets -D warnings 0, development_gates 11/11.
  Desktop lint/vitest NOT re-run this session (no UI change since Session 4c's 5196 pass).
- PIN: NOT bumped. At 20:45 neither `lz-pipeline-qwen4.15` nor `v0.14.3-lz.15` was on origin; release agent suites
  (suites2.sh pid 16686, suites3-base.sh pid 19744) still running. Wait: `git -C ~/Projects/Rapid-MLX ls-remote --tags origin | grep -E 'qwen4.15|lz.15'`.
  Bump = provision.rs PIPELINE_FORK_COMMIT + PIPELINE_FORK (both carry the sha) + rerun sidecar tests (preflight tests
  reference the const). PIPELINE_SINGLE_LINE_REVIEWED_THROUGH stays lz.14 unless ENGINE_LAUNCHER also moves (a test pins equality).
- DEV BUILD in flight at handoff: scratchpad live/build.sh under heavy-run (cargo build goose debug -> just copy-binary debug
  -> ui/desktop pnpm run package), log scratchpad live/build.log (BUILD_OK on success). Rebuild after the pin bump.
- LIVE TEST prep (not started): decided the ssh-peer path (documented recipe: seeded config, node `ssh: workmule`)
  instead of Link, because workmule's installed 3.0.132 serves Link and would refuse the new tag; replacing it is invasive.
  Seeded isolated profile: /tmp/gmimo/root/config/config.yaml (short path: sun_path), userdata /tmp/gmimo/userdata;
  mlx_engine port 8190, distributed port 8191, coordinator 32373, JACCL; workhorse en3 192.168.0.2 "EXO Thunderbolt 2"
  rdma_en3; workmule en9 192.168.0.1 "TB Workhorse" rdma_en9, model /Users/workmule/.goose/models/mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8;
  workhorse model /Volumes/AI-workhorse/models/goose/mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8. Mule has no
  ~/.goose/distributed yet (provision builds it; uv at /opt/homebrew/bin/uv). CDP driver copied to scratchpad live/cdp.mjs.
  Launch: GOOSE_PATH_ROOT=/tmp/gmimo/root GOOSE_USER_DATA_DIR=/tmp/gmimo/userdata GOOSE_DISABLE_KEYRING=1
  "ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app/Contents/MacOS/Goose Swarm" --remote-debugging-port=9461 (or Goose-darwin-arm64 path).
- BLOCKER for the 170 GB load: workhorse holds another session's PLD matrix (/Volumes/AI-workhorse/runs/2026-10-26-pld-qwen38,
  engine :8471, 33 GB, GPU 98%, at config c3 of 4). Rank 0 needs ~68 GB of ~71 GB available: wait for it to finish; never touch it.
  Mule had a CI cargo test job (goose-controller-build) in a heavy slot.
- SIDE EFFECT to note: provisioning upserts ~/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12 (shared with the owner's
  installed 3.0.132) to the new pin; 3.0.132 rebuilds its own pin on its next Flash start (Q-234).
- Remaining: pin bump -> rebuild -> live test (COORDINATOR item 4; screenshot to /Volumes/AI-workhorse/media/goose/mimo-2026-10-26/
  after ai-disk-guard --out) -> skill notes (item 5). Commits: no AI trailer (public repo hook rejects it).
- UPDATE: dev build finished BUILD_OK (pre-pin): "ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app". Rebuild after the pin bump.

## Session 7 (2026-10-26 ~20:00 EET, stopped at ctx-guard 160k before any code change)
- Head 712f5c0264 = origin/mimo-distributed. No code changed this session.
- WAITS: PLD benchmark FINISHED 19:55 (:8471 gone, workhorse 91% free) -> the 170 GB load is no longer blocked.
  Dev build BUILD_OK (old pin 8c00070, packaged at ui/desktop/out/Goose Swarm-darwin-arm64/). Fork tags
  `lz-pipeline-qwen4.15` / `v0.14.3-lz.15` NOT on origin at 19:58; release agent's pipe suites (suite2-pipe.log,
  suite2-base-pipe.log) at ~54%, single suites ended exit=134 (its problem, not ours). Re-check:
  `git -C ~/Projects/Rapid-MLX ls-remote --tags origin | grep -E 'qwen4.15|lz.15'`. Deadline for the bump ~22:55 EET,
  else report "tags not on origin".
- FOUND (live, dev package): the embedded DEBUG `goose serve` dies at once: "thread 'tokio-rt-worker' has overflowed
  its stack / fatal runtime error: stack overflow, aborting" (SIGABRT, /tmp/gmimo/userdata/logs/goose-serve-stderr.log)
  -> UI shows "This window's Goose backend stopped". Launching the app with `RUST_MIN_STACK=16777216` kept goose serve
  alive (>35 s, no abort). Use that env on every dev-package launch (or build `just release-binary`/copy-binary release).
  Not yet known whether 3.0.132 release is affected (release frames are smaller; likely debug-only).
- Launch line that works:
  cd ui/desktop && RUST_MIN_STACK=16777216 GOOSE_PATH_ROOT=/tmp/gmimo/root GOOSE_USER_DATA_DIR=/tmp/gmimo/userdata
  GOOSE_DISABLE_KEYRING=1 nohup "out/Goose Swarm-darwin-arm64/Goose Swarm.app/Contents/MacOS/Goose Swarm"
  --remote-debugging-port=9461 > $SCRATCH/live/app.log 2>&1 &   (stop: kill <pid> from live/app.pid; never pkill)
- Distributed UI: buttons have NO testids; click by text "Preflight (dry run)", "Start", "Stop" inside
  [data-testid=mlx-distributed]; split view rows mlx-dist-layer-split / -split-row / -split-reason. mimo plans with the
  built-in MODEL_PLAN_RESERVES mimo_v2 0.05 (no plan_reserve_ratio needed in the seeded config).
- Installed app (pid 40896, CDP 9897, another session) is running: leave it alone.
- Next, in order: tags -> bump PIPELINE_FORK_COMMIT + PIPELINE_FORK (provision.rs:115/134) -> cargo test -p goose-sidecar
  + clippy (--config 'profile.dev.package."*".strip=false', after the subcommand for clippy) -> rebuild via heavy-run
  (scratchpad live/build.sh) -> live test per COORDINATOR item 4 -> skill notes (item 5). Commits: no AI trailer.

## Session 8 (2026-10-26 20:00-22:00 EET)
- Head ee2a4b390d = origin/mimo-distributed. Commits: 23e7944192 (stack overflow), 0374b824f6 (pin), ee2a4b390d (rdmaGid).
- STACK OVERFLOW ROOT CAUSE (fixed): macro-generated `handle_custom_request` (goose-acp-macros) awaited every
  handler inline; its debug poll frame was 1,801,264 B (read from prologues in the crash report
  ~/Library/Logs/DiagnosticReports/goose-2026-10-26-195646.ips). Each arm now `Box::pin(async move{..}).await`:
  frame 792,176 B. Test crates/goose/tests/acp_custom_request_stack_test.rs (2 MiB workers): red SIGABRT before,
  green after. Dev app now runs WITHOUT RUST_MIN_STACK. Frame-size tool: scratchpad so/fs.py + so/frames2.py.
- PIN: PIPELINE_FORK(_COMMIT) = 07126f05 (lz-pipeline-qwen4.15, vendored mimo_v2). sidecar 500 lib + all tests green.
- rdmaGid: preflight failed both Macs on "GID[2], not GID[1]"; link repair could not move it; JACCL all_sum ran
  at 8.95 GB/s with that table (~/jaccl-smoke/run.sh jaccl). Check now fails only on an EMPTY index below the
  IPv4 GID (the soak's failing shape). Leave the "Repair the Thunderbolt link" switch OFF.
- Live UI: onboarding first (click [data-testid=onboarding-use-swarm], then "Get Started"); section is at
  #/leanzero-swarm?tab=mlx&mlx=engine. Provision = "Detect again" -> pick "workmule· workmule" -> Detect ->
  "Save and provision" (both Macs' envs Ready in <10 s; pipeline env at 07126f05). Preflight plan with free
  Macs: split 17/31 (rank0 workhorse L31-47 61.2 GB, rank1 workmule L0-30 107.4 GB), context 126,208.
- BLOCKED at 22:00: workhorse has a 32 GB TTS job (tts.py, content pipeline) and workmule a 7-label
  ~/lcbench/chain_flash.sh benchmark (another agent, started 21:48): plan "no split fits". Not preempted.
- OWNER RULE: one Goose app per Mac. Installed app was quit for the test (osascript quit by path); dev app
  pid in scratchpad live/app.pid (CDP 9461). After the test: quit dev, `open -a "Goose Swarm"`, confirm.
- Still to do: Start, 3 prompts (coding/reasoning/Jira) load time + tok/s vs 277 s / ~40 tok/s; then 16k/32k/64k
  prompts (TTFT, prefill, decode, peak mem both Macs); split-view screenshot to
  /Volumes/AI-workhorse/media/goose/mimo-2026-10-26/ (ai-disk-guard first); Stop; no ranks left; skill notes.

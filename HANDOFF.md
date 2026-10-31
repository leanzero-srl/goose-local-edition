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

## Session 9 (2026-10-27 02:10-08:20 EET): MiMo split LIVE through goose; 5 fixes; long-context ext OPEN
- Head 95784b84b5. Fixes (all measured live): 987d408419 rank program zlib (rank-1 ssh cmd was 354,529 B > OpenSSH
  256 KiB exec packet: "Connection reset by peer", exit 255); 0a79cc72a5 timed local GPU warm-up + 2nd CPU barrier
  before the seed (rank0 GPU took 6.9-16.4 s to answer after load, rank1's GPU-stream seed all_sum hit "[METAL] GPU
  Timeout Error"; 88ecb28d6d CPU-stream seed was REFUTED); 95784b84b5 MiMo tool parser -> qwen3_coder after tokenizer
  load (tokenizer_config.json says json_tools; f75971e5de _infer patch REFUTED). Launch tests 75/75.
- Launch: installed app quit by osascript; dev app = cd ui/desktop && GOOSE_PATH_ROOT=/tmp/gmimo/root
  GOOSE_USER_DATA_DIR=/tmp/gmimo/userdata GOOSE_DISABLE_KEYRING=1 nohup "out/Goose Swarm-darwin-arm64/Goose Swarm.app/
  Contents/MacOS/Goose Swarm" --remote-debugging-port=9461 &; nav #/leanzero-swarm?tab=mlx&mlx=engine; click
  [data-testid=placement-run-split]; Stop = [data-testid=placement-stop-split] then dialog "Stop the split".
  CDP driver scratchpad mimo9/cdp2.mjs (picks the main window; Run opens a 2nd "engine-glance" window).
- TRAP NFS: mule weights are an rclone NFS mount of the workhorse SSD (soft,intr, 192.168.0.2:12049). A cold rank-1
  load fails ~1/2 with "nfs server 192.168.0.2:/: not responding" + "[load_safetensors] Failed to open file
  ...00003...". Workaround before every Run: scratchpad mimo9/nfswarm.sh (mx.load all 36 shards on the mule, ~11 s).
  Root cause NOT found (rclone/mount infra, not goose).
- TRAP memory: plan sizes context to spare ~0 GB; workhorse swap grew 0.6 -> 3.2 GB during load; at 03:08 goose's
  watchdog closed admission and at 04:10 stopped the split ("available 4.5 GiB of 96.0") when other apps grew.
- Numbers (split 18/30 or 19/29, JACCL, context 257,536): click->first answer 266 s (load 154-285 s; manual 277 s).
  Direct to :8191: short prompts prefill ~135 tok/s, decode 34.8-35.3 tok/s (manual ~40). 16k: TTFT 53.4 s, prefill
  292, decode 31.2. 32k: 109.7 s, 288, 31.0. 64k: 562.4 s, 112 (slows from ~270 to ~80 tok/s after ~19k), 27.5.
  Peaks (cumulative, incl. prompt cache): rank0 71.3/72.6/75.0 GB of 83.5 limit, rank1 104.0/105.4/108.1 of 115.4;
  swap flat during the long runs (wh 1.76-1.94 GB, wm 0.76). Through goose (omlx, ~19.8k-token system prompt):
  first call 72 s, cached 14-18 s; coding now writes both files via tools and pytest 4 passed (was 4 empty responses).
- Quality: coding correct (independently re-run); reasoning correct (10:56:40, 155.6 km; one garbled self-check line);
  Jira JQL correct, but "single quotes not allowed in membersOf" is false. 16k: 3 signatures real; 32k: 10 points,
  quotes ~verbatim except "`judge_skipped` reached 84" (not in doc) and the "oldest open item" (doc slice has none);
  64k: all 7 consts listed, integrate-verify line found but quoted with a wrong line prefix.
- OPEN (owner, 2026-10-27): 128k, ~250k, 300k, 350k. Planner (layer_split.rs stage_need) at context 360k refuses:
  rank0 87.57/83.49 GB, rank1 121.46/115.45 — dominated by the prefill score workspace chunk x width x (heads*act+1)
  (256-token chunk at 360k ~23.7 GB; no fused SDPA for head_dim 192), KV is only 2560 B/token/full layer. 128k and
  ~250k fit the derived 257,536 context as-is: restart (clears prompt cache), nfswarm, Run, scratchpad mimo9/hc.sh
  (memwatch.sh writes ABORT at +2 GB swap; owner rule: >=8 GB headroom under each wired limit, abort at warn).
  300k/350k need a chunk below KV_CACHE_STEP (256) in layer_split.rs (chunk 64 cuts the workspace 4x) + a test.
  Prompts: mkprompts.py (MiMo tokenizer) — swarm.rs is 1.8 MB (~440k tokens), enough for 350k.
- UI findings: engine hero says "Stopped / no model mounted" while the split serves; split card estimates disagree
  (38k/49k/78k "fits only at" vs preflight 181,504 vs Ready 257,536); "goose's measured runs could not be read: the
  split reports runner mlxLmPipeline, which goose's plan does not key".
- End state: no ranks on either Mac; dev app quit; installed app restored (pid 51399, CDP 9897). Config restored
  (scratchpad mimo9/config.before-context.yaml). Screenshot /Volumes/AI-workhorse/media/goose/mimo-2026-10-26/.

## Session 10 (2026-10-27 08:30-13:30 EET): prefill cause fixed, memory model, 128k/250k/275k live, NFS + UI fixed
- Commits (mimo-distributed): 09ec8d0bf5 planner small chunks + adaptive step + PREFILL_PACE; 1a6e21d5b1.. load turns;
  d25db67adb scores x2 (pair 257) + gpu_headroom_gb + no-cache fallback; 7b6206228b node_gpu_headroom_gb; 879670cf5e
  adopting_extend (no KV copy into an empty generation batch) + end-copy charge; 3cf09e33ae per-shard eval + file-cache
  drop; wire-touch commit; final: rank 0 loads LAST after dropping the folder's file cache (msync MS_INVALIDATE).
  UI (agent): 590c272758 hero, 22ff9e614a card context = layer-split plan, 5bd17ea712 measured runs key mlxLmPipeline.
  NFS (agent, machine-sync 61f1b54): mule mount hard,intr,deadtimeout=180 (soft mount gave ETIMEDOUT -> Failed to open).
- PREFILL CAUSE: attention maths is ~60 s of a 64k prefill; the rest was rank 0's weights compressed by the kernel
  (MLX arrays are not wired until the GPU uses them) whenever the workhorse's RAM was contested: the mule's NFS reads
  through its file cache, TTS jobs, a GitHub Actions runner (16 GB), our own builds. 64k: TTFT 562 -> 218 s.
- RIG (scratchpad mimo10/): app.sh up|down, setctx.sh <ctx>, go.sh <tag> (reload, Run, wait PIPELINE_WARM both, load
  facts), hc.sh <key> (bench + abort -> stop.sh), memwatch.sh (3 s; ABORT at pressure warn or swap +1 GB in a run;
  load grace 20 samples / +2 GB while LOADING exists), stop.sh (reloads if the Stop button vanished), mkprompts2.py
  (swarm.rs prompts + answer key prompts.json.key). Holds: perl "mimo-longctx benchmark hold" on both Macs (node-probe
  BUSY -> TTS jobs queue): pids in mimo10/hold.wh, hold.wm — KILL BY PID when done.
- Config used (/tmp/gmimo/root/config/config.yaml mlx_distributed): context <N>, gpu_headroom_gb: 8.0. Restore after.
- MEMORY MODEL: peak/rank = weights + 2 x full_layers x 2560 B x N (live + end copy) + chunk x width x 257 B.
  19/29 weights 68.28 / 100.31 GB. Under 8 GB headroom the mule caps the context at ~277.5k: 300k short 0.65 GB,
  350k short 1.94 GB (planner refusals on the card, 12:00). Without the headroom rule they would plan.
- WAITING TRAPS: a CI runner on the workhorse (actions-runner-*), fam-lane TTS node jobs, our builds: wait them out
  before a load (the 10:24 and 12:10 loads were stopped for swap). go.sh's "NO RANKS" = the plan refused.
- RESULTS (bench mimo10/bench.py direct to :8191, swarm.rs prompts, key in prompts.json.key; peaks MLX, GB of 83.5/115.4):
  64k (18/30, 67,072): TTFT 218 s, prefill 293 tok/s, decode 28.0, peaks 74.8/114.8 (BEFORE the x2 fix: mule 0.6 GB
  under), answer 2/3. 128k (19/29, 131,072): 512 s, 250, 22.3, 72.9/105.9, 2/3. 250k (253,184): 1,594 s, 157, 15.8,
  74.3/107.6 (mule 7.85 under), 0.5/3 (garbled self-corrections). 275k (277,504, max under the 8 GB rule): 1,826 s,
  151, 15.0, 74.9/108.4 (mule 7.05 under: the model still under-counts ~1 GB on rank 1 at 275k), 1.5/3.
  300k / 350k: refused by the planner under gpu_headroom_gb 8 (mule short 0.65 / 1.94 GB); not run.
- OPEN: (1) the ~1 GB rank-1 under-count at 275k (find the term; until then keep headroom 9 for >250k); (2) answer
  quality past 128k degrades (garbled lines at 250k) — check the bf16 SDPA fallback numerics at long width; (3) the
  load fix (rank 0 last + folder cache drop) measured clean ONCE (12:36); confirm on the next loads.
- End state: ranks stopped on both Macs, holds killed, dev config restored (no context/headroom), installed app back
  (goose-single-guard --check OK).

## Session 11 (2026-10-27 13:15 EET, MiMo correctness + cache + Gauntlet agent; stopped at ctx-guard 350k)
DONE (pushed to mimo-distributed): 1b12fafa1e TOOL_TURNS_THINK (MiMo tool turns keep the template's thinking-on;
the rank used to force enable_thinking=false on every tool request, cloud baseline thought); 312cf5a3e reverted a
redundant last-message cut; 85dae67e85/b8bb49344f real-mlx_lm test
`a_thinking_mimo_chat_without_a_tail_reads_its_conversation_through_real_mlx_lm` (cached == LRU restore ==
prompt[k-1]-1, rings past window, warm == cold). sidecar 509 lib + integration green on the laptop (port_holders flaky
under parallel load, 3/3 alone). Correct-usage audit and cache design: skill goose-mlx-inference, section
"MiMo-V2.6-Flash on the mlx_lm pipeline split".
LIVE CACHE (13:20, split 18/30, plan context 119,808, served window 118,016; client scratchpad p1/cachetest.py,
results p2cache/results.jsonl): 10,177-token prompt cold TTFT 23.6 s, repeat 0.15 s (cached 10,176), greedy output
identical; conversation V cold 25.7 s vs 0.48 s, identical; W = A's conversation + new turn cached 10,176 (0.44 s);
agent chat with tools, thinking on (reasoning 11-85 chars per turn), 3 tool calls parsed, every answer correct incl.
item 17 from 4k tokens back, each turn cached = previous prompt - 1. The owner's "3 cached of 15,597" was a cold
one-shot prompt (3 = mlx_lm system segment). Verdict: reuse real, counter right, SWA restore exact.
STATE LEFT RUNNING FOR THE NEXT AGENT: dev app pid in scratchpad mimo10/app.pid (CDP 9461, profile /tmp/gmimo),
split serving on :8191 (/v1, model id mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8). Installed app QUIT.
OPEN (in order):
1. Gauntlet sb-7.2 on local MiMo from the dev app's Benchmark view over CDP (port 9461): provider OpenAI configured
   with host http://127.0.0.1:8191 (Configure providers; base path v1/chat/completions), entrant Single model, Model ID
   as above, clear "Stop a run at $". Procedure = ~/goose-builds/loop-state/sb72_chain.py launch() (lines 275-322);
   CDP tools with port env: scratchpad p2tools/{cdp.mjs,cdpkeys.mjs} (`CDP_PORT=9461 node cdp.mjs eval '...'`,
   `node cdpkeys.mjs OpenAI`). Before: ~/Projects/machine-sync/bin/ai-disk-guard --need-gb 5 --out
   /Volumes/AI-workhorse/runs/2026-10-27-mimo-gauntlet. Verify `ps -A -o command= | grep 'run_build.py --entrant'`.
   Record app version, engine argv (ps), provider+host, one rendered prompt showing thinking (rank log
   tool_turns_think true; responses carry reasoning).
2. Cloud baseline already on the board: Gauntlet sb-7.2 0.7909 (2026-10-02 OpenRouter xiaomi/mimo-v2.6-flash,
   https://leanzero.net/agentic-benchmarks/run/brun-029b6e80-82b5-49e1-83f9-e501cd296cb8). Compare overall + per
   section; if local is clearly below, find why (logs, template, parsing, truncation at 118k window/compaction,
   quant) before accepting. Run audit: goose-benchmark-iteration scripts/run_audit.py on the run dir.
3. Owner: if Gauntlet is close with no unexplained failures, Forge 2.0 on local MiMo (forge2-local/dispatch with
   port 9461; its preflight expects a single rapid-mlx engine, use --no-preflight + own checks). Cloud Forge 2.0:
   0.5283 median of 2 (0.5538/0.5028, 2026-10-11). Otherwise investigate, skip Forge.
4. Publish local scores from the app (skill "Publishing from the app"), then dated entry in goose-mlx-inference.
5. LAST: touch scratchpad/ngram-go (only after Forge 2.0, or after the gap is explained if Forge is skipped).
6. After: dev app down + installed app back: scratchpad mimo10/app.sh down; goose-single-guard --check.
TRAPS: Mac memory (CI runner / TTS on workhorse) can stop the split (watchdog); NFS mule mount now hard (61f1b54);
reload ~271 s via mimo10/go.sh. Local prefill slows past ~19k tokens (64k TTFT 218 s cold), so a cold re-read after
a cache eviction or compaction is minutes.

## Session 12 (2026-10-27 13:20-21:40 EET): local MiMo Gauntlet sb-7.2 = 0.5894 vs cloud 0.7909; image fix; plan
FACTS (article): model mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8 (MoE experts native MXFP4, rest 8-bit, 156 GB,
TEXT-ONLY conversion: vision/audio encoders and MTP not included, per its README). Split = mlx_lm pipeline over
JACCL/RDMA on the Thunderbolt link (rdma_en3, link reports 80 Gb/s): workhorse Mac Studio M3 Ultra 96 GB (Mac15,14)
layers 30-47 (18), workmule Mac Studio M5 Max 128 GB (Mac17,14) layers 0-29 (30). Served window 119,808 (run 1:
118,016), goose compacts at ~0.88 of it. App = dev build 3.0.132 from mimo-distributed (9411914e0f), provider openai
-> http://127.0.0.1:8191, thinking on (tool_turns_think true; reasoning in every turn), sampling 1.0/0.95.
- Run 1 (cloud-d078261f, 13:26): did_not_finish after 2h08m: read_image(selftest.png) sent an image part, :8191 404
  "Only 'text' content type is supported", the 404 was retried then fatal. 2 compactions before it died.
- FIX 9411914e0f (pushed): the openai provider at a loopback host asks /v1/models before sending an image (text-only
  engine -> named placeholder + one chat notice); an endpoint that still refuses gets the turn once more without the
  image; that refusal is a permanent 4xx in retry.rs. Tests + negative control (test fails without the fix: 6 bodies).
  LIVE: run 2 read screenshot1.png, got the placeholder, carried on.
- Run 2 (cloud-a6f8f01a, 16:44-21:26): score 0.5894 (raw 0.8083, inner 0.8231, ADMISSION CEILING 0.599 because
  s_visible_surface failed), 130 calls, 6.99M prompt / 334k completion tokens, decode 27.4 tok/s, agent 4h32m +
  scoring 9.5 min, 5 compactions (55, 106, 171, 223, 265 min in; max prompt 116,248). Cloud: 0.7909, 131 calls,
  37.0M prompt, 0 compactions at 1M, 2h08m.
- PUBLISHED BY MISTAKE: brun-0312730b-6286-4bb1-8dcc-6e9d0f57c2bb ("MiMo-V2.6-Flash, run locally split across two
  Mac Studios", community row, 58.9%) went live at ~21:35, minutes before the owner's "do NOT publish yet" arrived.
  It is still live. Removal is the owner's call (Sanity doc delete + revalidate; skill "Board audit" query).
- End state: ranks stopped on both Macs, holds killed, dev app quit, dev config restored (config.yaml.before-s12),
  installed app back on 9897 (goose-single-guard OK on both). ngram-go NOT touched (owner). Run 2 dir:
  /tmp/gmimo/userdata/benchmark/runs/build/openai-cloud-a6f8f01a-...-r0 (verdict.json, sessions.db); run 1 dir moved
  to <scratchpad>/s12/run1/build. Copy what is needed before /tmp is cleared.

### Session 12 plan (tomorrow)
1. PARITY CHECK before any local-vs-cloud run: `~/goose-builds/loop-state/local-parity/parity_check.py --port 8191`
   (b20669d in loop-state; PASS/FAIL table, exit 1 on any FAIL). Rows: served window >= 256k AND goose's
   GOOSE_CONTEXT_LIMIT set to it (no compaction where cloud had none; split proven to 275k at gpu_headroom_gb 8-9,
   setctx.sh 262144); vision parity (local /v1/models capabilities vs cloud input_modalities); thinking on
   (E_RANK_PIPELINE_SPLIT tool_turns_think); sampling = card (1.0/0.95 from /v1/status); tool parser qwen3_coder;
   max output (GOOSE_MAX_TOKENS unset or >= 32k; one think measured 20k tokens); no read timeout configured.
   VISION FINDINGS: XiaomiMiMo/MiMo-V2.6-Flash-RL is natively multimodal (OpenRouter xiaomi/mimo-v2.6-flash
   input_modalities text,image,video,audio; config vision_model_type mimovl). Our mlx-community quant has NO vision
   tower (index = model.layers/embed/lm_head only). mlx-vlm HAS mimo_v2 vision (mlx_vlm/models/mimo_v2/vision.py);
   ggml-org ships mmproj for the GGUF. Our pipeline split (mlx_lm) cannot serve images. Whether the cloud run
   ACTUALLY sent images is UNVERIFIED: its run dir (2026-10-02) is gone from this Mac; only the board doc remains.
   So until proven otherwise: the cloud COULD see its screenshots, local could not. State it next to any score.
2. PER-TASK DIFF run 2 vs cloud (verdict.json vs Sanity brun-029b6e80 checksSummary; lost points and cause):
   s_visible_surface 0 vs 1 (109/120 predicted pixels matched; this alone sets the 0.599 ceiling = most of the 0.20
   gap), t_height_pixels 0 vs 1 (0/6 tower tops within 3 px), q_inspector_framing 0 vs 1, s_currency_collar 0.5 vs 1,
   t_labels_culling 0.58, t_vs7dbg_truth 0.7 (restPixel False), q_overview_legibility 0.71, t_camera_math 0.86,
   t_brush_link 0.89, v_presentation_text 0.75, c_b5_generation_304 0.67 vs 1. Local WON: r_b3_sigkill_resync,
   r_b6_outbox_atomic, r_notifier_exactly_once, p_stream_apply, t_stream_diff, j_error_state, e_stream_apply_latency.
   CAUSE so far: the losses are all 3D PIXEL geometry, the backend is better than cloud. heightOf() in web/viz.js
   matches the contract (h = clamp(0.9+0.55 log10(a_major),0.2,4.2)), so the error is in projection/geometry, which
   only a screenshot shows: classed IMAGE (its one screenshot read was withheld) + MODEL ERROR. Compaction: 5
   summaries kept the contract file names; not shown to drop the geometry rules, NOT PROVEN either way. Quant and
   timeouts: no evidence (0 failed calls, no timeouts). TODO: diff viz.js projection vs SB7-CONTRACT camera/geometry
   and read the 5 summaries for what they dropped, to class each lost check as compaction vs image vs model.
3. RUN STEPS: (a) wait until node-probe shows no TTS/fam jobs on workhorse+workmule; place holds
   (perl $0="mimo-gauntlet benchmark hold"; kill by pid after); (b) mimo10/app.sh up; config: add OPENAI_HOST
   http://127.0.0.1:8191, OPENAI_BASE_PATH v1/chat/completions, provider_default_models.openai, GOOSE_CONTEXT_LIMIT
   262144, mlx_distributed context 262144 + gpu_headroom_gb 9 (app QUIT while editing); (c) mimo10/go.sh; (d)
   parity_check.py must exit 0 except the vision row (documented caveat); (e) Benchmark view over CDP 9461:
   Gauntlet, Single model, Refresh providers, cdpkeys.mjs OpenAI, Model ID, clear $ stop, Run benchmark; verify
   `pgrep -fl "[r]un_build.py --entrant"` (bracket the pattern: an unbracketed pgrep inside a waiter matches itself);
   (f) at scoring: stop the split (mimo10/stop.sh), drop holds; (g) publish ONLY on the owner's word; (h) app.sh
   down, restore config, single-guard --check on both Macs; touch ngram-go only when the owner says.

## Session 12 vision (2026-10-27 21:30-23:00 EET; stopped at ctx-guard 350k)
GOAL: local MiMo sees images like the cloud (Gauntlet parity). Owner widened to omni (image/video/audio), then
PIVOTED to a ready omni MLX build ("find a really GOOD version"); old mlx-community RL mxfp4-q8 folder was DELETED by the
coordinator (SSD 615 GB free). Old dev profile backed up: scratchpad mimo10/config.yaml.before-s12-vision; /tmp/gmimo
config now names EigenLabs/MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp (both nodes' model_dir).
CHECKPOINT: cloud baseline (Gauntlet 0.7909, 10-02) was OpenRouter pinned to provider "xiaomi" (sb72-openrouter-queue
"pin": "xiaomi"); Xiaomi's blog: MOPD served on their API since 2026-09-25 06:00 UTC+8 under the same name -> cloud = MOPD.
PICK: EigenLabs/MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp (170.97 GB; text tensors byte-for-byte the same sizes/recipe as the old
build: experts upstream MXFP4 repacked, attention/dense/embed/lm_head 8-bit affine g64; + vision_tower 1.457 GB bf16,
audio 0.277 GB, MTP 1.976 GB bf16; mlx-vlm key layout; no open "broken" discussions). Rejected: vogel61 oQ4e (same
recipe, encoders in side files, MTP 8-bit, 0 likes), nativ RL-4bit (RL), TensorFold RL-4bit-MTP (most popular, 11 likes,
but TEXT-ONLY and 4-bit attention), inferencerlabs Q9 (needs modified MLX, text-only), REAP builds (pruned).
DOWNLOAD: waiter pid in /Volumes/AI-workhorse/scratch/mimo-mopd-dl/waiter.pid waits until the voice jobs (rr-voice,
run-gates, voice_split, owner_gates) are gone 10 min, then node-run (heavy slot, guard 175 GB) runs dl.sh: hf download at
the pinned revision -> /Volumes/AI-workhorse/models/goose/EigenLabs/MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp, then SHA-256 of
every file vs SHA256SUMS.json ("verified N bad 0" + DONE). Logs: waiter.log; node-jobs list. CHECK IT FIRST TOMORROW.
The mule reads it over NFS at /Users/workmule/.goose/models/EigenLabs/... (same mount as before).
BUDGET (layer_split test an_omni_checkpoint_charges_..., prints max context under 8 GB headroom): run
`cargo test -p goose-sidecar an_omni_checkpoint -- --nocapture` (node-run on a free Mac) and record the three numbers.
Hand estimate from the 10-27 memory model (~53 KB/token across both ranks, discrete 3.5 GB layers): text ~277k; vision
bf16 on rank 0 ~250-265k; vision+audio+MTP ~205-225k (< 256k) -> audio + MTP are NOT loaded (sanitize drops them;
planner charges them nowhere). If vision-bf16 lands < 256k: GOOSE_MIMO_VISION_BITS=8 halves it (0.73 GB; parity not yet
measured at 8-bit).
CODE (all committed): fork Rapid-MLX lz/pipeline-mimo-omni 1e5d581d1 tag lz-pipeline-qwen4.16 (pushed):
rapid_mlx/mlx_lm_models/mimo_v2_media.py (ViT from mlx-vlm 0.7.10 + band attention in query blocks; Xiaomi preprocessing
from config.json processor_config; markers; MediaStore; MediaFrontend; expand) + mimo_v2.py (prefix strip in
ModelArgs.from_dict quantization + sanitize; substitute before layer 0 on the LAST pipeline rank) + tests/mimo_media/.
goose mimo-distributed 1a2b0b8267 (pushed): rank_media.py, capabilities "vision","video", layer_split omni roles, pin.
PARITY (torch CPU HF reference vs ours, tests/mimo_media/vision_parity.py prep/ref/cmp; ref python
/Volumes/AI-workhorse/venvs/voice-qa/bin/python): float32 cosine >= 0.99998 every layer (4 images, up to 1,900
patches); bf16 final 0.990-0.9999 vs torch-bf16 0.993-0.9999; patches == HF Qwen2VLImageProcessor exactly. 15 unit tests
green (pytest at scratchpad pydeps; MIMO_TOKENIZER_DIR=/Volumes/AI-workhorse/scratch/mimo-vision-parity).
LOW-CONFIDENCE ITEMS: (1) normalisation: upstream preprocessor_config + vLLM = ImageNet (our default); SGLang PR #40888
+ mlx-vlm/EigenLabs = CLIP. A/B LIVE: solid red/green/blue squares + "what colour", GOOSE_MIMO_IMAGE_NORM=clip vs
imagenet. (2) rank_media.py wiring has NOT run (no tiny-model e2e yet): first live load will show RANK_MEDIA lines;
traps to watch: apply_chat_template may return a dict/BatchEncoding (expand only fires on list), settle()/_tokenize call
order, manifest pickled to the mule each request (features shipped once per image; peer LRU 512 MB). (3) video: ffmpeg
path on rank 0 only, timestamps per SGLang, capped at GOOSE_MIMO_VIDEO_MAX_TOKENS=16384 (Xiaomi allows ~131k) - untested.
AUDIO: NOT wired (prepare refuses audio parts loudly). Plan: mel 24 kHz/128 mels/hop 240 (config processor_config),
mlx_audio has codec/models/mimo_audio_tokenizer (env has it) -> 20-codebook codes, group 4 -> audio_encoder (6-layer
local transformer + 2-layer projection, mlx-vlm mimo_v2/audio.py) -> 6.25 tokens/s; needs audio_encoder + tokenizer
encoder on rank 0 (~0.28 + 0.65 GB) only when asked. MTP: weights present, not wired (oMLX reports 67-96% acceptance).
NEXT (in order): 1 verify download; 2 cargo test -p goose-sidecar (layer_split + launch tests) on a free Mac; 3 pipeline
env on BOTH Macs to the new pin (goose provisions by PIPELINE_FORK; check pip shows rapid_mlx @1e5d581d1); 4 load the
split (mimo10/go.sh) when node-probe shows no voice jobs; 5 live tests: text sanity, /v1/models capabilities, rendered
table screenshot with a known cell, colour A/B, then a Gauntlet sb-7.2 run (Session 11 OPEN 1-6 still apply).
CLEANUP: /Volumes/AI-workhorse/scratch/mimo-vision-parity (1.7 GB encoder copy for parity) - delete by exact path once
the full build is verified (parity can rerun with --weights pointing at the build's model-00036-of-00037.safetensors).

## Session 13 (2026-10-28 05:22-06:45 EET): MOPD omni live; vision works; 2-row batch crash blocks Gauntlet
STARTED 05:22 after both Macs had 10 min with no voice/TTS job (waited from 23:39). Holds were perl
"mimo-mopd benchmark hold" + ~/.leanzero/benchmarks/<pid>.json marker (node-probe BUSY); RELEASED at stop.
END STATE: dev app quit, installed app back on 9897 (single-guard OK both Macs), holds killed, no ranks.
ngram-go NOT touched (stopped at ctx 351k, as instructed). /tmp/gmimo config KEPT for the next run:
mlx_distributed context 114688 + gpu_headroom_gb 8.0; OPENAI_HOST http://127.0.0.1:8191, OPENAI_BASE_PATH
v1/chat/completions, GOOSE_CONTEXT_LIMIT 114688, provider_default_models.openai = the model id. Backup of the
pre-s13 file: <scratchpad>/s13/config.yaml.before-s13. Rig: <scratchpad>/s13 (ask.py, pair.py, launch2.py, tools/).
COMMITS (mimo-distributed, each with a test that fails without it):
- acb9f02730 preflight proves the LAYER split's fork env (pipelinePython) -> a pin bump rebuilds it at Run
  (before: only qwen4_exp was proven; the 07126f051 envs ran against the 1e5d581d1 pin). Also fixed the stale
  tensor-program test (rank_media.py missing; it failed at 1a2b0b8267).
- ff3deeda31 checkpoint_weight_map + CheckpointIndexJson: mlx_lm sharded_load looked up model.* in an index
  keyed language_model.* ("Pipeline loading is only supported for MLX converted models").
- e210b73e2f rank_media wrap_chat_template: the wrapper was set THROUGH TokenizerWrapper (forwarded to HF,
  "maximum recursion depth exceeded" at readiness) and read tokenize default False (server passes none).
CONTEXT: 256k does NOT fit with the vision tower under the owner's rules (5% reserve, 8 GB under the Metal
limit). Planner measured: 19/29 needs ~75.1 GB on the workhorse at 262k vs budget ~71-72 (available ~76-77.6
GB); 18/30 needs the mule over its 107.45 ceiling above ~116k. Largest stable = 114,688 (both splits fit).
The ctx was bisected by reading the card text after setctx.sh + reload.
LIVE RESULTS (split 18/30 then 19/29, 241-246 s cold loads x5, 0 NFS errors):
- /v1/models: context_window 114688, capabilities text,vision,video,tools.
- IMAGE: rendered table (s13/table.png): Delta/Q3 = 8,416 and largest Q1 = Charlie: both right.
- COLOUR A/B (GOOSE_MIMO_IMAGE_NORM in the dev app's env reaches rank 0): basic 12 colours 12/12 both;
  pastel 12: ImageNet 11/12 (coral->apricot), CLIP 12/12; shuffled 20 CSS names 18/20 both (same swap);
  low-contrast codes right both; mean logprobs within 0.01 everywhere. VERDICT: no measurable difference ->
  default stays ImageNet (upstream preprocessor_config, vLLM). No code change.
- CACHE: 100k prompt cold TTFT 360 s (277 tok/s prefill), repeat TTFT 0.52 s; decode 24.8 tok/s; swap flat;
  peaks rank0 73.1 GB / rank1 104.5 GB (18/30).
- PARITY (parity_check.py): PASS vision, provider, sampling 1.0/0.95, thinking (tool_turns_think), qwen3_coder,
  max output, no read timeout; FAIL context 114,688 < 256k (and GOOSE_CONTEXT_LIMIT = 114,688).
BLOCKER (OPEN, design not settled): Gauntlet launch 06:20 (cloud-17f73a42, run dir /tmp/gmimo/userdata/
benchmark/runs/build/openai-cloud-17f73a42-...-r0) died 1 min in: rank 0 "[jaccl] Recv failed with error
code -12" (= ibv_post_recv ENOMEM, MAX_RECV_WR 32 full: recvs posted the peer never matched) during a 2-ROW
batch (req-4 963 tok + req-5 2422 tok, width 2425). REPRODUCED on a fresh load, no images: s13/pair.py
(two concurrent 1.7k/4.4k prompts): pair 1 ok, pair 2 -> rank 1 "[METAL] GPU Timeout" with y2 joining while
y1 prefilled (rank0 log: 1736/1737 then 2048/4459). Session 12's text-only run batched 2 rows 5x with no fatal
(split 18/30, old pin). Suspects, unproven: the 19/29 split (cache_weights full 5 vs 4), a cold row joining a
prefilling batch on the pipeline. Candidate fix: in rank_wrapper.room_for, a pipeline split admits a new
request only when rows == 0 (rank 0 alone decides what is shared, so ranks stay paired), with a named field
in RANK_ADMISSION. Check first that `prefill` is set for the pipeline spec (rank0 log had NO RANK_ADMISSION /
RANK_PREFILL lines: the admission path may not run there at all -> then the hold must go in the pipeline path).
Prove with pair.py x3 on a fresh load before relaunching the Gauntlet.
ALSO: MediaRequests.get_nowait calls media_prepare twice (Queue.get_nowait -> self.get); harmless for text,
fix it with the above. go.sh's memwatch kill was `pgrep -f mimo10/memwatch.sh | xargs kill` (pattern);
changed to a per-pid loop on the full scratchpad path. CDP: the dev app has TWO page targets (main window
and #/engine-glance); mimo9/cdp2.mjs picks the one whose URL has 'leanzero-swarm' -> PAGE=benchmark when the
main window is on the Benchmark view. Launch: s13/launch2.py (exec of sb72_chain.launch tail, port 9461 tools;
pick OpenAI with tools/cdpkeys.mjs OpenAI first).
NEXT: 1 wait for no voice jobs 10 min (s13/wait_clear.sh), holds (s13/hold.sh); 2 fix the 2-row crash + test;
3 rebuild (live/build.sh), app.sh up, go.sh, pair.py x3, parity; 4 Gauntlet via launch2.py; at BENCH_PHASE
score stop the split; 5 compare per task: cloud = s12/cloud_checks.txt (0.7909), old local =
s13/old-local-verdict.json (0.5894); 6 publish a NEW entry (pub_local.py-style title "MiMo-V2.6-Flash MOPD
4-bit · 2-Mac split · vision, 114k"), notes = caveats (114k not 256k; cloud 1M); 7 cleanup; touch
<scratchpad>/ngram-go LAST.

## Session 14 (2026-10-28 15:05 -> 10-29 02:40 EET): 2-row crash + token desync fixed; 256k; Gauntlet 0.6605 published
HOLDS: s13/hold.sh on both Macs (released at the end; a stale mule marker 39898.json from the first hold removed).
A voice-QA job (workhorse-20261028-180900-8853) ran 18:09-18:12 despite the hold: waited, not preempted.
COMMITS (mimo-distributed): 0d398f8a70 media queue prepares once (get_nowait double prepare; test fails
['a','a','b']); af41f7949d pipe trace accepts plain ints (all_sum(0) killed both ranks); 84d18c7744 FIX 1;
e8f6ae9fd5 gpu_headroom_gb doc = test margin, not an owner rule; 288c838e35 FIX 2.
FIX 1 (2-row crash, session 13's -12 / GPU Timeout): mlx_lm BatchRotatingKVCache.extend pads an empty row with
mx.array([]) = float32 (Q-447 fixed only BatchKVCache; MiMo has 39 SWA layers). Trace (fresh load, pair.py):
rank 1 `send (2,768,4096) float32` vs rank 0 `recv_like (2,768,4096) bfloat16` (recv buffer typed from rank 0's
own embedding) -> next shapes unpaired -> GPU timeout. Admission DOES run on the pipeline spec (prefill is Some;
RANK_ADMISSION only logs a HOLD); the rows==0 hold was not the cause and was not added. Proof: pair.py x3 OK on a
fresh load (129k) + x3 on the final 262k build, 0 float32 p2p transfers.
FIX 2 (Gauntlet run 1 crash 19:02:56Z, rank 1 GPU Timeout): the ranks sampled their own tokens. `ended` CRC
checkpoints matched for 69 answers, then diverged at the run's first image request (uid 70 from token 9-16,
uid 71 from token 2) -> rank 1 cached a different answer -> req-73 had a shorter cached prefix on rank 1 ->
trace op #430,957: rank 0 `recv_like (1,103)` (last chunk) vs rank 1 `send (1,256)`. Fix: rank_pipeline_split
wraps server._make_sampler: every rank returns all_sum(rank 0's token, zeros elsewhere). WHY the samples first
differed on images is NOT isolated (no mx.random in the media path; M3 Ultra vs M5 Max lm_head) -> open item.
Verified: image asks at temp 1.0 + pairs -> all ended CRCs equal; run 2 (13+ image requests, 2 compactions) clean.
CONTEXT 262,144 (owner correction): gpu_headroom_gb 8 -> 0 in /tmp/gmimo config (it was a test margin stacked on
the 5% reserve). Planner KV = 9 full x 4 kv x (192+128) x 2 = 23,040 B/token (matches owner arithmetic) + one
prompt-cache copy (measured) + workspace = chunk x width x 257 (scores+softmax, measured): no planner bug.
Split 19/29, planned 75.1 / 107.05 GB. Probes: 124,597 tok (129k load) TTFT 608 s, peaks 72.5/105.5 GB;
210,724 tok (262k load) TTFT 962 s, 218.9 tok/s prefill, 17.7 tok/s decode, peaks 75.9 / 109.1 GB, swap flat
on both. Gauntlet peaks rank 1 109.7 GB; swap never grew. /tmp/gmimo config KEPT at 262144 / headroom 0 /
GOOSE_CONTEXT_LIMIT 262144 (it is the isolated test profile).
PARITY: 9/9 PASS incl. context 262,144 >= 256k.
GAUNTLET sb-7.2: run 1 openai-cloud-122c5484 (18:3x-21:03, died, FIX 2); run 2 openai-cloud-a16c45bf
(21:18-02:24) SCORE 0.6605 vs cloud 0.7909 / old local 0.5894. 12 checks below cloud: v_presentation_text 0.25,
c_webhook_discipline 0.6 (forged -> None), t_labels_culling 0.64, v_styling/t_coast_identity/b_total_field 0.75
(total 12288 want 12289), t_brush_link/s_tower_geometry 0.78, t_vs7dbg_truth/t_pick_buffer 0.8,
q_overview_legibility 0.83. 7 above cloud: t_stream_diff 1.0 (cloud 0), r_b6_outbox_atomic, r_notifier_exactly_once
1.0, r_b3_sigkill_resync 0.67, p_stream_apply 0.5, j_error_state 0.3, e_stream_apply_latency 0.25. E gate FAILED
(fraction 0.925; e_optimistic_paint 0, e_stream_apply_latency 0.25); m_committed_event_replay 0 (cloud 0 too).
verdict copy: <scratchpad>/s14/verdict.json. Ranks stopped at scoring by s14/watch.sh (killranks by pid).
BOARD: NEW entry "MiMo-V2.6-Flash MOPD 4-bit · 2-Mac split · vision, 256k", 66.0%,
leanzero.net/agentic-benchmarks/run/brun-79c4f078-8a57-41e7-a610-b8261f934d6f (200 signed out). The app's publish
card has NO notes field, so caveats live here: 256k vs cloud 1M (2 compactions); 4-bit local quant; 2nd launch
(run 1 crashed on FIX 2); built on mimo-distributed dev build, not a release. Old entry untouched.
CDP traps: Gauntlet Single model -> "Refresh providers" first (OpenAI hidden until 8191 answers); cdpkeys.mjs's
focus walk misses -> s13/tools/pickprov.mjs (ArrowDown then click the option). BENCH_PHASE goes to the app
only; scoring = <run dir>/graded-sb7-db appears. Call budget = 150 turns (bench_budget.CALL_BUDGET).
END STATE: dev app quit by pid, installed Goose Swarm on 9897, goose-single-guard OK on both Macs, no ranks,
no holds/markers, /tmp/goose-pipe-trace flags removed. ngram-go NOT touched.
OPEN: why ranks' samples diverged on image requests (FIX 2 masks it); E gate (optimistic paint, stream latency).

## Session 15 (2026-10-30 22:00 EET ->): main merged; re-Gauntlet sb-7.2 with compaction r4-6 + wrap-up nudge
COMMITS: 2e1304d70a merge main ed0d7d1303 (3.0.136) into mimo-distributed (conflicts: openai.rs = main's, the
branch's fix is main's b173e17213 byte for byte; i18n + MlxEngineView keep both sides; main's HANDOFF.md kept as
HANDOFF-floating-window.md; engine.rs ROPE_SCALING_MODEL_TYPE so distributed::config's "no quoted model_type
outside RUNNERS" test passes). 8b6d5e4d4f sb7.2 + forge2 manifests refrozen for the nudge's harness bytes (main
ships 1c71262a7d without a refreeze -> `pnpm run package` refuses "Packaged benchmark differs ... bench_budget.py").
TRAP (workhorse): Homebrew `openssl` is now 4.x; llama-cpp-sys-2 0.1.146's vendored httplib fails (const
X509_NAME). Fix = <scratchpad>/s15/env.sh (OPENSSL_ROOT_DIR/INCLUDE_DIR/DIR + CXXFLAGS/CFLAGS -I openssl@3), AND
delete the stale cmake cache target/debug/build/llama-cpp-sys-2-<hash>/out/build (it caches /opt/homebrew/include).
FLAKY: goose-sidecar tests/port_holders.rs (process/port timing) fails ~1 in 3 under load; identical on main.
NUDGE IN THE BENCH: run_build.py (1c71262a7d) switches the nudge ON for every budgeted entrant and strips the
operator's GOOSE_WRAP_UP_* from the snapshot: running WITHOUT it needs the three harness files reverted + refreeze.
Audio on the split: /v1/models declares text,vision,video,tools (no audio) -> openai.rs withheld_media() sends
attached audio as its named placeholder (loud to the model, not an engine error); images stay in the agent turn
(media pre-pass sends images only when the engine declares audio), so rank 0's vision path is unchanged.
Readiness notes: /Volumes/AI-workhorse/runs/2026-10-30-mimo-split-regauntlet/NOTES.md. Rig: <scratchpad>/s15
(gate.sh, build.sh, replay.py growing-session replay from the 10-28 run's real session, probe.sh, wait_free.sh).
LAUNCH 01:08:20 (10-31) openai-cloud-51bfaccd on 18/30 at 262,144 with nudge v2 (build eca39ae893: split declares
reasoning_controls ["enable_thinking"], provider sends no graded effort there; the 00:30 launch 48c30cdd died on
that 400). Planner refuses >= 266,240 today (workhorse available 74.6 GB). A reload under other sessions' voice-qa
load re-planned 17/31 (rank 1 113.3 planned vs 115.45 ceiling): never run that; stop, free the workhorse, re-plan.
Vigil: run-causes/20261031-002917-mimo-split-sb72-s15/VIGIL.md; ranks stop at scoring via s15/watch.sh.
RUN 51bfaccd DIED 04:18 (10-31) at 97/150 calls, UNSCORED (incomplete-agent.json). Not the split: the newsroom launchd
PREPARE tick (claude -p /leanzero-newsroom pid 20476) started `mflux-generate-ernie-image-turbo` (pid 93635) on the
workhorse at 04:17:58, outside node-run; kernel pressure critical -> goose's distributed supervisor stopped the split
("the engine stops before the kernel kills it, and is not restarted"). Until then: 1 compaction at 211,784 -> 18,229,
summary graded A- by the vigil, vision used 3x and described truthfully, 0 rank faults, nudge not reached (fires 120).
Coordinator asked (04:20) to pause the newsroom image job / tick on the workhorse for ~5 h.
STATE AT HANDOFF (subagent stopped at the 350k ctx limit): dev app pid 70520 on CDP 9461 (installed app QUIT on the
workhorse; restore with app.sh down at the end), split STOPPED, holds ALIVE: workhorse perl pid 81165 (+ marker
~/.leanzero/benchmarks/81165.json), workmule perl pid 80887 (+ marker). watch.sh exited (RANK_FATAL). Vigil agent
for 51bfaccd will report the end and stop by itself. /tmp/gmimo config: context 262144, GOOSE_CONTEXT_LIMIT 262144.
RELAUNCH (all scripts in <scratchpad>): 1 confirm the newsroom tick/mflux is paused and no voice-qa/mflux/python GPU job
on the workhorse (ps); 2 `zsh mimo10/go.sh` -> must print split [18, 30] (if 17/31: stop.sh, free memory, retry;
never run 17/31); 3 `curl -s 127.0.0.1:8191/v1/models` (reasoning_controls ["enable_thinking"]), s13/ask.py
s13/table.png (8,416 / Charlie); 4 `c nav '#/benchmark'`, click "Single model", "Refresh providers", `cd s13/tools &&
node pickprov.mjs OpenAI`, `PYTHONPATH=~/goose-builds/loop-state python3 s13/launch2.py`; 5 verify run_build --sb72 +
vendor 200 on 8850; `nohup zsh s15/watch.sh <R> <L0> <L1> &` (L0 L1 from mimo10/logs.cur AFTER go.sh); 6 fresh vigil
agent with s15/VIGIL-BRIEF.md (new RUNID, new run-causes dir via long_run_gate.py check, same why/cheaper text).
END: publish from the app (sb72_chain.publish_label pattern: run row -> title input placeholder "e.g. My M4" ->
"MiMo-V2.6-Flash MOPD 4-bit · 2-Mac split · vision, 262k, nudge" -> Publish); verify the live run page signed out;
only then hide brun-79c4f078 (Sanity invalidated + invalidationReason, scratchpad hide-run.cjs / supersede.cjs);
release holds (kill 81165; ssh workmule kill 80887; rm both markers), app.sh down, goose-single-guard --check.
## Session 16 (2026-10-31 05:00 EET ->): relaunch 2 of the MiMo split sb-7.2
LAUNCH 05:13:40 openai-cloud-8985a86d on 18/30 at 262,144 (nudge v2, thinking on), holds 81165/80887 reused, dev app
70520 on 9461. CI runners on both Studios paused with leanzero-analytics runner_watch.py pause 7 (resume after the run:
`runner_watch.py resume`). TRAP: go.sh must run with the app on '#/leanzero-swarm?tab=mlx&mlx=engine' (on #/benchmark it
finds no placement card and prints NO RANKS); `mimo10/c click 'text=...'` does not do text selectors, use s16/ui.py
(sb72_chain cdp with SP=s13/tools, PYTHONPATH=~/goose-builds/loop-state). Vigil: run-causes/20261031-050648-mimo-split-sb72-s16.

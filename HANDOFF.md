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

## Session 2 (2026-10-26): DONE in commit "distributed: one runner table" (NOT yet compiled)
- config.rs: `Runner::MlxLmPipeline` ("mlxLmPipeline"), `pub const RUNNERS` table (qwen3_5/qwen4_exp/mimo_v2),
  methods `wired_summary`, `placement_kind`, `layout_words`, `uses_pipeline_env`, `runs_rank_wrapper`;
  re-exported `RUNNERS` from distributed/mod.rs. Test `no_other_file_maps_a_model_type_to_a_runner` (walks
  goose-sidecar/src + goose/src, non-test part of each file, flags `"<type>" =>` and `model_type == "`).
- Folded copies: mlx_distributed.rs split_way + load record, mlx_placement.rs x2, mlx_speed.rs, planner.rs
  (wired + label), discover.rs prose + pipeline-env check. Imports added (verify `PlacementKind` is in scope in
  mlx_distributed.rs split_way: it is imported only inside a fn at :591 -> add a top-level `use` or full path).
- NEXT: `cargo check -p goose-sidecar -p goose` (heavy-run) and add the MlxLmPipeline arm in every exhaustive
  match the compiler names (preflight 1409, supervisor 2250, provision for_runner -> Self::pipeline(), link_host 190
  (phase Warming->Ready also for MlxLmPipeline: use `runs_rank_wrapper()`), supervisor 1393 server_load
  (`runs_rank_wrapper()`), supervisor 2314 peers_follow_rank0 (decide: mlx_lm pipeline = same loop as tensor ->
  false? tensor peers do not follow rank0; check what the flag does), preflight 452/963 pipeline checks
  (`uses_pipeline_env()` for the env probe; the qwen4 PIPELINE_MODULE --help probe stays qwen4-only), runner_update 143.

## Measured facts (session 2)
- Byte table from the checkpoint headers (all 45 files, `model.layers.N.` grouping): layer 0 = 308,625,408;
  full-attention layers 5,11,17,23,29,35,41,47 = 3,519,366,144 each; the 39 SWA layers = 3,524,936,832 each;
  shared (every stage) = embed_tokens 664,010,752 + lm_head 624,951,296+19,529,728+19,529,728 + norm 8,192
  = 1,328,029,696. Layer 0 is ALSO full attention (hybrid_layer_pattern[0] = 0): 9 full layers, 39 SWA.
  Script: scratchpad hdr.py (python3 -I hdr.py <dir>). Store as test fixture JSON.
- KEY FINDING: under goose's ONE fit rule (budget = available - RAM x 0.093, min GPU ceiling) the measured figures
  give budgets 67.11e9 (workhorse, 96 GiB) / 98.22e9 (mule, 128 GiB) = 165.3e9 total < 168.6e9 total weights
  (165.9e9 layers + 2 x 1.328e9 shared). NO split fits under goose's rule; the manual 19/29 run lived inside the
  margin (8.4 / 10.7 GB from raw available). Max-min spare picks: raw budgets 76.7/111 + overhead 1.10 -> 19/29
  (min spare ~0.66 GB before KV/workspace); goose budgets no overhead -> 19/29 (both negative, refused);
  goose budgets + 1.10 -> 20/28 (refused). Plan: test A = measured figures through NodeFigures::new -> refusal
  naming both nodes with numbers; test B = budgets 76.7e9/111e9 set directly -> 19 rank0 / 29 rank1. Do NOT
  loosen AVAILABLE_MARGIN_RATIO (measured kernel-WARN rule shared by every runner). Report to Mihai.
- Planner shape: per stage need = with_overhead(weights + state + prompt_cache) + workspace;
  state = roundup256(C) x full_layers_in_stage x 2560 + swa_layers_in_stage x 128 x 8 x 320 x 2 (655,360 B);
  prompt_cache = PROMPT_CACHE_CONTEXTS x state; workspace = chunk x C x (64 heads x act 2 B + 1 mask) when the
  stage has a full layer. Select split at min chunk 256 (KV_CACHE_STEP) maximising min(budget - need), rank0 =
  last layers; derived context = largest C that split fits; chunk = min over ranks of headroom/(C x pair),
  cap TENSOR_PREFILL_STEP, floor to 256. Emit RankPlan per rank (layer_start/end, prefill Some(TensorPrefill)
  with pair_bytes = 129, kv_bytes_per_token = MAX over ranks, sequence_state = MAX), report.pipeline_starts.
- mlx_lm 0.31.3 sharded_load calls `model.model.pipeline(group)` TWICE (lazy file selection, then after
  load_model): the prelude's patched pipeline must be deterministic per fresh model (it is).
- PIPELINE_FORK_COMMIT question RESOLVED: a18e14fd4 = lz-pipeline-qwen4.12 (2026-09-27), an ANCESTOR of the pin
  8c0007054 (lz-pipeline-qwen4.14). The live env ~/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12 is simply
  stale (filed as Q-234 on 09-28, fixed a60f985f9: an env at another pin fails the update by name). Keep 8c000705.
- Fork mimo sha: NOT landed. Rapid-MLX branch lz/pipeline-mimo-v2 is still AT 8c0007054 (worktree in the
  coordinator's scratchpad wt-pipe), lz/mimo-v2-vendor at 03fa3b38f. ~/jaccl-smoke/HANDOFF.md has no
  "Rapid-MLX mimo_v2 vendoring" heading yet. Plan: keep the ONE constant PIPELINE_FORK_COMMIT, add the doc line
  "bumped when the fork's mimo_v2 commit lands (lz/pipeline-mimo-v2, on top of this commit)"; preflight for
  MlxLmPipeline probes `import rapid_mlx.mlx_lm_models, mlx_lm.models.mimo_v2` in pipeline_python and FAILs by name.
- Launch: remote_script is launch.rs:734 (add `ulimit -n 65536 2>/dev/null || ulimit -n 10240;`); RankProgram
  MlxLmServer tag currently "mlxLmServerNamedConversation" (launch.rs:194) -> new rename e.g.
  "mlxLmServerPipelineSplit", old one becomes an alias; interpreter() at :682; rank_specs at :491 (add split);
  TENSOR_PROGRAM at :54 (add rank_pipeline_split.py before rank_wrapper.py); rank_wrapper.py sys.argv at :2371
  (append "--pipeline" when spec["pipeline_split"]).

## Traps
- The ctx-guard hook warns at 160k. This agent's system context was large and it reached 161k after reading only. Read
  with narrow sed ranges. launch.rs is 10k+ lines and supervisor.rs 5k, never read them whole.
- Never touch the engine on :8090 or other sessions' jobs; never pkill by pattern.
- The PIPELINE_FORK_COMMIT bump waits on the parallel vendoring agent's sha (it lands on top of 8c000705).
- Session 2 hit the ctx-guard at 162k BEFORE compiling: the system context alone is ~60k. Next agent: no planning
  reads, go straight to `cargo check` and the compiler list.

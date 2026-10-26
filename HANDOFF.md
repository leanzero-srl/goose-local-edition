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

## Traps
- The ctx-guard hook warns at 160k. This agent's system context was large and it reached 161k after reading only. Read
  with narrow sed ranges. launch.rs is 10k+ lines and supervisor.rs 5k, never read them whole.
- Never touch the engine on :8090 or other sessions' jobs; never pkill by pattern.
- The PIPELINE_FORK_COMMIT bump waits on the parallel vendoring agent's sha (it lands on top of 8c000705).

# MiMo-V2.6-Flash first-class in goose: coordinator handoff (2026-10-26)

The coordinator stopped at its context limit. Agents still running in the background when it stopped:
- Rapid-MLX release agent: fork suite base-vs-new on both lines, tags `v0.14.3-lz.15` (single line, branch
  lz/mimo-v2-vendor, local b94574821) and `lz-pipeline-qwen4.15` (pipeline line, branch lz/pipeline-mimo-v2,
  local 07126f051 on 8c0007054), push, fresh-venv proof. Its state: ~/jaccl-smoke/HANDOFF-rapidmlx.md; results
  appended to ~/jaccl-smoke/HANDOFF.md section "Rapid-MLX mimo_v2 vendoring".
- goose compile+split agent (this worktree): scope A-D in HANDOFF.md (compile RUNNERS table, split planner,
  plan in the preflight/status DTO, gate). Its state: HANDOFF.md here.

## Decisions taken
- Vendor, do not bump: Rapid-MLX caps mlx-lm <0.32 (#1248). Contract: `import rapid_mlx.mlx_lm_models` makes
  `mlx_lm.models.mimo_v2` resolve. Vendored model adds `MiMoV2Model.pipeline(group, split=None)` (from 0.32.0);
  goose must set the split on THAT method, not on the shared PipelineMixin (pipe_split.py's patch no longer reaches it).
- mimo runner = goose's rank wrapper in pipeline mode, run from the PIPELINE env; bump PIPELINE_FORK_COMMIT to
  the lz-pipeline-qwen4.15 sha once pushed.
- Memory: goose's shared plan reserve (9.3% RAM) refuses MiMo (67.11 + 98.22 = 165.3 GB budget < 168.6 GB).
  Default untouched; new `plan_reserve_ratio` config override, floored at WATCHDOG_WARN_RESERVE_RATIO; the refusal
  names the lowest ratio that fits.

## Remaining, in order (fresh agents, tight briefs; each starts ~60k context so give ONE concern each)
1. Launch path: prelude imports rapid_mlx.mlx_lm_models, sets each rank's layer range, ulimit -n 65536 over ssh.
2. Desktop Run it / split view: per-node layers, GB, spare, reason (vitest; owner UI rules).
3. Pin bump + full gate (fmt, clippy -D warnings, cargo test -p goose-sidecar, development_gates, pnpm lint:check + test:run).
4. Live test: dev build on workhorse AND workmule, start MiMo split from the app over CDP (goose-feature-dev recipe,
   isolated profile, never :8090), both ranks up, 3 prompts (coding, reasoning, Jira) through the swarm provider,
   load time + tok/s vs manual (277 s cold JACCL, ~40 tok/s), stop from app, no ranks left on either Mac,
   qwen3_5 distributed tests green. heavy-run --status before the 170 GB load.
5. Record facts: goose-mlx-inference references/distributed-engine-history.md + mlx-jaccl-cluster changelog.

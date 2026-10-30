# HANDOFF: engine memory tunables, goose side (branch lz/memory-tunables)

Worktree: ~/Projects/goose-memory-tunables (off origin/main 6e34d1e378). Stopped at the 350k context
limit. Contract: /Volumes/AI-workhorse/runs/2026-10-30-flash-fixes-verify/metal-limit/INTERFACE.md.
Fork branch lz/metal-tunables is pushed (coordinator says tip 3ed994de9); flag names verified against
fork cli.py `_MEMORY_TUNABLE_FLAGS` (74a7aa5): all match.

## Commits (not yet merged, not pushed)
- d2e9f6bed3 goose-sidecar: tunables reach the argv (memory_tunables.rs, engine_profile.rs supports_memory_tunables)
- 5a553eac88 acp: DTOs, memoryMachine / wiredLimitApply / wiredLimitRemoveDaemon, status memoryBudget (wired_limit.rs)
- 51bab82038 desktop: Memory tab (MlxMemorySection.tsx, memoryTunables.ts), i18n 16 catalogs, 5% floor correction
- 2e0451c552 sdk regenerate

## Design decisions (put in the final report)
- Config: `mlx_engine.memory.<snake_name>` (EngineSettings.memory: MemoryTunables); DTO `memory` camelCase.
- lz.20-only flags are sent only when the launcher tag is >= v0.14.3-lz.20 or the branch `lz/metal-tunables`
  (engine_profile::supports_memory_tunables). A set lz.20-only tunable on an older pin FAILS the mount by name
  (no silent ignore). --cache-memory-mb, --prefill-step-size, --max-concurrent-requests go to every engine.
- Default floor = round(0.05 x physical GB) per coordinator's measurement (was 8%); Workstation preset = 8%,
  Inference box = cap (physical GiB - 16) GB, floor 4 GB; suggested wired limit = physical MB x 122000/131072.
- --max-model-len = memory.max_context_tokens, else the mounted model's profile context_limit. NOT
  GOOSE_CONTEXT_LIMIT (client-side, per session; the engine serves many clients/peers).
- --max-concurrent-requests: user value also replaces the hybrid-cache count (x3) and sizes the swarm
  router's sidecar node (swarm_router local_engine_max_concurrent, nodes/project.rs) and status DTO.
- Prefix budget set -> --cache-memory-mb replaces --cache-memory-percent on unguarded engines.
- Not exposed: engine `--memory-floor-wait-s` (seconds, not in the INTERFACE table).
- Wired limit: Rust validates 0 or 8192..physical MB (u64), builds `/usr/sbin/sysctl iogpu.wired_limit_mb=V`,
  refuses quotes/backslash/newline before osascript `do shell script ... with administrator privileges`.
  Daemon: plist staged in temp_dir, root cp/chown/chmod, sysctl now, launchctl bootout/enable/bootstrap.
  Local only (no nodeId). Metal working set is per-process (goosed reads it at start): UI says so.

## Gate status
- cargo clippy -p goose-sidecar -p goose-sdk-types -p goose --all-targets -D warnings: green (before the 5% commit;
  re-run). cargo test goose-sidecar + goose-sdk-types: green. cargo test -p goose -p goose-swarm --no-fail-fast:
  63 ok, 1 FAILED = providers::key_connection::tests::only_a_refused_loopback_connect_reads_as_a_server_that_is_not_started
  ("Connection reset by peer"), fails deterministically; not touched by this diff. A probe on the clean main
  checkout ~/Projects/goose was started to prove it pre-exists: rerun
  `cd ~/Projects/goose && cargo test -p goose --lib only_a_refused_loopback` to confirm.
- UI: `pnpm run lint:check` exit 0 (tsc + eslint + i18n). vitest: memoryTunables.test.ts 10/10,
  MlxMemorySection.test.tsx 9/9 BEFORE the 5% edit (test strings updated to 7 GB / 5%); FULL `pnpm run test:run`
  NOT run yet (navigationUtils MLX_TABS gained 'memory': check tests that enumerate MLX tabs).

## Open items, in order
1. Re-run gates: `source bin/activate-hermit; export CARGO_TARGET_DIR=~/Projects/goose/target;`
   `cargo clippy --config 'profile.dev.package."*".strip=false' -p goose-sidecar -p goose-sdk-types -p goose --all-targets -- -D warnings`
   `cargo test --config '...' -p goose-sidecar -p goose-sdk-types`; `cargo run -p goose --bin generate-acp-schema`
   (the 5% commit added DTO fields: regenerate acp-schema.json, then `cd ui/desktop && pnpm run build-goose-sdk`, commit).
   `cd ui/desktop && pnpm run lint:check && pnpm run test:run`.
2. Live CDP check (not done): binary already copied to ui/desktop/src/bin/goose (debug, pre-5% build: rebuild
   `cargo build -p goose-cli --bin goose`, cp + `codesign -s -`). `pnpm run package`; quit installed app
   (`osascript -e 'tell application "Goose Swarm" to quit'`, installed pid was 1513); launch
   `GOOSE_PATH_ROOT=$SCRATCH/gooseroot GOOSE_USER_DATA_DIR=$SCRATCH/userdata GOOSE_DISABLE_KEYRING=1 nohup "out/Goose Swarm-darwin-arm64/Goose Swarm.app/Contents/MacOS/Goose Swarm" --remote-debugging-port=9333 &`
   with `$RUN_DIR/config/config.yaml` seeded: `mlx_engine: {port: 8095, models_dir: /Volumes/AI-workhorse/models/goose,
   spawn_command: [uvx, --from, "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@lz/metal-tunables", rapid-mlx]}`;
   small model mlx-community/Qwen3.5-9B-MLX-4bit (5.7 GB). Navigate `#/leanzero-swarm?tab=mlx&mlx=memory`
   (cdp.mjs from ~/.claude/skills/goose-feature-dev/references). Screenshots ->
   /Volumes/AI-workhorse/media/goose-memory-tunables/ after `~/Projects/machine-sync/bin/ai-disk-guard --need-gb 1 --out /Volumes/AI-workhorse/media/goose-memory-tunables`.
   Do NOT click the wired-limit buttons unattended (they raise a password prompt). Then quit own pid,
   `open -a "Goose Swarm"`, `~/Projects/machine-sync/bin/goose-single-guard --check`.
3. Independent reviewer (fresh general-purpose agent) on `git diff origin/main...HEAD` + owner spec: hard-coded
   numbers / controls without effect. Verify findings, fix real ones.
4. Rebase on origin/main (locale JSONs: replay key-by-key), ff-merge to main, push. Do NOT touch ENGINE_LAUNCHER.
5. Reply to the coordinator (agent a6a0e6be46e66adb0): 5% floor applied, others label applied, new budget fields added.
6. Cleanup: dev app, ui/desktop/src/bin copies (gitignored), scratch userdata; `git worktree remove` after merge.

## Traps hit
- sanitizeSettingsForWrite dropped unknown fields: it now carries `memory` (else a Sampling save wiped tunables).
- flag_value helper already existed in engine.rs tests (duplicate name compile error).
- i18n: other locales must carry every en key (i18n-validate-locale); merge script kept en order (moved one
  pre-existing out-of-order key mlxSetup.step2Reading, harmless).

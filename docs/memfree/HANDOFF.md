# HANDOFF — goose "Free up memory" (2026-10-25, written by the research agent on Gabee)

Read first: `~/Projects/goose-memfree-RESEARCH.md` (facts, verdicts, measured numbers, DESIGN with confidence).
Skill: `goose-feature-dev` (layer map, gate, CDP live check). Owner brief = the task text: Phase 2 BUILD.

## State
- Phase 1 research: DONE (file above, also committed as docs/memfree/RESEARCH.md on the branch).
- Phase 2 build: NOT STARTED. The research agent hit the 160k context guard after research.
- Worktree: `~/Projects/goose-memfree`, branch `feat/memfree`, created `--no-track` from origin/main d0d65b401
  (3.0.128). Another agent is releasing 3.0.129 from `~/Projects/goose-fix129*` — never touch those; rebase
  `feat/memfree` onto origin/main after 3.0.129 lands (`git fetch && git rebase origin/main`).
- Machine: Gabee (Mac.lan, M3 Max 64 GB, macOS 27). Gabriela is USING it: never quit/kill her apps.

## Build plan (in order; each step = commit, `<area>: <claim>`, repo identity, NO AI credit lines)
1. `crates/goose-sidecar/src/memfree.rs`: census (proc_listpids + proc_pid_rusage V4 ri_phys_footprint +
   proc_pidpath + uid), pure `categorize(rows, ctx) -> Vec<Group>` and `never_kill(pid,…)`; table tests
   (helpers fold into outer .app; WindowServer/loginwindow/Finder/Dock/other-uid = system; Goose app + its goose
   serve + the caller's ancestors = protected; MLX engines by `distributed::probe::classify_foreign_engines`).
2. goose-owned reclaim in sidecar/goose: unmount own idle engine (only when no open reply — holders.rs), reuse
   `machine::stop_other_engine`, SIGTERM orphaned goose serve (port selectOrphanedGoosed's rule), LM Studio
   (`lms unload --all` if on PATH / its REST — VERIFY the endpoint in lmstudio.ai docs first), Ollama
   (`GET :11434/api/ps`, `POST /api/generate {model, keep_alive:0}`). Each returns freed bytes measured
   before/after by footprint, not guessed.
3. App quit: `osascript -e 'tell application id "<bid>" to quit'`, never SIGKILL, refuse system/protected.
4. ACP (path E): sdk-types `MlxEngineMemoryHoldersRequest/Response`, `MlxEngineMemoryFreeRequest/Response`
   (custom_requests.rs near the mlxEngine types ~2734), handlers in `crates/goose/src/acp/server/mlx_engine.rs`
   (`core_memory_holders/free`), routes in custom_dispatch.rs (pattern: `dispatch_mlx_engine_stop_other_engine`
   ~1016), remote: add `MlxOp::MemoryHolders/MemoryFree` in `crates/leanzero-link/src/state.rs` (path() +
   from_path()) and arms in `GoosedMlxControl::dispatch` (~1432). Remote app quit: new config switch
   `GOOSE_LINK_ALLOW_REMOTE_APP_QUIT` default false, checked on the PEER; add `quit_apps` to `NodeAllows`
   (wire.rs:111, serde default false). Then `cargo run -p goose --bin generate-acp-schema` and
   `cd ui/desktop && pnpm run build-goose-sdk`.
5. Desktop: `ui/desktop/src/acp/` wrapper; a `FreeMemoryDialog` (lz Dialog/Button primitives, solid category chips,
   bold GB, NO left rails / tints / native select / window.confirm); entry points: NodeCard.tsx `primary` for
   state `cantRun` with fit short (nodeGlance.ts ~325 gives `candidate.fit`), and MacLoadPanel.tsx per Mac;
   after a free, re-read `mlxEngine/status` mountFit and show before → after. When the gap > everything
   reclaimable say so and name the smaller quant. Tunables: `GOOSE_MEMFREE_ENABLED` (true),
   `GOOSE_MEMFREE_SHOW_APPS_MIN_MB` (200) in Settings › App (pattern `settings/app/MlxRestoreSettings.tsx`).
   i18n into all 15 catalogs, FILE order (not sorted).
6. Tests: Rust unit (grouping, categories, never-kill), an ACP test (crates/goose/tests, one fn per Config test
   file), vitest for the dialog + the card entry.
7. LIVE on Gabee: load a small model YOURSELF (goose engine, e.g. a ~4–8 GB MLX model already in the models dir),
   measure budget, run "free goose's own" from the packaged app (isolated profile per the skill recipe, own CDP
   port, mlx_engine port != 8090, tell the owner before a window opens on Gabee's screen), assert budget rose by
   the measured footprint (± drift 2% RAM). Do NOT quit any of Gabriela's apps; app-quit live test only on an app
   you launch yourself (e.g. TextEdit with a new empty doc, or Chess).

## Commands
```
cd ~/Projects/goose-memfree && source bin/activate-hermit
# macOS 27 sqlx trap: every cargo build/test here needs
cargo --config 'profile.dev.package."*".strip=false' build -p goose-cli
cargo clippy --config 'profile.dev.package."*".strip=false' --all-targets -- -D warnings
# heavy builds go through: ~/Projects/machine-sync/bin/heavy-run (check its --help)
cd ui/desktop && pnpm install --frozen-lockfile && pnpm run build-goose-sdk && pnpm run lint:check && pnpm run test:run
```

## Traps
- Free memory on the card is the fit BUDGET, not vm free; purge/memory_pressure don't move it (research §2).
- `memory_pressure -Q` percentage is meaningless for this (85% "free" with 19 GB available).
- sysinfo `process.memory()` is RSS, not footprint; use proc_pid_rusage for the census numbers.
- Load average ~19 on Gabee from the 3.0.129 gate: measure before/after within seconds, drift is real.

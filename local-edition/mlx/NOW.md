# NOW — MLX in-house engine campaign (branch goose/mlx-inferencing)

## CURRENT CHAPTER (2026-08-31 evening): the Goose Swarm restructure
Mihai's five-phase UI reorg: (1) "merge main for Benchmark" — MEASURED no-op, we contain all of
main (0 behind / 868 ahead); Benchmark was hidden because the renderer's edition is a stored
setting defaulting 'standard' (never provider-derived like edition.rs) — same root cause as the
.local-edition CSS scope being absent. Pass A (in flight, panel-surgeon): edition derives from
provider (fragments incl. mlx), rebrand "Goose Local Edition"→"Goose Swarm", nav declutter to
[New Chat, Skills, Memories, Benchmark, Leanzero MLX, Settings], drop Settings>Models. (2) Recon
in flight (Explore): session working-dir metadata + swarm-settings/provider-credential plumbing.
(3) Pending passes: B = ChatGPT-style Projects tree (projects as local paths, sessions grouped
under them, new-session inherits the project's cwd); C = "LeanZero Swarm" three-tab view (LeanZero
MLX | Cloud Providers | Swarm Settings with node creation + per-node provider [mlx-or-cloud] +
weight), plus components/mlx → leanzero-swarm namespace rename. (4) After Pass A: build the FULL
DMG via `just release-fork 1.41.103` (Mihai: "build the full DMG to check"), again at the end.

**Read this FIRST after any compaction, before anything else in this dir.** Budget ~2k tokens.
Rule: when the thread changes, this file changes IN THE SAME COMMIT.

## The mission (Mihai, 2026-08-30)
Add an in-house, open-source MLX inference engine that goose itself supervises — **NEXT TO LM Studio,
never replacing it**. It supersedes only the built-in `goose-local-inference` FFI path. Primary
workload: **the swarm** — N concurrent long-context tool-calling agents. Plus: a starter desktop UI
window (engine control: mount/unmount, sampling incl. presence penalty, context length; HF browser:
MLX-only search + download into a configurable `models_dir`). Full plan:
`~/.claude/plans/i-want-you-to-enumerated-hejlsberg.md`.

## Hard constraints (Mihai's words, do not relax)
- Tests use **qwen3.5-9b 4-bit ONLY, freshly downloaded**. Mounting the fleet's 27B is FORBIDDEN for now.
  Amendment (Mihai, later that night): if the 9B itself proves inept at tool calls, downloading a
  similarly-sized tool-call-strong replacement is sanctioned. Status: NOT needed — the 9B scored 100%
  tool fidelity over 78 requests on rapid-mlx 0.13.1; any later fidelity gap on the same model is an
  ENGINE differentiator, not a model defect.
- The running LM Studio fleet must not be disturbed: memory gate before ANY mount (`gates.py`),
  `lms ps` snapshot/verify around every session, sidecar never on ports 1234/11434.
- One configurable `models_dir` (default `~/.goose/models`) for downloads AND mounts — not tied to LM Studio.

## State at last write (2026-08-30)
- Branch created off local-edition @ 5d238baa5 (pulled 779 commits first; agentic structure now in-repo).
- Finalists: **Rapid-MLX** (raullenchai/Rapid-MLX — tool parsers, decode speed, goose precedent) vs
  **oMLX** (jundot/omlx — continuous batching, prefix-sharing paged KV + SSD tier, memory guard).
  MTPLX + mlx-serve dropped → see TRUMP-CARDS.md. Verdict comes from the swarm-shaped bake-off, NOT web claims.
- **Inherited hazard (docs/EXPERIMENTS.md spike, 2026-06-25): on Gated-DeltaNet hybrids (all our qwens),
  a prefix-cache HIT can silently break tool-calling (omlx #825, mlx-lm #980). LM Studio was chosen over
  raw mlx-lm/omlx for exactly this. The bake-off scores this as a first-class dimension.**

## Restart, in order
1. `cat local-edition/mlx/NOW.md && cat local-edition/mlx/LEDGER.md | head -60`
2. `git log --oneline -10` on branch goose/mlx-inferencing
3. `python3 local-edition/mlx/gates.py probe` (memory state) and `~/.lmstudio/bin/lms ps` (fleet state)
4. Continue from "The one decision waiting" below.

## Operating mode — quality bar (Mihai, 2026-08-30, verbatim intent)
"Don't leave TODOs and fake implementations, don't do shitty fallbacks, never do horrible
deterministic stuff that no one cares about — find solutions to problems, that is your job, and
implement them correctly. That means putting in the effort." Applies to every artifact on this
branch: sidecar, UI, fork patches, bench harness. A missing input is a LOUD named absence, never a
quiet substitute; a mechanism measures and nudges, never gates for gating's sake.

## Operating mode
**UNATTENDED (Mihai, 2026-08-30 ~23:00, going to sleep): full autonomy for the night.** Decide and
act on best knowledge; no questions; maintain house rules + agentic discipline; commit every step;
keep this file and the ledger current so restrictions and thread survive compaction. The approved
plan is the authorization for all of it — bake-off, fork, integration, UI.

## The one decision waiting — MORNING STATE (2026-08-31 ~03:00)
EVERYTHING GREEN except two named holds. Done tonight: bake-off (8 runs, verdict Rapid-MLX),
fork live (leanzero-srl/Rapid-MLX @v0.13.1, uvx pin proven), goose-sidecar crate (supervisor +
gates), 11 ACP methods, desktop MLX Engine window verified in the RUNNING app (mount/sampling/
HF-download/delete, two spawn defects + gate-banner bug found live and fixed), SwarmEngine
boundary steps A/B/C in swarm.rs (surgeon; per-engine partition; TRACE YES), and the SWARM E2E
through the sidecar — real artifacts, 4/4 tests (LEDGER ~03:00; final banner lost to an external
kill-all of background tasks — rerun is one command).
HOLDS, both with reasons in LEDGER: (1) local-inference default-feature flip — the flag still
gates live desktop surfaces (onboarding/dictation/ModelSettingsPanel); needs Mihai's call on those
surfaces' fate. (2) Mixed-pool (LM planner + sidecar worker) run — blocked on the fleet's
LM_API_TOKEN, which lives on the MacBook.
MORNING one-liners: clean-banner swarm rerun (command in LEDGER); `just run-ui` to reopen the
desktop (app was killed with the task sweep); provide LM token for the mixed run.
Branch pushed to origin through the latest commit.

## Superseded waiting-item (kept for the record)
Bake-off verdict, pending oMLX numbers. State (2026-08-30 ~23:00): rapid-mlx 0.13.1 DONE — 3
concordant runs in experiments.jsonl (fidelity 1.0 at N=1/4/8, TTFT ~1.7/3.8/7.5s, aggregate up to
~50 tps, no hybrid footgun, RSS max 4.4 GB, serves ~/.goose/models path directly, hermes parser
auto-selected). rapid stopped cleanly (SIGTERM per-pid), fleet gate ALLOW throughout. oMLX 0.6.4
serving on :8091 from --model-dir ~/.goose/models (id Qwen3.5-9B-MLX-4bit), bench running 2x.
Next after verdict: LEDGER entry + fork the winner (leanzero GitHub, upstream remote) + build/test
crates/goose-sidecar (authored+committed, build held during scored runs) + swarm.rs EngineAdapter
via the swarm-surgeon agent + desktop window. Bench instrument fixed twice (req_tps replaces
decode_tps; RSS sampler fails loud) — first experiments.jsonl row's decode_tps is void.

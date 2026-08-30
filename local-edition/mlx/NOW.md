# NOW — MLX in-house engine campaign (branch goose/mlx-inferencing)

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

## The one decision waiting
Bake-off verdict, pending oMLX numbers. State (2026-08-30 ~23:00): rapid-mlx 0.13.1 DONE — 3
concordant runs in experiments.jsonl (fidelity 1.0 at N=1/4/8, TTFT ~1.7/3.8/7.5s, aggregate up to
~50 tps, no hybrid footgun, RSS max 4.4 GB, serves ~/.goose/models path directly, hermes parser
auto-selected). rapid stopped cleanly (SIGTERM per-pid), fleet gate ALLOW throughout. oMLX 0.6.4
serving on :8091 from --model-dir ~/.goose/models (id Qwen3.5-9B-MLX-4bit), bench running 2x.
Next after verdict: LEDGER entry + fork the winner (leanzero GitHub, upstream remote) + build/test
crates/goose-sidecar (authored+committed, build held during scored runs) + swarm.rs EngineAdapter
via the swarm-surgeon agent + desktop window. Bench instrument fixed twice (req_tps replaces
decode_tps; RSS sampler fails loud) — first experiments.jsonl row's decode_tps is void.

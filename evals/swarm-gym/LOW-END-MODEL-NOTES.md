# Low-end-model assessment — note-app A/B (best-of-3 vs best-of-1), 2026-06-26

Test subjects (kept): `apps/note-bestof3` (47 tests pass), `apps/note-bestof1` (206 tests pass).
Pool during the run: `qwable-v1-mlx`, `qwopus3.6-27b-coder-mlx`, `qwopus3.6-27b-v2-mtp` (the two qwopus being swapped for stronger models; qwable stays).

## Verdict
- **Both apps are REAL and WORK** — 47 / 206 tests pass on my own re-run; CLIs run. **Near-zero hallucination of results**: the weak models do not fake green. N=3's `integrate-verify` honestly found + fixed 4 real bugs and its PASS was true.
- **Status was ANTI-correlated with quality**: N=1 (single draft) is the *better* artifact (clean `src/` layout, 206 tests, zero pollution) but **reported FAILED**; N=3 (best-of-3) is worse (flat layout, 47 tests, dead-wired `--store-path`, 3 stray files) but reported DONE.
- **best-of-N is not the lever that matters for weak models — the dynamic replanner is** (it 4×-ed N=1's coverage). But a hung *bonus* (replanner) task failed the whole N=1 run — a scheduler-policy bug, not a model bug.

## Weak-model weaknesses (measured, with evidence)
1. No text-read tool in `[write,edit,shell,tree,read_image]` → ~30 failed calls: qwable invented a `read` tool 10×; both 27Bs called `read_image` on `.py`/`.toml` 11×; all fell back to `shell cat`.
2. `python` vs `python3` — 16 guaranteed-fail round-trips (every model).
3. qwable relative-path bug — 15 incidents / 6 sessions: drops the leading `/`, write nests a duplicate tree, then flails with ls/find/mv/rm. **qwable-specific** (27Bs: 0).
4. `py_compile` reported as "tests pass" (never ran pytest).
5. No re-read-before-theorize reflex: a failing test sent a worker into a `.pyc`/bytecode spiral for ~10 turns (the 15-min hang class).
6. Many tiny `edit`s on a slow device burned an 850s budget (unicode-escape thrash).
7. Pollution: `:memory:` (sqlite idiom on a JSON store), leaked `plan.json` (stale, also fed back into worker context), scout-written `lens_brief_*.md`.
8. tests-pass ≠ correct: `--store-path` dead-wired (group callback never sets `ctx.obj`); 47 green over a broken public flag.

## Guiding principles
- **P1** A green suite is NOT correctness — exercise the real artifact (run the CLI with real args).
- **P2** Bonus/opportunistic (replanner) tasks must be NON-FATAL; judge run success on the core plan.
- **P3** Match task→device; never pin the longest task to the slowest worker; diversify device on retry.
- **P4** Hand weak models the exact tools + env facts; don't make them discover the toolset by trial-and-error.
- **P5** Force "re-read the file before theorizing" on an unexpected failure.
- **P6** On slow devices prefer one full-file `write` over many `edit`s.
- **P7** Quarantine scratch output — the workspace is not a scratchpad (scouts/scaffold/smoke must not litter).
- **P8** Workers use ABSOLUTE paths; validate at the tool boundary.

## Ranked tweaks
HIGH (structural — survive the model swap, do first):
- Add a real text `read`/`cat` tool OR rename `read_image`→`view_image` + prompt: "to read text use `shell cat`; no `read` tool." (P4)
- Worker prompt: "Use `python3`, not `python`." (P4)
- Scheduler: tag replanner/bonus tasks NON-FATAL; run success = core plan only. Flips N=1 false-FAILED→DONE. (P2)
- Worker prompt: absolute paths only; `write`/`edit` echo/validate the resolved path. (P8)
- integrate-verify gate: run the real CLI with args (catches `--store-path`); anti-pollution check (no unexpected files); test-authors must actually run pytest. (P1,P7)

MEDIUM-HIGH:
- Scouts MUST NOT write workspace files (return briefs in-message); don't leave `plan.json` on disk. (P7)
- Retry: exclude/de-prioritize the device that just timed out; diversify on retry. (P3)
- Throughput-aware timeout: scale by device tokens/s, or cap by turns+idle rather than wall-clock; route long/edit-heavy tasks off slow devices. (P3)
- Worker prompt: "on an unexpected test failure, re-read the file under test before theorizing." (P5)
- Worker prompt: prefer one batch `write` over many `edit`s on slow devices. (P6)

SAMPLING (A/B, lower confidence):
- qwable: temperature ~0.2–0.3, top_p ~0.9, min_p ~0.05 — hypothesis for the dropped-slash path bug. qwable stays → worth the A/B.
- Slightly lower temperature for test-authoring/verify roles to curb idiom flourishes (`:memory:`). Lower confidence.

## Swap relevance
- ERASED by swapping the qwopus models: `qwopus-v2-mtp` stream-decode errors + ~4× slowness (the entire N=1 false-failure chain); possibly the `read_image`-on-text habit (re-measure on the new models).
- PERSISTS (structural): everything else — the read-tool gap, `python3`, non-fatal bonus tasks, throughput-aware timeouts + retry rerouting, verify-gate gaps, scout pollution, re-read reflex, qwable path bug + its sampling tuning.
- Do the HIGH-confidence structural tweaks regardless of the swap.

## Overnight self-driving loop (2026-06-27, 3x identical qwopus-coder-mtp pool)
Fixes SHIPPED + validated this session (all on local-edition):
- **Over-read guard** (a6800de7): workers re-read the whole project before acting despite already having the manifest + dep specs. Rule: read at most the one file you edit, then act. VALIDATED — greenfield modules wrote at 24-37 msgs (vs the amendment's wire-cli-all 2622s paralysis).
- **No-cd** (41ec6abd): cli-module burned ~half its 28 turns on redundant `cd` into the cwd it was already in (~32-39min runtime). Rule: NEVER cd, run commands directly.
- **Idle-based timeout**: validated 3x — slow-but-progressing tasks (wire-cli-all 2622s, test-cli 2070s, test-timer 1398s) complete instead of being false-killed by wall-clock.
- **owned_files + manifest injection**: workers write to their EXACT assigned paths (fixed the root-vs-src layout divergence + silent file loss).
- **speed_weights** (6695f1ad + config): per-host volume skew; config substring must match the actual device id (bug: `mihai` didn't match `local-qwopus...@q5_k` -> use `local`).
Open items (NOT yet fixed — for next sessions):
- **Cross-task SEMANTIC divergence**: parallel tasks invent divergent UNSPECIFIED behavior (flashcards `ease_factor`; pomodoro `pomodoro_count`/pause-resume -> 14 failing tests). The read-source rule fixes APIs, not semantics. Mitigation: architect should PIN behavior in specs, or rely on integrate-verify to reconcile (it runs pytest + fixes to green — but it's also the slow over-reading tail).
- **Replanner dependency gap**: replanner injects bonus tasks (test-X, update-readme) WITHOUT depends_on the task that builds what they test/document -> they run too early, confused, produce nothing (test_cli_stats_export_import.py silently lost). Fix: replanner must add correct deps or only inject independent work.
- **Speed model murky**: WorksMacStudio (configured fastest, weight 3) posted the SLOWEST observed times (timer-module 517s, test-timer 1398s) because it drew the heavy tasks. The volume skew trusts the CONFIGURED rank; if wrong it misfires. Observed-time hard-routing self-corrects; consider making the volume skew observed-driven, or re-confirm the hardware ranking.
- **Concurrency-vs-volume tension**: with pool weight(concurrency)=1, a fast node on a heavy task is blocked by in_flight from taking MORE, so the volume skew never fires + the slow node still bottlenecks. Options: bump fast-node concurrency (uncertain on single MLX instance) or make the slow node fallback-only.
- **Missing `__main__.py`**: greenfield CLI packages built without `__main__.py`, so `python3 -m pkg` fails (must use `python3 -m pkg.cli`). Minor packaging nudge candidate.

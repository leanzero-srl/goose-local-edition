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

## Overnight loop, continued (2026-06-27 late — laps 2-3, contact-book/dice/note-amend-pin)
More fixes SHIPPED + validated (all local-edition), each from reading a real session trace:
- **inject-file-content** (ea705f7c): amendment workers got the manifest but not the CURRENT CONTENT of the file they edit → the cli-integration task cat-ed the file 18× to learn the API. Now the layout block injects each owned file's existing content (cap 12000). VALIDATED: the pin-amendment cli-integration did **0 cats** (vs 18 before) and ran faster.
- **planner-on-fastest-host** (1b46b37b) → refined to **quant-aware-planner** (71b665cd): first routed the architect to the highest-`speed_weight` host; but that host ran a **q5** quant which produced NO skeleton AND stalled the solo-planner fallback (generating=0). The planner does the HARDEST task, so QUALITY > speed: now ranks `(not-low-quant, fastest)` — avoids `@q5/q4/q3/q2`, falls back to fastest only if all low-quant. VALIDATED: planner moved to local non-q5, architect produced a 6-subtask skeleton.
- **ignore-run-artifacts** (cf8999e3) + **plan.json/prompt.txt extension** (ea8882da): workers fixated on the swarm's/harness's OWN artifacts in the cwd — a data-models task cat-ed `out.json` 34×, a cli-edit task cat-ed (a hallucinated) `plan.json` 20× + `prompt.txt`, both 0 writes. Worker prompt now ignores `out.json`/`*out.json`/`*progress*.log`/`.swarm/`/`plan.json`/`prompt.txt` (+ parent dir) and must not create `plan.json`. Also: redirect the run's own `out.json` OUTSIDE the run cwd in the harness. (Confirms notes item 7 — the leaked-plan.json class.)
- **integrate-verify fixes BASELINE regressions** (2035b4f3): an amendment that adds a field (`pinned`) changes `to_dict()` → an existing baseline test fails; integrate-verify ran pytest 6× but made 0 writes (paralyzed by "keep existing tests green" vs "don't change behavior"). Now told to EDIT the existing test that an intentional change legitimately broke. (Addresses the amendment-baseline-regression class.)
- **idle-based timeout**: now validated 5× — roller-module 1049s, multiple test-cli/test-roller ~800-880s complete instead of dying at a wall-clock cap.
- **__main__.py**: now emitted on greenfield CLI packages (pwgen, dice) → `python3 -m pkg` works. (The prior open item is resolved.)
Still open: cross-task semantic divergence; replanner dependency gap; speed model murky / heavy-task-on-slow-node (roller-module 1049s on gabee); readme/doc tasks still over-read modules to document them (candidate: inject deps into doc tasks too).

## Overnight loop, lap 4 (2026-06-27 — dice/contacts/mdtoc/expense-splitter/tags/archive)
Structural + quality fixes SHIPPED + validated (all local-edition), each from a real trace:
- **strengthen-over-read-guard** (2f09daea): a cli task burned 13 reads on 6 `test_*.py` files ("read the codebase to understand it") before any write. Guard now: NEVER read `test_*.py`, never browse to "understand the codebase", read ONE source file only for an exact API. VALIDATED — later cli tasks read cats≈1.
- **stop-when-green** (801bcec1): a cli task ran pytest 12× agonizing over an UNSPECIFIED detail (multi-tag AND vs OR) while already green, blocking integrate-verify. Now: the moment your file's tests pass, call final_output; don't re-run pytest >2× or re-litigate unspecified behavior — pick a sensible default and STOP.
- **architect-CLI-entry** (4ae349d1): markdown-toc built parser/formatter/anchor modules + tests (33 green) but NO `cli.py`/`__main__.py` — not a runnable CLI despite "Build a CLI". Architect now MUST plan a cli.py+__main__.py entry subtask for any CLI request. VALIDATED — expense-splitter got a `cli-entry-point` task and `python3 -m expense_splitter` RUNS.
- **mkdir-precreate** (45740213): a cli-commands worker ran `mkdir` 27× on a nested path (0 writes, paralysis). The swarm now `create_dir_all`s every owned file's parent BEFORE dispatch and the prompt says dirs EXIST, never mkdir. Deterministic > nudge. VALIDATED — re-run had mkdir=0 across all workers.
- **click-mix-stderr** (072cdc72): test tasks wrote `CliRunner(mix_stderr=False)` → 28 TypeErrors (removed in Click 8.2+); the worker spun pytest 10×. Prompt now: construct `CliRunner()` no-args, drop removed kwargs instead of fighting them. (Models carry stale library API knowledge — a broad class.)
- **hallucinated-completion-guard** (bc990fe0): a test-archive task called final_output ("done") after 0 writes; the test file never appeared yet the task was accepted. The dispatcher now verifies every owned file exists + non-empty before accepting success, else returns Transient (retry). Deterministic completion check.
- **plan.json/prompt.txt ignore** (ea8882da): extends ignore-run-artifacts — a worker looped 20× cat-ing a (hallucinated) `plan.json`; cascaded to wedge integrate-verify (gated on the paralyzed task) → whole contact-book run wedged. Now ignores plan.json/prompt.txt too; also redirect the run's own out.json OUTSIDE the cwd.
- **integrate-verify-regressions** (2035b4f3): when an amendment intentionally changes behavior (new field in to_dict), the resulting baseline test failure must be FIXED by editing that test, not stalled. VALIDATED — archive amendment's 2 to_dict regressions reconciled back to 206 green.
- **quant-aware-planner** (71b665cd): planner prefers a NOT-low-quant node (avoid @q5/q4/q3/q2) then fastest — a q5 architect produced no skeleton and stalled the solo fallback. VALIDATED — planner moved to local non-q5, skeletons produced.
- **idle-timeout**: now validated 7× (storage-archive-persist 683s, test-archive 900s, roller-module 1049s, cli-archive 1047s all completed instead of dying on wall-clock).
Kept apps this lap: pwgen, dice, contacts (8-cmd CLI), mdtoc (library), split (8-cmd runnable CLI), note-amend-{pin,tags,archive}.
Still open (lap 4): replanner over-analyze (6 pytest 0-writes deciding bonus tasks); test-task-over-read (test tasks cat source for the API — candidate: inject dep content into test tasks); parallelism/concurrency (1 slow critical-path task idles 2 nodes); spec-coverage divergence (architect builds `add --tags`+`list --tag` instead of literal `tag`/`untag` subcommands); slow 27B generation (10-17 min/task) — idle-timeout covers it but wall-clock per amendment is ~1h.

## Lap 5 validation (2026-06-27)
- **hallucination-completion-guard VALIDATED on pwcheck** — fired 2× (retried 2 zero-write tasks that had claimed done); final 79 tests green, CLI rates passwords correctly (123=Very Weak 8/100, Xk9#mLp2qR=Weak 45/100). mkdir=0, architect-CLI-entry (pwcheck/__main__) all held.
- Replanner ADDS VALUE here: it injected unicode edge-case tests that exposed 4 real scoring gaps, then integrate-verify reconciled them to green — the replanner converged (not a runaway). Kept apps/pwcheck.

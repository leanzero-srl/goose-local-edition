# THE REBUILD BATCH — what is queued, in what order, and what gates each item

**Written 2026-08-09 so the plan survives a context compaction.** The scarce resource is the
REBUILD (~4 fleet-days and a corpus reset), not the code. Everything below lands in **ONE** rebuild.

---

## 0. PRECONDITIONS — both already satisfied

- ✅ **Corpus frozen** at `runs/nodeloop-SNAPSHOT-pre-batch-2026-08-09` (F705): 118 result files,
  117 verdicts, 13 built `vendorsync` trees, 40 archived logs, `loop.log`, and `tiers.jsonl` in
  **both** states. Four change-list readouts are baselines measured against the current corpus; the
  rebuild resets it (`is_done` keys on `engine_build`), so without this freeze the rebuild destroys
  its own comparators.
- ✅ **Four zero-cost gates run.** C10 PASSED. C4 PASSES at full scope. C13 UNRESOLVED ⇒ **out of the
  batch**. C12 STRUCTURALLY UNAVAILABLE ⇒ ships env-gated, default OFF, ungated by evidence.

---

## 1. ALREADY WRITTEN AND VERIFIED — these ride the next rebuild

| # | What | Risk | Verified |
|---|---|---|---|
| **F706** | Stall lever's three contradictory comments corrected. **Comments only — the default is untouched.** | none (documentation) | `cargo check` clean; grep control confirms all three false strings gone |
| **F707** | The shadow/branch invariant **my own `a9f43543d` broke** — `conf_lifted` hoisted above the emission (emission NOT moved, or two diagnostics silently change meaning across 24 archived rounds); both `would_skip_ladder` and `would_skip_ladder_prelift` ship | low | `cargo check` clean; **call-site regression test passes** |
| **F711 · C1** | Timeout scan pointed at the app, not the swarm's own tests; `is_test_path` extracted so a fourth copy cannot be written | low | replay 2.75 → 1.62 findings, 2.4 SE |
| **F712** | End-to-end test for the above, **watched FAILING with the filter deleted** | none (test) | proven to discriminate |
| **F717 · C12(b)** | `description_chars` on `TaskDispatched` — the split child's instruction length | none (instrument) | compiles; `landcheck` will confirm it emits |
| **F717 · C5(A)** | `requested_best_of_n` / `distinct_draft_models` / `clamped` on `skeleton_drafts` — today's `requested` is POST-clamp | none (instrument) | same |
| **F717 · C6(1)** | `inconclusive_reasons` on `complete_verify` — why a run abstained; previously stderr-only | none (instrument) | **string probe CANNOT attribute this** (name pre-exists on `spec_contract`) — `landcheck` is the only check |
| **F717 · C5(C)** | Four sites claiming "1 node drafts 2 skeletons" corrected. The clamp is **distinct models**; one node drafts ONE, so it has no agreement score at all — a CAPABILITY difference that was being read as a behaviour one | none (comments) | assertions untouched; 5 `⚠️ CORRECTED` markers |
| **F718 · C7** | Scout no longer told it "cannot look anything up" when the spec names documents and it holds a shell. **77% of scouts fetched a URL under that instruction.** Pure `scout_lookup()` the call site matches on | low — **env-gated `GOOSE_SWARM_SCOUT_DOC_URLS`, default OFF** | 2 tests, all four combinations + both gate states, **watched FAILING against reverted logic** |

**Verification for all of the above is now executable, not remembered** (F719/F720): `greengate.sh`
(fmt + clippy `--all-targets` + full suite, refuses the build), `probe.py --verify` (6 literals
baselined ABSENT, a real absent→present flip), `landcheck.py` (the fields actually EMIT in a run
log — the half a string probe cannot answer), `boundary.py` (never rebuild under a live cell).

**Also corrected inside F707:** the comment claiming "one node drafts 2". All 11 one-node runs show
`requested=1`. The cap is **distinct draft models**, not node count.

## 2. HARNESS ONLY — need a sweep restart, NOT a rebuild

| # | What |
|---|---|
| **F695** | `shutil.rmtree(dst)` was **erasing a completed scored run** on every reused cell dir. Now archives to `_superseded/<cell>@<build>`. |
| **F698** | The void predicate keyed on `harness_ok` alone, which would have destroyed **3 real 112–124 min runs** (one a real lever run). Now keyed on the **st-2 "no run log" signature**. |
| **F699** | `tiers.jsonl` backfilled — 104 phantoms flipped, plus 5 the join could not reach. |
| **F702** | Scorer emits per-item `parts`. **Proven byte-identical on scores and detail ⇒ `SCORER_VERSION` deliberately NOT bumped** (a bump forces a full corpus re-score for a field that changes no number). |
| **F708** | `e2e_oracle_off` and `spec_sized_plan` given questions — they were **unschedulable** and they test today's two biggest defects. |
| **F709** | Orphan-arm check is now a **startup refusal** in `preflight`, not a per-pass warning. |

## 3. NOT YET WRITTEN — highest confidence first

**C1 · COMPLETE · point the timeout scan at the app, not the swarm's own tests.**
`swarm.rs:26202`. 55% of file-attributed round-0 findings name a `tests/` path the scorer never
grades. **Replay over 24 archived runs: findings 2.75 → 1.62, paired delta 1.12 ± 0.47 = 2.4 SE —
the only significant readout available on this fleet.** Fix the sibling basename/full-path bug at
`:26105` and `:3028` in the same commit and extract one shared `is_test_path(lang, f)` so a fourth
occurrence cannot be written. Readout: zero finding_texts naming a `tests/` path — **one
counterexample falsifies it.**

Then the rest of `CHANGE-LIST.md` in its stated confidence order. **Every uncertain change ships
behind an env gate, DEFAULT OFF**, so the whole experiment program runs by flipping variables and
nothing needs a second rebuild.

---

## 4. THE ORDER

1. Write C1 (+ any further STRONG items), env-gated where uncertain.
2. `touch STOP` → the sweep finishes its current cell and **exits cleanly between units** (F675).
   Never mid-cell: a past interruption landed 0.0563/0.0561/0.0561 as non-void and **overwrote the
   campaign's best result, 0.9033**.

   **Gate this on `python3 boundary.py`, never on a hand-written glob or `pgrep` (F715).** Rebuild
   only on `BOUNDARY-REACHED` (exit 0). Twice on 2026-08-09 I globbed the heartbeat under `.swarm/`
   — it is at the **cell root** — got zero, and *a blind zero is indistinguishable from a finished
   cell*; acting on it would have rebuilt under a live 21-minute run. Three facts the detector
   encodes so they are not re-derived wrong:
   - `heartbeat` / `run.jsonl` are at the **cell root**; `.swarm/` holds only `current-run.json`,
     whose mtime is a **start** time and so looks stale on a healthy run.
   - The live dir is named for the **entrant** (`swarm-3node-r1`, `sweep.py:1544`); the sweep logs
     the **cell** (`baseline-n3-r1`). They disagree BY DESIGN (`:1580`) — not the reused-dir bug.
   - A quiet `run.jsonl` beside a fresh heartbeat is a **long worker call**, not a hang. Only a
     heartbeat older than ~3 min is a real stall.
3. **`./greengate.sh` FIRST, and build only on exit 0.** Then `cargo build --release -p goose-cli`.

   This step used to say only "cargo build", and that omission cost twice in one day.
   **F714:** `cargo fmt --check` failed after four engine edits verified with `cargo check` alone —
   which says nothing about formatting, while AGENTS.md says never skip fmt. **F718:** an edit landed
   between an existing `#[test]` and its function; the orphaned attribute duplicated onto the new
   test and the original became dead code that no longer ran. **Only clippy saw it.**
   ⚠️ **And the test TOTAL cannot catch that class at all** — F718 measured 473 passing in *both* the
   broken and the fixed tree, because a disabled test and an added test cancel exactly. After adding
   tests, confirm the neighbours still run **by name**. The gate is proven to exit 1 on the F718
   defect and 0 once restored.
4. **Verify the binary with the string-literal probe** — `strings` cannot see private Rust fn names
   (three known-present fns read 0), but it *can* see emitted literals, which is what makes a zero
   there a proven negative rather than an observed one.
5. `./loop.sh start` — clears STOP itself, and now **refuses** if any arm cannot fire or is orphaned.
6. Confirm `ps -o ppid=` prints 1.

## 5. WHAT MUST NOT BE TOUCHED

- **`pre_review`** — 2.0/run at one node vs 10.2 at three. The **only** mechanism that scales with
  node count.
- **The retarget ladder as a GATE.** Make it cheap, make its threshold honest, log when it is inert —
  **do not cut the rung as a speed play.** It is the only mechanism that ever lifted a below-floor
  plan, and the alternative path is worthless here (every `low_confidence_ask` resolves to a timeout
  in 0.08 min — nobody answers).
- **The `owns_nothing` filter on `green_blocking_failed`.** A model self-report may never veto green.
- **`established()`'s strictness for genuine abstentions** — it closed 4/4 measured false greens.

⚠️ **The sink is NOT on this list any more.** The change list called its parallelism "the one clean
win"; the red-team refuted that — the e2e fan is `clamp(worker_count, 2, 4)` over SLOTS with zero
variance, so the command union is identical and extra shards merely re-pay build-and-launch.
`clamp(worker_count,2,4)` is now itself a candidate defect, and `spec_sized_plan` (F708) shows the
same `worker_count`-is-SLOTS confusion in the planner.

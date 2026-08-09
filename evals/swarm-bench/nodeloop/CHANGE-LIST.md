<!-- Produced by the phase-improvements-v2 workflow, 2026-08-09. 21 agents, 0 errors.
     Every proposal was stress-tested; a proposal could only be rejected on CONTRADICTED,
     ALREADY-EXISTS or TRADES-QUALITY-FOR-SPEED. Thin evidence was NOT a rejection ground —
     the first audit used that bar and killed 13 of 13 by construction (F691). -->

# ⚠️ ONE CONFLICT WITH THE RED-TEAM — RESOLVE BEFORE ACTING ON THE SINK

**Section 4 below says the sink's parallelism is "the one clean win" and must not be touched, citing
"64% more verification for 10% more wall" and "the join is FASTER (11.7 vs 15.5)".**

**A PARALLEL RED-TEAM REFUTED BOTH NUMBERS** (F689-CORRECTED). The e2e fan is
`clamp(worker_count, 2, 4)` over SLOTS — 2.00±0.00 shards at one node, 3.93±0.07 at three, zero
variance — so **the union of commands checked is IDENTICAL** and four shards re-pay build-and-launch
four times for the same coverage. The rest is slowdown, not work: same `verify::<M>` task count, each
task **+69% (p=0.001)**. And "integrate-verify is faster at three nodes" is **one outlier run**
(baseline-n1-r1 at 64.6 min); drop it and the sign flips, and it flips within both powered build strata.

The two workflows ran concurrently and did not see each other. **The red-team re-derived from raw data
and read the source; the change list did not.** So:

- **The sink's "do not cut shard count" instruction still stands** — but on the grounds that C4/C11/C13
  are about *information*, not capacity, NOT on the grounds that the fan is buying coverage. It is not.
- **`clamp(worker_count, 2, 4)` is itself a candidate defect** — more shards for the same command union
  is pure duplicated setup. That question is now OPEN, not settled by section 4.

---

# GOOSE SWARM ENGINE — CHANGE LIST FOR THE NEXT REBUILD
**Gate: 3 nodes must beat 1 on quality (0.733 → 0.866). Speed secondary, hard floor: 3 never slower than 1.**

---

## 0. THE HONEST HEADLINE, BEFORE THE LIST

**Not one of the fourteen changes below can be shown to move the build score on this fleet.** Pooled score sd is 0.1767 — a 0.05 effect needs ~196 runs/arm (~330 fleet-hours). The 0.733 → 0.866 gate is, as stated, unmeasurable here. Any future proposal whose only readout is the build score is untestable and should be sent back for a phase-level readout.

So what this batch actually buys, and it is the precondition for ever passing the gate:

1. **Three false-signal sources removed** — the run's verification claim is false 25/25 by construction (C6), 55% of its repair findings point at ungraded test files (C1), and it reports the fleet's own extra work as unplanned cruft (C2). All three are settled offline; none needs fleet time to justify.
2. **Two channels opened that carry the fleet's differentiator** — the sink's reports become readable (C4), and the module verifications reach the only task that can act on them (C11).
3. **The one phase quantity that clears 2 SE gets cheaper without loosening its gate** (C8, C3) — planning, +13.4 min at t=+3.63.

And the structural fact behind all of it: **on the vendorsync bench the vendor-fact channel is already saturated** (plan fact-coverage 8.79/9). If the campaign wants a score readout at all, it needs a bench spec whose document carries facts the current plans miss. That is a bench problem, not an engine problem, and no amount of engine tuning substitutes for it.

---

## 1. THE CHANGE LIST — ORDERED BY CONFIDENCE IN CORRECTNESS

### C1 · COMPLETE · CHANGE · point the timeout scan at the app, not at the swarm's own tests
**Edit.** `swarm.rs:26202` — replace `app_scope_py(&cwd, &smoke_all_files)` with a new `app_scope_py_source_only(&cwd, &smoke_all_files, complete_lang)` that drops a file when `lang.is_test_file(base_of(f))` **or any path segment equals `tests`/`test`**. Do it at this one call site, not inside `app_scope_py` — `cross_module_drift` shares that helper and reports 0 findings in 24/24 runs, so widening the edit buys an untestable scope change. In the same commit fix the sibling bug at `swarm.rs:26105` and `swarm.rs:3028`, which pass a **full path** to a function that takes a **basename** (6 other call sites already use `base_of`, three of them with comments explaining why) — extract one shared `is_test_path(lang, f)` so a fourth occurrence cannot be written. Add `"skipped_tests": N` alongside the existing `"checked"` on the `http_timeout_scan` event and assert `checked > 0` after filtering. Extend the detector test at `swarm.rs:5342` with a `tests/test_x.py` fixture holding a no-timeout `HTTPConnection` and assert it is absent from findings while `pkg/bad.py` survives.

**Evidence.** 56 of 71 round-0 findings (79%) are no-timeout findings; of the 60 naming a parseable file, **33 (55%) are test files** — 28 on `tests/test_api.py`. The scorer's matching check `client_timeouts` is Tier D and grades the app's vendor client. Verbatim finding: `tests/test_api.py:95 calls HTTPConnection with NO timeout`.

**Readout (phase-level, no score).** Zero `complete_verify` finding_texts naming a `tests/` path — currently 51.9% of file-attributed no-timeout findings do, so **one counterexample falsifies the patch**. Plus `http_timeout_scan.checked` and the new `skipped_tests`.

**Expected.** Replay over all 24 archived runs already ran: round-0 findings 2.75 → 1.62 (-41%), paired per-run delta **1.12 ± 0.47 = 2.4 SE** (the only significant readout available on this fleet). All 27 test-attributed findings drop; all 25 `vendorsync/meridian.py` findings survive. Exactly 1 of 24 runs loses a repair round, worth 141 s of ungraded work. Per arm: n1 3.82 → 2.18, n3 1.85 → 1.15.

**What breaks if it is wrong.** A real app-side timeout defect reachable only through a test helper is missed — narrow, since meridian.py is untouched. The subtler risk runs the other way: fewer runs enter repair, and n3 already skips repair in 5/13 runs vs 0/11 at n1. Countered by the fact that no-timeout findings always clear (all 7 unresolved findings in the corpus are pytest, zero are no-timeout).

---

### C2 · EXECUTE · CHANGE · rebuild the owned-file manifest from the POST-execute DAG
**Edit.** `scheduler.rs` — add `final_owned_files: Vec<String>` to `RunReport` (`scheduler.rs:371`, populated in `build_report` at `:2161` from `self.dag.tasks`, the same union already computed inline at `:905`). `swarm.rs:25815` — make `smoke_all_files` `mut`; immediately after `scheduler.run()` returns, `extend / sort / dedup`. Filter every appended entry through three one-line predicates: (a) reuse `app_scope_py`'s `safe()` — reject absolute paths and `..`/`.`/empty segments; (b) **require `cwd.join(f).is_file()` and non-empty at union time** — this is load-bearing, it makes a failed or never-dispatched bonus task unable to manufacture a "planned deliverable MISSING" finding, so the bonus-tasks-must-not-block-green invariant survives; (c) drop the task_split half of the rationale — `scheduler.rs:1991` enforces `union == orig_files` on every split, so splits contribute nothing and claiming otherwise invites a correct rejection. Emit `manifest_extended {files, task_ids}`. Gate `GOOSE_SWARM_LIVE_MANIFEST`, **default OFF**.

**Evidence.** `orphan_files` fires in 11/17 n3 runs and **0/12 n1 runs**; 16 of 19 orphan filenames map 1:1 to a `replanned.added` or `task_split.children` id. `replanned` 14/17 vs 0/12. 6 of 19 orphans are bare root paths that `scope_dirs` skips entirely (`swarm.rs:15733`), so `run_ast_review`, `cross_module_drift` and `http_timeout_scan` never see them either. Source: `swarm.rs:25815`, comment "the scheduler consumes `dag`"; 15 downstream consumers.

**Readout.** `orphan_files` count and file list. Pass: the 16/19 attributable orphans go to zero **while the 3 unattributable ones survive** — a run reporting zero orphans including the unattributable ones means the manifest went permissive and the detector is now blind.

**Expected.** Replay over the 28 archived logs already ran (union of `task_dispatched.owned_files`, which mirrors the post-replan DAG): **17/19 orphan files covered, 9/11 orphan runs to zero, 2 genuine unplanned files surviving** — exactly the pass condition, with the blindness failure mode ruled out. Score effect: none claimed.

**What breaks if it is wrong.** The risk is inverted — more files visible to the fix loop means a longer repair tail if those replan-added tests are bad. That cost lands in COMPLETE, not EXECUTE, and it fires only at n3, which is precisely the direction that can break the hard floor. Hence default OFF and COMPLETE wall reported separately from EXECUTE.

---

### C3 · PLANNING · CHANGE · the ladder shadow asks a different question from the branch it mirrors; and `struct_stop=80` is provably inert
**Edit (b), unconditional, ship first.** Do **not** relocate the `plan_convergence` emission — `swarm.rs:14312-14316` deliberately places raw `agreement_conf`/`pool_penalty` before the lift and moving it silently redefines two diagnostics across 24 archived rounds. Instead hoist the value: `let conf_lifted = conf1.max(best2);` above the emission at `:14292`, keep `"agreement_conf": conf1` unchanged, and emit **both** `"would_skip_ladder": diverse_plan_would_skip(struct_conv, struct_stop, conf_lifted)` (== the branch at `:14323`) and `"would_skip_ladder_prelift": …conf1` (continuity with 24 archived rows). Add a **call-site** regression test asserting the third argument is the same value in both places for every archived triple — the existing test at `:9376` tests the pure predicate, which is exactly why a caller passing two different arguments stayed invisible.

**Edit (a).** `default_struct_stop()` (`swarm.rs:87`), the Default literal (`:1236`) and the assertion at `:8320`: **80 → 95**. Rewrite the doc at `:489-496` to state the sweep.

**Evidence.** All 24 multi-draft `plan_convergence` events: `struct_conv ∈ {100×18, 93×4, 95×1, 86×1}`. 80 is **below the observed minimum**, so `struct_conv >= struct_stop` is TRUE 24/24 and the lever collapses to `struct_conv > agreement_conf` — "always skip". Sweep: 80/85/86 → 19/24 (identical, proving inertness across the whole range ≤86), 90/93 → 18/24, 95 → 14/24, 100 → 13/24. Shadow/enforce disagree on **2 of 24 rounds (2 of 10 pool-invariant-reachable), both false-positive**.

**Readout.** `would_skip_ladder` starts varying instead of reading a constant TRUE; and it must equal the branch outcome 1:1 whenever `enforced=true`. Free — rides on runs already scheduled, readable after ~3 n3 runs.

**Expected.** Zero behaviour change while `diverse_plan` stays OFF. The lever becomes honest: at 80 it means "disable the ladder", at 95 it means "skip when drafts are structurally near-identical". State plainly that on top of the already-shipped pool-invariant (7/24 rounds rescued), 95 adds only 1 more round while 90/93 would add 5 — **and those 5 are the low-agreement rounds (68-81 against struct_conv 93-100) where skipping is the forbidden quality-for-speed trade.** 95 is the conservative default; 90/93 is a deliberate, untaken speed option.

**What breaks if it is wrong.** Nothing on today's default path. The risk of *not* doing it: someone reads 80 as a real threshold, turns `diverse_plan` on, and silently disables the n3 quality gate outright.

---

### C4 · SINK · CHANGE · the shard reports are unreadable, so "did 64% more verification find 64% more problems" is unanswerable
**Edit.** **Drop `verify_coverage` and `shell_ok` from the engine** — every field is computable post-hoc from `task_completed.tool_calls`, already in the log; ship them as an analysis script, not as six emit-site churn in Rust. **Ship the one genuinely new bit**: on `SwarmEvent::TaskCompleted` (`event.rs`) add `finding_texts: Vec<String>` — the same field name and shape `complete_verify` already emits — capped 8 × 400 chars, populated at `scheduler.rs:1066` **before** `self.ctx.merge(tid, output)` moves the string, gated on `owned_files.is_empty()`. Populate at the other five emit sites too (`:1182, :1198, :1255, :1720, :1877`) — the salvaged and failed paths are precisely where an unexecuted verification hides. Add `oracle_len` to the **existing** `sink_plan` event (`swarm.rs:12646`), not to a new roll-up.

**Evidence.** 21 of 77 e2e shards and 17 of 25 joins completed `status=done` with **zero shell tool calls** — 18-22% of all sink node-minutes. The report text exists only in `~/.local/share/goose/sessions/sessions.db`; `task_completed` carries status/elapsed/device and nothing about what was found. The identical "empty tool_calls ⇒ it didn't look" predicate already ships for research at `swarm.rs:10424`.

**Readout.** State it as an **absolute, not an arm delta**: `join_executed` = 8/26 today. The n1-vs-n3 exec-rate gap (0.05, SE 0.10) is unresolvable here and claiming it repeats the score trap. An intervention that makes the sole integration oracle actually run something clears a binomial in 6 runs.

**Expected.** Zero wall, zero score, by construction. Makes every subsequent sink proposal falsifiable in one run instead of ~200. Log growth ~90 KB/run on a 200 KB jsonl.

**What breaks if it is wrong.** Nothing behavioural. If the excerpt were mis-gated, implementer output enters the log — hence the owns-nothing gate.

---

### C5 · PLANNING · CHANGE · make the log say when the confidence gate did not run at all
**Edit (A, land now).** Three additive fields on `skeleton_drafts` (`swarm.rs:14186`, all three in scope at the emit): `requested_best_of_n` (pre-clamp), `distinct_draft_models` (`draft_models.len()`), `clamped` (`n < requested_n`).
**Edit (B).** On the `n == 1` path emit a **distinct** event — `plan_convergence_skipped {reason:"single_distinct_draft_model", …}` — **never** `plan_convergence`. Reusing the name breaks `armcheck.py:308` (returns UNKNOWN on absence) and `tierlog.py:107` (takes the first occurrence as conv1) simultaneously.
**Edit (C).** Fix the two source comments at `swarm.rs ~14260` and `~14300` that assert one node drafts 2 skeletons and that the pool-invariant is a no-op at one node "because best2 == conf1 at 2 drafts". Both are false on this fleet — one node drafts 1 and the block never executes.

**Evidence.** `plan_loaded.plan_confidence` NULL in 11/11 one-node runs, non-null in 17/17 three-node runs. `skeleton_drafts.requested` is 1 on every one-node run despite `ask_floor` resolving to 85 on both arms. `draft_models` is deduped by model name at `:14013`, so a one-device fleet has one distinct model. And the retarget finding: **11 of 11 redraft rungs escalated `best_of_n` to 4 or 5 and drafted 3** — the ladder's only mechanism is structurally inert on a 3-model fleet while still costing a full planning pass.

**Readout.** The fields themselves; zero runs needed for correctness (already settled by replay). One n3 run confirms wiring: requested_best_of_n 3→4→5 while distinct_draft_models stays 3 and `clamped` flips true from round 2. **Control:** `armcheck.py` and `tierlog.py` must produce byte-identical output on pre-change logs.

**Expected.** No behaviour change. Removes the standing misreading that "one node never ladders" is a behaviour difference when it is a **capability** difference — the confound that would make "cut the ladder" look free.

**What breaks if it is wrong.** Nothing. If any rung ever reads `returned > distinct_draft_models`, the clamp is not what limits the pool and the inert-rung story is wrong; max returned in the corpus is 3.

---

### C6 · COMPLETE · CHANGE · a by-design abstention demotes the run's only verification claim
**Step 1 (pure instrument, ship first, zero semantics).** One field on `complete_verify` at `swarm.rs:26313`: `"inconclusive_reasons": verdict.inconclusive.iter().map(|r| elide_middle(r,150,650)).collect()`, mirroring what the `spec_contract` event already does at `:26270`. Today the merged list exists only in stderr, which is why the true flip rate had to be reconstructed from 9 surviving `verdict.json` files instead of all 24 event logs.
**Step 2 (semantics, gated `GOOSE_SWARM_COVERAGE_GAPS`, default OFF).** Split `SpecContractResult` into `inconclusive` (a check TRIED and could not conclude: spawn error, curl timeout, port bound) and `coverage_gaps` (the `spec_unprobed_advertised` string only, `swarm.rs:17510`). Stop extending `coverage_gaps` into `verdict.inconclusive` at `swarm.rs:26292`. Then, to keep the intent the change otherwise reverses: (a) `complete_result` gains `"coverage": {probed, unprobed, gaps}` so `verified:true` is never readable without knowing POST /api/sync was never touched; (b) keep the strict reading for the one consumer that needs proof — gate the persona save on `final_passed && final_verified && coverage_gaps.is_empty()`.

**Evidence.** `complete_result.verified == false` in **25/25** runs. `spec_contract` emits `inconclusive>=1` in 50/50 events, 47/50 reading exactly `verified:2, findings:0, inconclusive:1` — it probed two endpoints successfully and abstained on one it never probes. `established()` (`swarm.rs:15956`) = `self.ran && self.inconclusive.is_empty()`. The unprobed push self-describes as "an admission about the CHECK, not a defect in the app".

**Readout.** From step 1, for free, on runs the loop does anyway: among runs whose final `complete_verify` is `ran:true, findings:0`, the fraction whose `inconclusive_reasons` contains **only** the "advertised endpoint(s) were NOT probed" string. That fraction **is** the flip, exactly, not by counterfactual. Budget ~8-10 green runs before reading it, not 3.

**Expected.** `verified` 0/24 → roughly **6 of 18 green runs** (measured 3 of 9 recoverable; Wilson 95% CI ≈12-65%). The rest keep a real pytest-timeout or port-bound abstention and stay false, which is correct. Zero wall, zero score. Payoff to name: `unwired_demotes_verified` (`:27173`) becomes reachable. **Drop the "unblocks the persona snapshot" claim** — `detect_stack_key` blocks that independently.

**What breaks if it is wrong.** `established()` was tightened to close 4/4 measured false greens where pytest **timed out**, `smoke_output` returned None, and the run claimed verified having executed nothing. Cut on the wrong side and that re-opens. Mitigation: only the never-probed-by-design string is reclassified; `passed` is untouched, so no app can be flipped green by this. Side benefit of step 1: it is the only readout anywhere for the rate at which green runs shipped with pytest timed out.

---

### C7 · RESEARCH · CHANGE · the engine tells 77% of its scouts a falsehood
**Edit.** `swarm.rs:13467` — hoist `let doc_urls = spec_doc_urls(user_prompt);` out of the closure and split one boolean into two: `has_mcp = !exts.is_empty()` drives `tool_hint` **only**; `has_docs = !doc_urls.is_empty()` drives `lookup_clause` **only**. `tool_hint` keeps today's false-branch text verbatim. `lookup_clause` gets three branches: `has_mcp` → today's; `!has_mcp && has_docs` → "The spec names these documents: `<doc_urls>`. Fetch each with `curl -s <url>` as your FIRST action and quote it VERBATIM for anything you assert about the vendor API; mark anything not present in the fetched text as UNVERIFIED."; neither → today's text unchanged. **That last branch is what makes it safe: a spec with no URL is byte-identical to today.**

**Evidence.** `research_completed.grounded` mean 2.27/3, **zero runs at 0**; per lens libraries 22/25, architecture 21/26, edge-cases 16/26 — **59 of 77 scouts (77%) fetched a URL while under an explicit instruction that they cannot**. `research_lookups` (`swarm.rs:10501`) grounds on `is_mcp || fetched_external`; `fetched_external` is set by a shell curl (69fd7a419, 2026-08-02 08:33); the contradicting clause is c129bee6b, same author, same day, **12h43m later**. The comment at `:13460` claiming "grounded was 0 on every run" is stale and refuted by the corpus.

**Readout.** **Not `grounded`** — the instruction guarantees it, so it is circular. Use per-lens **scout contract-token coverage**: count doc-only vendor tokens (`next_cursor`, `Retry-After`, `ETag`, `Idempotency-Key`, `429`) in each lens's `finding_texts[*].text`. Real spread today (per-run 4..9). **Hard guard:** `prefix.research_secs` (mean 7.49, sd 2.51) must not rise — by this phase's own tier-C correlation (r=-0.650 within build, t=-2.90) a longer research phase is a loss regardless of grounding, and a rise triggers revert on that readout alone.

**Expected.** Ship the code-fact fix without claiming a measurable gain. If run at all: 5 runs one arm, no control — baseline per-scout grounded is 61/80 = 76%, so 15/15 is p=0.019 under the null.

**What breaks if it is wrong.** Scouts read it as licence to explore, turns rise, research_secs rises. And more verbatim vendor text into the planner prompt feeds the one phase already bleeding (+13.4 min, t=+3.63).

---

### C8 · PLANNING · CHANGE · do not detail a plan the ladder is about to discard
**Edit.** Factor out `detail_plan(&self, v, goal, findings, wm, lang, fan_verify_applied, …)` containing the detail fan (`swarm.rs:14957-15136`) **and the T2 sink canonicaliser (`:15137-15205`)** — T2 is post-fan and is what installs the joined/thin `integrate_verify_spec` on the sole end-to-end gate; splitting at the fan alone ships an un-canonicalised sink. Add `defer_detail: bool` to `parallel_plan` (`:13791`), set from `retarget_on` — **not** `retarget_round < retarget_cap`, which leaves the ask exposed on `RetargetAction::Ask/None` and on ReResearch-with-settled==0. Call `detail_plan` exactly once, immediately before each of the two break sites (`:25492` best_plan branch, `:25500` fall-through), on the plan that actually ships. The eight post-skeleton transforms stay in the skeleton path — all read ids/deps/files only. Gate `GOOSE_SWARM_DEFER_DETAIL`, default OFF.

**Evidence.** A rung is 16.96 ± 0.93 min (n=9) while `skeleton_drafts.secs` is 4.74 ± 0.32. `detail_completed` per run: **17-29 on laddering runs vs 6-12 on non-laddering**, 2.5×. `detail_memo` does not recover it — `detail_memo_key` (`:2805`) hashes the skeleton brief, which the redraft re-authors every round, so the key misses (subtask-id overlap 0.44-0.89 yet per-round counts stay flat at 9-13). The redraft decision at `:24843` reads only `plan_conf.final_conf`, which is finalised at `:14594` from the skeleton alone.

**Readout.** (1) total `detail_completed` on a laddering run: 17-29 → one round's worth (9-11). Zero within-arm variance across 24 observed rounds, so **n=1 settles it**. (2) rung `plan_convergence`→`plan_convergence`: 16.95 ± 0.95 → **11.5 ± 1.5 min** (the 6.14-min probe+transforms and 5.41-min decision+next-skeleton both survive). (3) free DAG-drift guard: `plan_loaded` task ids/deps/owned_files shape unchanged, count in the observed 12-23 band. (4) the shipped `integrate-verify` description still matches the canonical thin/joined spec — the T2 regression check.

**Expected, corrected.** **5.40 ± 0.65 min per discarded rung; 6.1 min per laddering run; 2.9 min/run across the n3 arm; planning 26.1 → ~23.2.** Not the 8-11/rung and 19-21 originally claimed.

**What breaks if it is wrong.** Quality risk is nil by construction — the redraft decision never read the detailed specs and the shipped plan is detailed exactly as today. One real consequence to name rather than deny: `clarify_questions`' `plan_excerpt` becomes a whole-DAG skeleton view instead of ~one detailed task; log the question set and eyeball it once. And it retires an instrument — `retarget_discarded.desc_sha` (`:24962`) starts hashing skeleton briefs; retire it deliberately in the same commit.

---

### C9 · EXECUTE · CUT · take the read-only gate off the critical path (one line)
**Edit.** **Leave `fan_verify_split` entirely alone** — its join loops are already overwritten by `fan_e2e_split` in 27/27 runs, so editing it is a dead edit. In `fan_e2e_split` change the shard's `"depends_on": verify_ids` (`swarm.rs:3607`) to the union of those verify tasks' own deps (the modules). `integrate-verify`'s dep set is untouched, so the join's context slice cannot regress. Gate `GOOSE_SWARM_VERIFY_NONBLOCKING`, default OFF. The existing tests at `:4081-4230` fork per lever state.

**Evidence.** DAG depth is **exactly 4 in 16/16 n3 runs, zero variance**, while root width is 5.8 and layer-1 width 8.6 — the plan is wide enough for 6 slots and too deep anyway. A `verify::<M>` sits on the critical chain in 12/14 runs. The task's own text: "You own NOTHING and must WRITE NO files — this is a read-only per-module gate", `"files": []` at `:3706`. A task that writes nothing cannot change the tree its successor reads; the edge carries no data, only delay.

**Readout.** (1) DAG depth from `plan_loaded[].deps`: 4 in 16/16 → 3, deterministic, **n=1 falsifies the patch**. (2) e2e-shard ready-lag = dispatch ts minus max(dep completion ts); measured median gain 3.96 min against 63 archived controls. Do **not** try to read 2 min off the 26.3-min wall sd — that needs ~434 runs/arm. (3) **The quality guard and the only reason the arm exists:** per-shard report length and concrete golden-value finding count, ON vs OFF, pulled from `sessions.db` via the `session_id` on `task_completed` (or from C4's new field once landed). The shard's prompt content swaps from N verify reports to N implementer reports; if reports degrade, revert regardless of the schedule win.

**Expected, corrected.** **-2.06 min mean / -1.60 median** on n3 makespan from the 3-machine list-schedule sim on measured elapsed_ms; zero in 3/16 runs; exactly zero at n1. Sell it as a structural simplification with a small n3-only schedule win, not a 7% phase cut. **Delete the "a broken module degrades the shard report" risk** — 151/151 `verify::` completions are `done`, so the shard already runs against whatever tree the modules left.

**What breaks if it is wrong.** The shards lose the verify reports from their context slice (`slice_for` is keyed on direct deps). They gain the implementers' reports for the same modules; the join never had the verify reports anyway. That swap is the unknown, and readout (3) is the test.

---

### C10 · RESEARCH · REMAKE · turn on the deterministic doc fetch — **this one needs no rebuild**
**Edit.** `swarm.rs:1291` — `doc_fetch: false → true`. **Ship (a) only. Drop the `select_lenses(spec_has_docs)` cut entirely** — leave `select_lenses` and its two call sites untouched, so nothing is removed from the run and the change cannot cost a grounded source. Note: `swarm_gate_cfg("GOOSE_SWARM_DOC_FETCH", …)` already exists at `:24141`, so **this can be tested on today's binary, right now, with zero rebuild**.

**Evidence.** The orchestrator fetch at `swarm.rs:24140-24205` already prepends the document **verbatim** to both `research_findings` (planner) and `doc_facts` (every worker), on the orchestrator, 20s reqwest timeout, zero fleet time. It has **never run**: 0 `doc_fetched` events in 17,215 lines across 54 logs, `doc_fetch=false` in all 26 runs. Both `doc_fetch` and `doc_prefetch` are off on a belief written into their own doc-comments (`:968-971`, `:3430`, `:24069`) that grounding is `is_mcp && ok` so they would forward nothing — 69fd7a419 made it `is_mcp || fetched_external` and the corpus reads grounded 2.26/3 with zero extensions attached. **That belief is dead.**

**Readout, all deterministic, n=1.** (1) exactly one `doc_fetched{ok:true, status:200, bytes:4789, truncated:false}` — today zero, so any non-zero is unambiguous; `ok:false` with an error means the loopback GET failed and the run is **void, not a negative**. (2) The worker half, which is the genuinely new channel: grep `~/.local/state/goose/logs/llm_request.*.jsonl` for the literal "Documentation retrieved from the spec's own URLs" — it appears in **0 of 739 files** today and must appear in every worker request. `task_dispatched` carries no prompt text, which is why the original readout was unusable. (3) `prefix.research_secs` flat, and `task_completed.elapsed` for the **test-author** kind as the volume canary.

**Expected.** The payload is **4,789 bytes, not 24 kB** — the cap never binds. Context cost is **+4.8 kB on a ~22.5 k-char worker prompt, ~+21%**, and that is the single real risk. Do **not** claim the planner lacks `/v1` — it has it in 12/12 runs. The claim is narrower and correct: `doc_facts` is `""` in every archived run, so **no worker has ever received one byte of the vendor contract verbatim.**

**What breaks if it is wrong.** EXECUTE slows on 27B workers under +21% prompt. **Pre-registered follow-up so a bad result routes somewhere:** gate the `doc_facts` block **by task kind** (vendor-touching implementers yes, test-authors no) — not by cutting a scout lens, which conflates a volume lever with a research lever.

---

### C11 · SINK · CHANGE · feed the join the module reports it was built to act on
**Edit.** In the existing owns-nothing sink branch (`scheduler.rs:915-930`, which already injects `judge_notes` for exactly this reason), append the stored `n.result` of completed `verify::*` tasks — but **filter, do not cap**. Inject only reports carrying a defect token (`✗`/`❌`/"MISSING from"/"is a stub"/"method mismatch"/"does not match"/"not defined"/FAIL outside a negation). An 800-char cap is the wrong instrument: p90 report length is 805, so it never binds and every green report still lands. **Inject into the context slice (`scheduler.rs:869`), not `prior_hint`** — `swarm.rs:22168` renders prior_hint as "your previous attempt was stopped", which is simply false on the sink's first attempt (and is already live today for the judge_notes block); `swarm.rs:21326` renders the slice as "Relevant context from completed dependencies", the honest frame and the same channel the e2e shards already receive these reports through. Timestamp each lead: "leads from per-module verification as of HH:MM — CONFIRM against the current tree before changing anything". Emit `sink_module_leads {injected, task_ids, texts}`. Gate `GOOSE_SWARM_SINK_MODULE_LEADS`, default OFF.

**Evidence.** `fan_e2e_split` (`swarm.rs:3626`) drops every `verify::` dep from the join, and `do_claim` builds the prefill from **direct deps only** (`scheduler.rs:870`), so the 5.71 module verify reports per run never reach the sole repair point. Those verifies are the honest half of the sink — only 2 of 139 made zero shell calls (1%) — and cost 15.31 ± 0.99 node-min/run at n3. The join's slice is 3849 ± 571 chars over 3.93 deps.

**Readout.** `sink_module_leads.injected` (deterministic, one run proves the plumbing) and `sink_min` (join elapsed, ~8 min at n3) to confirm the filtered block costs no measurable wall — 6 runs settle both. **Do not use round-0 `complete_verify` findings as primary**: sd 1.97 against a possible effect ≤0.3 needs ~345 runs/arm.

**Expected, and stated against my own gate.** Filtered, this is **0.3 reports/run at ~470 chars** — sub-KB, effectively zero critical-path cost. I pre-registered a gate of ≥1 actionable non-duplicated module finding per 3 runs and **the offline census does not clear it**: 3 candidates over 30 runs, 1 self-labelled transient, exactly 1 confirmed non-overlapping (the `/api/sync` GET-vs-POST case). So the honest call is: **ship the filtered injection as a near-free lever, do not expect a findings delta this fleet can resolve.**

**What breaks if it is wrong.** A stale lead describes a file the shards already repaired and the join "fixes" something correct — hence the timestamp and the CONFIRM wording. Unfiltered, it would be 2.7 kB of "Module passes verification. No errors found." diluting the sink's attention on the run's single serialisation point.

---

### C12 · EXECUTE · CHANGE · stop starving the split child
**Edit.** (a) Flip **both** readers in one commit: `scheduler.rs:58` `split_inherit_spec_enabled()` to the `salvage_spin_enabled` shape (`!matches!(env, "0"|"off"|"false"|"no")`) **and the identical shape at `swarm.rs:24471`** — there is no `SwarmConfig` field to bridge and one cannot be added from `goose-swarm`; miss `:24471` and every run's `levers_resolved` lies about what it ran. (b) **Land `description_chars: spec.description.len()` on `TaskDispatched` (`scheduler.rs:901`) unconditionally and first** — behaviour-free, `description` is already bound at `:870`, and it retroactively makes both arms of a lever that has never been observed comparable. (c) Run the flip with `GOOSE_SWARM_OWNED_FILE_FENCE=1` (`swarm.rs:849, 12122, 21288`) so the named clobber risk becomes a **counted, self-healing event** instead of prose enforcement checked by a detector that reads 0 in 29/30 emissions.

**Evidence.** `split_inherit_spec = false` in **28/28** runs — the starved child is live in every run on record. `task_split` fires 6/17 at n3, 0/12 at n1. Parent spec is **2514 chars mean** (1418-3468) against a 43-char replacement (`scheduler.rs:78-84`). Restricted to tasks with ≥1 dep, split children arrive with 560 chars of context slice vs 1406 for normal tasks (t=-5.53). **Splitting correlates with HIGHER quality (0.7710 vs 0.7355)** — so this is not "stop splitting", it is "stop starving the child".

**Readout.** Not `description_chars` — that only proves the string changed. Use the **per-file pass rate**: the verdict scorer's Tier-A checks name files and methods ("5/5 named files"; "8/8 methods, missing […]"), and `task_dispatched.owned_files` says which task owned each. Compare pass rate for split-child-owned vs normal-owned files — a proportion over ~32 child-owned files pooled across runs, orders of magnitude more power than the 0.1767-sd score, **and the OFF-arm baseline is computable from data on disk for zero fleet-hours.** Secondary: `owned_file_violation` count.

**Expected.** Child instruction 40 → ~3100 chars, certain. Downstream effect unknown and untestable by score here. **Stop calling it "43 characters of total prompt"** — the child does get a dep context slice (mean 803 chars), the frozen-contract bundle and the file manifest; what it never gets is the parent's implementation spec.

**What breaks if it is wrong.** The child writes its siblings' files. It holds a lock only on its own, so it *can* clobber a concurrent sibling; the `apply_split` partition validator (`scheduler.rs:1983-1995`) guarantees disjoint file sets, so a clobber is detectable — and `OWNED_FILE_FENCE` both detects and restores. **Harness consequence the proposal missed:** flipping the default converts `baseline` and every other arm in `sweep.py` into inherit-spec arms, makes the queued `split_inherit_spec` arm (`sweep.py:240, :633`) a no-op duplicate, and re-points the `split_off` control (`:253`) at a different question. Rename the control to `split_inherit_off` in the same commit, or hold the default OFF. Also: `runs/nodeloop/split_inherit_spec-n3-r0..r9` all exist as `void:true, harness_ok:false`, written in 3 minutes on Aug 5 — **that arm has never executed.**

---

### C13 · SINK · CHANGE · a shard that never ran becomes a deterministic, clearable finding
**Edit.** Trigger: a `verify-e2e::<i>` whose `task_completed.tool_calls` array is **EMPTY** (not "zero ok shell calls" — the stricter predicate loses nothing and is unambiguous). Target: the rows that shard owned under the **real** rule `position % shards == (i+1) % shards` (`swarm.rs:3543`), **intersected** with the rows `spec_contract` structurally cannot see — `spec_unprobed_advertised` (non-GET) plus the GET rows its `{`/`<`/`:` placeholder filter drops. On this bed that is exactly `POST /api/sync` and `GET /api/payments?limit=<int>&offset=<int>`, and it correctly excludes `/api/health` and `/api/summary`, which are green in 12/12 trees. **Clearability is the load-bearing part:** do not hand a prose finding to a model — extend `run_spec_contract` (which already spawns the app on a free port with a scratch DB `goose-spec-contract-<pid>.db`, so a POST is side-effect-contained by construction) to issue those rows itself, **and assert the value oracle: `fetched > 0`**. Re-evaluated per round like `missing_source_deliverables`, so a fix genuinely clears it and the mustsolve-test5 never-green trap cannot recur. Gate `GOOSE_SWARM_UNEXECUTED_VERIFY_BLOCKS`, default OFF, joins `delivery`.

**⚠️ Read this against the dead end in §5.** The **key-set / advertised-key comparison is DEAD** — it measured 3/5 against the grader and was wrong on both cells that scored `sync_shape` 0.00. What survives is only the **value assertion**, and it is the single highest-yield line here: four of twelve trees return `200 {"fetched":0,"inserted":0,"total":0}`; a key-set compare passes them, `fetched > 0` does not. **Do not ship this without the offline replay in §3 reproducing 4 correct / 4 false-green / 2 RemoteDisconnected / 1×500 / 1×502.**

**Evidence.** 24 of 85 shards have empty `tool_calls`, all `status:done, salvaged:false` (7/22 at n1, 17/63 at n3). At three nodes each shard owns exactly 1 of the spec's 4 endpoints, so one silent shard leaves 25% of the advertised surface unchecked while the run reports done. This **does not** touch the `owns_nothing` filter on `green_blocking_failed` (`swarm.rs:20974`) — that filter is correct as written and must not be reversed; the defect is that nothing about the sink is deterministic enough to block. "This task made zero tool calls" is an engine fact of the same class as `failed_task_finding`.

**Readout.** `spec_contract.verified` 2 → 4 (proof the unprobed rows are now probed); `complete_result.remaining_findings` must be 0 in at least one run (**proof the finding is clearable — if it is ≥1 in every run, this is the mustsolve-test5 trap and reverts**); `complete_fix_dispatched` count at n3 answers the floor question, since the only population that can gain a round is the 5/16 n3 runs where round-0 currently passes. All deterministic; one run per arm.

**Expected.** Adds ~1 fix round in the ~55% of runs with ≥1 silent shard; +3 to +8 min wall in those runs, roughly equal across arms (0.64 vs 1.00 silent shards, t=1.15), so the floor is not selectively harmed. This is the only change that converts an unchecked endpoint into an executed one.

**What breaks if it is wrong.** The fix worker "satisfies" the finding by writing a test instead of running the command — hence the finding demands the run and the readout is exec rate, not findings-cleared. Also correct the abstention rationale: an empty oracle does **not** mean the shard owned nothing (`fan_e2e_split` still cuts 2-4 shards with an empty oracle); it means the engine cannot name what the shard owned, which is a better reason to abstain.

---

### C14 · RESEARCH · CHANGE · cut the scout work budget — **instrument before spending an arm**
**Edit.** Split into two independently-attributable levers. **L1**: `default_scout_max_lookups()` (`swarm.rs:1109`) 10 → 4. **L2**: `turns_after_ground` on `ScoutBudget` (~`:1095`), keyed on the **docs** fetch specifically (match the URL path the spec named, not any `http://`), defaulting to **2, not 1** — otherwise a scout that curls `/v1/payments` before `/v1/docs` is cut before it ever reads the contract, the exact quality loss this change claims to avoid. **Prerequisite: land `scout_completed {lens, model, secs, TURNS, lookups, chars}` at the scout closure (`swarm.rs:13436`) first.** Without `turns` nobody can show the 10-cap binds at all.

**Evidence.** Research ranges 4.63-15.31 min, sd 2.52. Longer is worse and it survives both obvious confounds — **within build** (r=-0.651 n=8, r=-0.649 n=12, pooled Fisher-z t=-2.90 on tier C) and **within arm** (n1 r=-0.801, n3 r=-0.498). Median split at 6.57 min: tier C 0.604 vs 0.868 (t=-2.69), score 0.665 vs 0.799 (t=-2.00), **wall flat** (103.74 vs 101.37) — the long tail is not even bought with visible wall. Grounding *rises* with research minutes (r=+0.381) while tier C falls, so the extra turns are demonstrably not buying better facts.

**Readout.** **Replace `grounded`** (tautological for L2, saturated, ~91 runs/arm) with **contract-token coverage** of `research_completed.finding_texts` over `{Retry-After, HTTP-date, 410/cursor_expired, ETag/If-None-Match/304, next_cursor, 409/Idempotency, RFC-3339/UTC offset}`. Already in every archived log, deterministic, valid at n=1, and it is the variable that actually tracks tier C (r=+0.377, t=+2.44, n=38) while research minutes do not (r=-0.045). Frozen-build floor: **5.42 of 7, sd 1.73, min 3**. Revert rule: below 4/7 on any run, or arm mean down more than 1.0. Speed readout `prefix.research_secs`, frozen-build sd 1.84 min ⇒ 3 min needs ~7 runs/arm, 2 min ~14. **Run the arm at three nodes**, not one — n1 has two lens waves and shows roughly double the saving, flattering the change on the one axis the goal does not want moved.

**Expected.** Honestly unknown, and it may be a **no-op**: if the modal scout uses ≤4 turns, `scout_max_lookups=10` never binds and the entire 7.41→5.0 projection is dead. Two instrumented runs decide that for the price of 2 runs instead of 22.

**What breaks if it is wrong.** This is exactly the shape the brief forbids — a speed cut — defensible only because the quality correlation runs the other way. The honest weakness is **reverse causality**: a run where the vendor API confuses the model may research longer *and* build worse, in which case shortening buys nothing and loses the cases where the turns were needed. The guardrail is the discriminator: if a 4-turn cap holds token coverage at 5.4 while research_secs drops, the turns were waste; if coverage collapses, they were not and this is a quality-for-speed trade to reject.

---

## 2. PER PHASE, ONE LINE

| Phase | Subpar? | Verdict | The one line |
|---|---|---|---|
| **RESEARCH / SCOUTS** | Yes | **REMAKE** | Costs 7.4 min in both arms (flat null, t=-0.12), its output has no positive relationship to the build (grounded≥2 scores −0.0067, t=-0.06) and its only signal runs backwards (tier C r=-0.650 within build) — take vendor-doc discovery away from the models (C10) and stop telling 77% of scouts a falsehood (C7). |
| **PLANNING** | Yes — the only phase quantity in the engine clearing 2 SE (+13.4 min, t=+3.63) | **CHANGE, never CUT** | The ladder's stated mechanism is provably inert (`n = requested_n.min(draft_models.len())` clamps to 3 distinct models, so best_of_n 3→4→5 buys nothing, 11/11 rungs) — make the rung cheap (C8) and the gate honest (C3, C5); **do not cut the ladder, it is the n3-only quality gate the n1 arm is structurally exempt from.** |
| **EXECUTE** | Yes in shape, not in scheduling | **CHANGE** | It is a scheduler correctly saturating a DAG that cannot use the fleet (CP-bound 13/14 at n3, still CP-bound simulating a 4th node) and then discarding the output of the extra work — un-freeze the manifest (C2), stop a write-nothing gate pinning the depth at exactly 4 (C9), stop starving the split child (C12). |
| **SINK / VERIFY SHARDS** | **No on cost — yes on honesty** | **CHANGE (instrument + feed), never CUT** | 64% more verification for 10% more wall and a *faster* join: **the one place parallelism demonstrably works** — but 27% of shards and 68% of joins executed nothing, and the join never sees the module reports. Make it readable (C4), feed it (C11), make silence a finding (C13). |
| **COMPLETE / SMOKE / REPAIR** | Yes | **CHANGE** | The phase's own verification claim is false 25/25 by construction (C6) and 55% of its file-attributed findings point at test files the scorer never grades (C1) — fix what it looks at and what it says, not how long it runs. |

---

## 3. THE BATCH — ONE REBUILD, AND THE DOCTRINE THAT MAKES IT SUFFICIENT

**The scarce resource is the rebuild (~4 fleet-days and a corpus reset), not the code. So: every uncertain change ships in THIS rebuild behind an env gate, default OFF. Then the entire experiment program runs on one binary by flipping environment variables.** Nothing below should ever require a second rebuild to test.

**BEFORE YOU REBUILD — snapshot the corpus.** Four of the fourteen readouts (C1 −41%, C2 17/19, C3 2/24, C12's OFF-arm per-file baseline) are baselines computed from the current 28-run corpus. Freeze `runs/nodeloop/{_archive/logs,eventlogs,*/run.jsonl}` and the replay scripts first, or the rebuild destroys the comparators.

### Do these FIRST — zero fleet-hours, zero rebuild, on today's binary
1. **C10 doc_fetch probe.** `GOOSE_SWARM_DOC_FETCH=1`, one n3 run. Pre-check for free in 30 seconds: `curl -s http://127.0.0.1:<port>/v1/docs | wc -c` must return 4789, or the arm is dead before any fleet time is spent.
2. **C13 offline replay.** 12 cells under `runs/nodeloop/*/vendorsync` still hold built trees; each cell's `run.jsonl` carries **its own randomized vendor port** (8930..9002 — a single-port replay silently produces false "connection refused", the one trap here). Must reproduce 4 correct / 4 false-green / 2 RemoteDisconnected / 1×500 / 1×502. ~90 seconds. If it does not reproduce, **C13 does not go in the batch.**
3. **C12 Stage-0 baseline.** Join Tier-A named files/methods in `verdict.json` to owning task via `task_dispatched.owned_files`; report pass rate for the 32 split-child-owned files vs normal. If they already pass at the same rate, the mechanism claim is in trouble before a single run is spent.
4. **C4 scope check.** One sqlite `SELECT` on `messages.content_json` for the 8 joins and 61 shards that *did* make shell calls: does the report text contain anything not derivable from `tool_calls` + status? If not, ship only `oracle_len` and keep the rest as a Python script.
5. **C3 replay** (already done — reproduce with `scratchpad/pc2.py`, `pc3.py`) and **C1 replay** (already done): both confirm before landing.

### The rebuild, in landing order

**Wave 0 — instrumentation, zero behaviour change. Land first so every later readout has its event.**
- C4 `finding_texts` on `TaskCompleted` (all six emit sites) + `oracle_len` on `sink_plan`
- C5(A) `requested_best_of_n` / `distinct_draft_models` / `clamped` on `skeleton_drafts`; C5(B) `plan_convergence_skipped` as a **new event name**; C5(C) the two false source comments
- C6 step 1 `inconclusive_reasons` on `complete_verify`
- C12(b) `description_chars` on `TaskDispatched`
- C14 prerequisite `scout_completed {…, turns, lookups}`
- C1 `skipped_tests`; C2 `manifest_extended`; C11 `sink_module_leads`
- C3(b) the `conf_lifted` shadow fix + both fields + the **call-site** regression test

**Wave 1 — shared-helper correctness. Land before anything that consumes a file list, or C2 inherits the bug.**
- The single `is_test_path(lang, f)` helper, applied at `swarm.rs:26105`, `:3028` and C1's new call site. Three copies of one rule that disagree is the mechanism; deleting the duplicate is what stops it recurring.

**Wave 2 — default-ON fixes that need no arm (settled offline or on the code fact).**
- C1 the scan-scope filter + the extended detector test
- C3(a) `struct_stop` 80 → 95 (plus `:8320` assertion and the `:489` and `:9373` docs)
- C7 the three-branch `lookup_clause`
- **Rider, not a graded proposal:** `GOOSE_SWARM_COMPLETE_STALL_ROUNDS` **provably cannot fire** (complete_rounds=2 ⇒ rounds 0,1 ⇒ the check needs round>0 ⇒ stall reaches at most 1 ⇒ firing needs 2; 0 fires in 54 logs) and carries **three contradictory comments about its own default**. Delete the lever or fix the comments. Do **not** "fix" it by raising `complete_rounds` — that is H8 below, and it is held.

**Wave 3 — gated levers, default OFF, one env var each. These are why one rebuild is enough.**
| Gate | Change | Arm cost to settle |
|---|---|---|
| `GOOSE_SWARM_DEFER_DETAIL` | C8 | **1 forced run** (`GOOSE_SWARM_ASK_FLOOR=100 GOOSE_SWARM_RETARGET_ROUNDS=1` guarantees exactly one discarded rung) + 1 free n1 control |
| `GOOSE_SWARM_LIVE_MANIFEST` | C2 | 3 n3 runs (n1 is a free null, 0/12 by construction) |
| `GOOSE_SWARM_COVERAGE_GAPS` | C6 step 2 | 0 — step 1's field decides it from runs the loop does anyway |
| `GOOSE_SWARM_VERIFY_NONBLOCKING` | C9 | 4 n3 runs, judged on shard report quality |
| `GOOSE_SWARM_SINK_MODULE_LEADS` | C11 | 6 runs for plumbing + wall |
| `GOOSE_SWARM_UNEXECUTED_VERIFY_BLOCKS` | C13 | 1 run/arm, all readouts deterministic |
| `GOOSE_SWARM_SCOUT_MAX_LOOKUPS` / `turns_after_ground` | C14 | **2 instrumented runs first** — may prove the cap never binds |

**Already env-gated — no rebuild, no code:** `GOOSE_SWARM_DOC_FETCH` (C10), `GOOSE_SWARM_SPLIT_INHERIT_SPEC` (C12), `GOOSE_SWARM_OWNED_FILE_FENCE`, `GOOSE_SWARM_SINK_REVIEW`.

**Harness edits that must land in the same commit as C12**, or the sweep silently measures the wrong thing: rename the `split_inherit_spec` arm to `split_inherit_off` (or hold the default OFF), re-point the `split_off` control (`sweep.py:253`), and delete the ten `split_inherit_spec-n3-r*` stub directories — all `void:true, harness_ok:false`, written in 3 minutes on Aug 5, never executed.

**Order of arms after the rebuild:** C8 (1 forced run, biggest phase movement, zero quality risk) → C2 → C13 (only if the replay passed) → C9 → C12 → C11 → C14. C10 and C6 need no arm at all.

---

## 4. WHAT MUST NOT BE TOUCHED

**THE SINK'S PARALLELISM.** Three nodes does **64% more verification for 10% more wall (46.0 vs 28.0 shard-minutes, 34.0 vs 30.8 wall) and the join is FASTER (11.7 vs 15.5)**, with median 4 concurrent shards vs 2. It is the **one clean win** measured anywhere in this engine. Every sink change above adds information (C4), adds input (C11) or adds determinism (C13) — **not one removes a shard.** Do not cut shard count, do not cut sink node-minutes, do not "optimise" the fan. If a future proposal's saving comes out of sink capacity, reject it on sight.

**PRE_REVIEW.** 2.0/run at one node vs 10.2 at three — **the only mechanism in the engine that scales with node count**. Do not contend with it: if `sink_review` is ever turned on, measure that `pre_review` count/run does not fall (`pick_prereview` runs first in the tick, `scheduler.rs:2680`).

**THE RETARGET LADDER AS A GATE.** Make it cheap (C8), make its threshold honest (C3), log when it is inert (C5) — but **do not cut the rung as a speed play.** It is the only mechanism that ever lifted a below-floor plan (3 of 8 sequences: 81→100, 81→88, 81→100), and the alternative path is worthless on this bench (all 7 `low_confidence_ask` events resolve to `low_confidence_ask_timeout` in 0.08 min — nobody answers). Cutting it ships those three runs' plans at 81, and whether that costs score is untestable here.

**THE `owns_nothing` FILTER ON `green_blocking_failed`** (`swarm.rs:20974`). Its doc is right: a model self-report may never veto green. It has never once excluded anything (integrate-verify is `done` 25/25). C13 adds a deterministic fact on the blocking side **without touching this filter** — keep it that way.

**`established()`'s STRICTNESS FOR GENUINE ABSTENTIONS.** It was tightened to close 4/4 measured false greens where pytest timed out and the run claimed verified having executed nothing. C6 reclassifies **only** the never-probed-by-design string. Timeout, spawn error and port-bound keep their full demoting power. Anyone widening that split re-opens the false green.

**`apply_split`'s PARTITION VALIDATOR** (`scheduler.rs:1983-1995`, `union == orig_files`). It is what makes a clobber detectable and what makes C2's split rationale unnecessary. Do not relax it.

**ALREADY SHIPPED — do not re-propose:** best-plan retention (`swarm.rs:24853`), the retarget stall guard (`:1250`), `replan_has_enough_dag_left`, the straggler stop (the lone last draft is **deliberately** aborted once 2 valid drafts land — by design, costs nothing measurable), and the ~40 golden-formula levers baked ON in `SwarmConfig::default`.

**`fan_verify_split`** — leave it alone. Its join loops are already overwritten by `fan_e2e_split` in 27/27 runs; editing it is a dead edit that enlarges C9's blast radius for nothing.

---

## 5. THE DEAD ENDS

### D1 · REJECTED · COMPLETE · probe the advertised non-GET endpoints and compare advertised KEYS (`run_spec_contract`, `swarm.rs:17316`)
**Ground: contradicted by the data. Not thin evidence — measured, twice.**
- **This exact experiment already ran.** FINDINGS.md **F402** built the POST probe plus advertised-key comparison and measured it against archived cells with the vendor mock up: **3/5 agreement, wrong on both cells that scored `sync_shape` 0.00** (the probe returned 200 with all three keys). The engine's bar is F398's 5/5. F402's own conclusion: "a checker that disagrees with the grader on 2 of 5 cells does not ship."
- **Re-derived on a larger corpus and it is worse.** Of 12 non-void cells, 8 have a broken sync; in **4 of those 8, `sync_shape` is 1.00** — all three advertised keys present, 0/247 payments fetched. Key presence is structurally blind to half the defects because the failure is a wrong **value**. On the other 4 the body carries 0/3 keys because the request errored — and the proposal's own mitigation ("non-JSON/non-2xx is inconclusive") suppresses exactly those. Applied to F402's five cells the stated rule yields **zero findings on 5 of 5**, including all three the scorer marks broken.
- **The corpus percentages quoted in support are artefacts.** `sync_shape` <1.0 is 4/12 = **33%**, not 56%; `health_shape` is **0/12 = 0%**, not 25%. Both quoted figures are recoverable only by counting the **105 void cells** where the harness never bound a server, so every body check reads a blind 0/3.
- **The cost claim is contradicted by the harness.** `score_build.py:87` gives POST /api/sync a **240 s** timeout because a full 247-payment sync runs through a deliberately throttled vendor; the engine's curl is `-m 5` inside `smoke_output(cmd, 8)`. As written the probe times out into a new inconclusive — inert, not "single-digit seconds" — and `spec_contract` runs ~2× per run.
- **Bigger blast radius than stated.** `spec_repair` is baked default-ON and **races repair attempts across the fleet** on a finding, so a false finding spends n3 time exactly where the campaign located the arm's wall variance; `pick_repair_winner` promotes on finding count dropping, which a stub key satisfies.

**What survives from it:** only the **value oracle** (`fetched > 0`), folded into C13 and gated on the offline replay. **Do not re-propose the key-set comparison.**

### D2 · STRUCTURAL DEAD END · any proposal whose only readout is the build score
Pooled score sd 0.1767 ⇒ ~196 runs/arm (~330 fleet-hours) for 0.05. Wall sd 26.3 min ⇒ ~434 runs/arm for 5 minutes. **Only phase-level metrics have low enough variance to be measurable on this fleet.** A proposal that cannot name a phase-level or deterministic readout is not ready, regardless of how good its mechanism sounds.

### D3 · BENCH DEAD END · testing research quality on the vendorsync spec
Plan fact-coverage is at **8.79 of 9**, and grounded=1 runs already reach 8.75. **No number of runs on this spec can show a benefit from raising grounding.** Before any research-quality arm is scheduled, grep `evals/swarm-bench` for a spec whose document carries facts the archived plans miss. If none exists, say so and stop proposing research-quality arms on this bed.

### H · HELD, NOT DEAD — earlier proposals not carried into the change list. One line each so they are not re-proposed as new.
- **H1 · PLANNING · sequential draft waves** (make the rung actually grow the pool past 3). Adds ~4.7 min/rung and can make three nodes slower — a hard-floor risk — and `draft_temp` steps *down* 0.05/round, pushing toward less diversity, not more. Revive only behind a lever with the monotonicity readout (agreement must never decrease across a rung; it decreases in 3 of 7 today).
- **H2 · PLANNING · cut the Redraft rung when the pool cannot grow.** The **alternative** to H1, not a pair — ship only if H1 fails its monotonicity readout. Quality risk, not speed: it ships 3 runs' plans at 81 instead of 88-100, and the fallback ask is worthless on this bench.
- **H3 · PLANNING · relative count-spread penalty in `plan_agreement`.** A gate loosening dressed as a metric fix: it makes below-floor plans clear the floor without changing the plans. **Hold until C8 has landed** — once the rung is cheap, loosening the gate buys much less at the same risk.
- **H4 · EXECUTE/SINK · `sink_review` idle-fill.** 18.6 idle node-min/run at n3 in the sink window vs 2.0 at n1, and the mechanism has never run (0 events in 54 logs) — but the reviewer can be co-scheduled onto the join's device and the join is the critical path. Needs no rebuild (`GOOSE_SWARM_SINK_REVIEW=1`); if run, abort on join elapsed, not sink-segment wall.
- **H5 · SINK · e2e replicas.** Same node-minutes, every endpoint checked twice. Cannot predict the disagreement rate, and the engine's one documented historical regression is a spec-distilled check turning a correct app red. Revive with the rule that a disagreement is a prompt to *run* the command, never itself a finding.
- **H6 · SINK · re-dispatch a join that executed nothing.** **The one proposal that can genuinely make three nodes slower** — the join is the critical path and fires slightly more often at n3. Do not run it until C4 and C13 are in, so the readout can distinguish "ran and found nothing" from "never ran".
- **H7 · COMPLETE · `fix_cap` 1200 → 960 and `complete_rounds` 2 → 3.** The empirical case is real (no attempt in the corpus ever completed between 929 s and 1198 s; 4152 attempt-seconds spent past the last time a fix ever finished) but n=22 natural completions is thin for a hard ceiling, and it drags three coupled defaults plus `config.yaml` (which carries the pre-raise 1200 and would silently reinstate it).
- **H8 · COMPLETE · make the `!verdict.ran` failed-task branch reachable.** It has fired **zero** times (`ran` is true 50/50) while its event string claims it is "driving the fix loop". **Fix the false string now** (fold into Wave 0); hold the reachability change — it is the guard's original bug and a static finding set here pins a good app red forever.
- **H9 · COMPLETE · `sink_finding_shadow`.** The instrument-only half is free and answers a question this fleet currently cannot answer; the gated half is the highest-risk change proposed anywhere (a model-authored verdict driving code edits — the engine has scarred itself on that twice). If revived, shadow first, gate never without a deterministic re-check.
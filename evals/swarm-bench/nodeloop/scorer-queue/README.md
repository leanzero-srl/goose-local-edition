# The freeze queue — what to land, in what order, and what argues against each

Everything here was found while the binary was frozen at `717ff4a6e` for the F533 spread
measurement. Nothing in this directory has been compiled. This file exists so the rebuild is an
ordered checklist rather than a re-read of six hundred findings.

## Two classes, and the distinction is load-bearing

**COMMIT-NOT-BUILT** — already committed to git, not yet compiled. Both are single additive JSON
values at sites where the data is already in scope, so they cannot break a build:

| commit | what |
|---|---|
| `81c11c21a` | F560 — `skeleton_drafts_round2`, instrumenting the second silent draft window |
| `9a49deda0` | F567 — `spec_contract.inconclusive_reasons`, the count alone caused a misattribution |

**SPECIFIED-NOT-CODED** — deliberately NOT written as code, because each needs a signature change, a
lock-scoped restructure, or a helper, and none can be compiled under the freeze. F578 is the rule
they all follow: committing code I cannot compile into a queue that must build cleanly is how the
queue gets poisoned.

## Order to land, and why

### 1. `F574-max-limit-phase-scope.patch` — LAND FIRST, it gates another question
`score_build._trace_split` returns four values; three are scoped to the graded phase and
`max_limit_used` reads the WHOLE trace. On `baseline-n1-r2` the same call returns `sync1_reqs=0` and
`max_limit_used=100`: the delivered client made zero graded vendor requests and still scored a
perfect 1.00 for page-size efficiency, off eight build-time requests. That app's `/api/sync` raises
on every call.

- **Bumps `scorer_version` sb-3 -> sb-4 and requires a RE-SCORE.** Never land mid-measurement.
- **It flatters the 3-node hypothesis** (+1.00 to a 1-node cell, +0.02 to a 3-node one), which is
  exactly why it is version-gated rather than applied quietly.
- Landing it is a PREREQUISITE for reconsidering an engine-side POST probe (F575), because it is the
  only remaining channel by which a build-time probe could contaminate tier C.
- Bounded impact: 0.02 of the composite.

### 1b. `F618-replan-projected-overlap.md` — pure instrument, and it re-orders the replan question
**Replan-injected tasks are 42.2% of all solo node-time at three nodes** (2704s / 11 runs), more than
the integrate-verify sink at 39.5%. Control: replan is arithmetically impossible at one node (F607),
and the 1-node arm shows **exactly 0.0s** of injected solo time. The idle-filler's own output runs at
concurrency ONE.

The guard already exists and was reasoned carefully — `replan_has_enough_dag_left` names this exact
harm and was calibrated on two measured cases at 14% and 11% remaining. **What is new is that the
harm recurs above its 25% bar**, because the bar is a proxy for "will this overlap" rather than a
measurement of it.

- **Pairs with F591, and arguably outranks it.** F591 asks why replan DECLINES; this says the more
  urgent question is why it ACCEPTS.
- **Ship the event, not the conclusion.** The entry carries its own counter-evidence: the injected
  work is all tests, F616 measured 3-node verification at 2.82x the problem-find rate, F611 priced
  the quality side at 183 cells/bucket, and F618 did not prove the run is longer.

### 1c. `F643-scout-lens-timing.md` — pure instrument, and it prices its own prize honestly
**The research phase does not get faster with the fleet**: 377.2s at three nodes against 395.4s at
one — 4.6% on the median, +39.1s at **0.64 SE** (n=11 vs 17). The lens fan is structurally
fleet-blind too (`select_lenses` never sees node count). ~7 min per run where extra hardware buys
nothing, on the pillar that looks worse.

Two of three candidate causes are already dead: the lenses DO fan out one-per-device
(`fanout_over_fleet_straggler`, swarm.rs:13436), and lens-drop is balanced across arms (0.0% vs
4.3%, F643). The Amdahl reading fails its own arithmetic — a 1.05 observed ratio where lens-bound
work would give ~2.0.

- **The phase is the least-instrumented in the engine**: 6m07s of total log silence between
  `scouts_planned` and `research_completed`, and `research_completed` carries lens NAMES but no
  timings and no device.
- **The prize is bounded and I am not hiding it**: ~7 min of a ~120 min run, so perfect
  parallelisation returns at most ~3 min, against F628's ~9.1 min/run ladder projection.
- **F642's registration was compromised** (guard refused the write while the measurement ran); the
  4.6% is disclosed as not-cleanly-pre-registered. The `select_lenses` source observation is
  unaffected.

### 2. `F591-replan-declined-reason.md` — pure instrument, no verdict changes
Dynamic replan's trigger is EIGHT conjunctive terms and the cap is not the binder (all four 3-node
cells fired one round against `max_replans` 2). The engine emits `Replanned` when it fires and
NOTHING when it declines, so which term blocks a second round is unanswerable from any log.
Gate the emit on `idle_capacity() >= 2` or it becomes F590's 666-row failure.

### 3. `F578-per-draft-device-timing.md` — pure instrument, EARNED not speculated
The draft round is the only place the engine hands every device an identical prompt at the same
moment — the system's one matched device-speed comparison — and it records neither which device
produced which draft nor how long each took. Earned because the attempt to answer it from
`task_completed.elapsed_ms` REFUTED ITSELF: the per-device ranking is unstable run to run (1.61x,
1.28x, 5.29x with a different winner each time), so that measure tracks task mix, not hardware.

### 4. `F603-per-draft-width.md` — pure instrument, the sharpest open question
The engine logs which skeleton it picked and never what it picked from. `score_skeleton` is ALREADY
fleet-aware (F602), so the selector is not the problem; a selector can only pick the widest candidate
it is given. Whether the drafts arrive at 5/5/5 roots or 4/6/8 decides between two OPPOSITE fixes,
and nothing on disk can distinguish them.

### 5. `F577-diverse-plan-enforce.md` — LAND LAST, and only behind its own arm
`diverse_plan` is built, unit-tested, defaults false, and `enforced: false` in every archived cell.
Enabling it would have skipped the ladder on `baseline-n3-r1` — a 0.9760 run, and at the time the
best of the frozen era.

**The evidence AGAINST it is stronger than the caveat I first wrote**, and it is quoted in the file:
the engine's own note records laddering cells at 0.9343/0.7147/0.8157 against 0.6030/0.6695, and
concludes *"the ladder may be buying the quality… Measure first."* This is a TRADE, not a free win,
and it needs its own arm judged on BOTH pillars.

**MEASURED 2026-08-08 (F612), and it is worse than that caveat.** `would_skip_ladder` is **TRUE for
all five laddering rounds in the corpus** — so `diverse_plan` would have skipped **every ladder this
campaign has ever run**, not just the one cell. That includes the single round F612 showed was
*genuine* (`agreement_conf` 81, `agreement_best2` 81, `pool_penalty` **0** — real disagreement on
both footings), which is the **0.9760** run, the best 3-node score of the frozen era.

### 6. `F612-pool-invariant-at-round-1` — the engine author's own fix, now evidenced
Not my idea: `swarm.rs:14260-14280` states it and closes *"Emitted rather than acted on… Measure
first."* `best_subset_agreement(k=2)` exists so a growing pool can only RAISE the metric and reports
every fleet on a 1-node footing, but it is wired as `consensus_k` (*"retarget only"*), so the
pool-invariant measure never reaches the round-1 decision point where the pool penalty is what
triggers the ladder. Their words: *"That is backwards."*

**The measurement they asked for (F612): 4 of 5 laddering rounds had `agreement_best2` clearing the
85 floor while `agreement_conf` did not** — penalties 10/19/13/10, raw conf 83/69/80/83 rising to
93/88/93/93 on the 1-node footing. Each such round costs **25.0 min (F610, 6.73 SE)**.

**🔴 DO NOT LAND THIS YET — MEASURED 2026-08-08 (F645, CORRECTED BY F646). ONE RUN SHORT OF DECIDABLE.**
The author's own words are `Measure first`, and the risk they named is `the ladder may be buying the
quality and a silent flip here could spend that`. F612 discharged the MECHANISM half. The OUTCOME half
cannot be discharged at all: of 11 real rows only **5 carry a known `ladder` value**, and in those five
**all 3 laddering runs are 3-node and both non-laddering runs are 1-node** — ladder and arm are
PERFECTLY COLLINEAR, so the within-arm comparison has no data in either cell. That is no answer, not a
weak one, and it is what F607/F612 predict once ladders are pool-bought.

⚠️ A first pass appeared to give a within-arm gap of +0.1463 (n=3 vs 3); the second group was entirely
rows whose `ladder` field is **None** — a could-not-look scored as a negative. Invalid, and withdrawn.

**CORRECTED 2026-08-08 (F646) — the collinearity above was an ARTEFACT OF MISSING DATA, and the
author's own objection has now FIRED.** `ladder` is the count of `confidence_retarget` events; the six
rows tierlog left as `None` had their run.jsonl overwritten by cell reuse, but **those logs survive in
the two archives**. Re-deriving recovered all six, and the derivation reproduces all five known values
exactly (1,0,2,0,1), zero mismatches, every match inside 6s. Corpus **5 -> 11 rows**.

Two recovered rows are **3-node runs that did NOT ladder** (0.4624, 0.7703) — precisely the cell
reported empty. So ladder and arm were never collinear; that was the shape of the hole.

**P1 FIRED: laddering runs average 0.7408 (n=4) vs 0.6900 (n=7) — a gap of +0.0508 at 0.43 SE against
a 0.10 bar. The author's five-cell 0.185 does NOT replicate.** So the stated reason for holding is not
supported — but +0.0508 +/- 0.1175 is UNINFORMATIVE, not evidence either way.

**P2 remains refused and is decisive: within 3 nodes it is 4 laddering vs 2 not — ONE SHORT of the
pre-registered minimum of 3.** Observed within-arm gap is +0.1244 and is NOT quoted as a result. At one
node laddering is structurally zero (F607/F612).

⇒ **The trigger is now concrete: ONE more 3-node run that does not ladder makes the within-arm test
runnable at 3 vs 4.** Two of the last six 3-node runs did not ladder, so this is a matter of runs, not
instrumentation.

✅ It becomes answerable at no extra cost: `tierlog.py` now records `ladder` on every new row, so each
future run adds a gradable point. The blocker is that 6 of 11 existing rows predate the field.

- **Denominator is 5.** A per-round mechanism check against the author's stated criterion, not a mean.
- **Does NOT license flipping anything by itself** — F611 measured the quality side of this trade as
  needing **183 cells per bucket**. This explains why the tax is paid, not that paying it is wrong.
- Distinct from F577: this makes the *trigger* fleet-invariant. F577 *skips the ladder entirely*, and
  F612 just showed that would have killed the genuine round too.

## Standing constraints for whoever lands these

- **`cargo` needs `. bin/activate-hermit` and cwd = repo root.**
- **Re-score after F574**; do not mix `scorer_version` sb-3 and sb-4 in one table.
- **Judge on mechanism, not on score.** F594 measured the outcome comparison as unaffordable — 102
  cells per arm for quality, 776 for speed, against a corpus of 39. The campaign's own charter
  already says this in three places (F596); the arithmetic just shows the charter was conservative.
- **The one finding that survived every test is F584/F600**: at one node the FLEET binds, at three
  the PLAN binds — 7/7 against 8/9 across five builds. It is a verdict per run, not a mean, which is
  why the replicate spread cannot touch it. Anything landed here should be judged the same way.

## Also queued, found while auditing (F605) — a coverage split, not a bug

**Only two of twelve instruments read `runs/nodeloop/_archive`**: `goal.py` (which writes it) and
`verdicts.py` (which resolves activity dirs out of it). The other ten glob the live cell directories
and therefore see **nine runs where twenty exist**.

**There are TWO event-log archives, and the on-landing chain protects the wrong one (F608).**
`tierlog.py` writes `nodeloop/eventlogs/`; `verdicts.py` reads ONLY `runs/nodeloop/_archive/logs`
(line 489), which `goal.py` writes. Running `tierlog.py` first to "protect the log" therefore does
nothing for the instrument that produces the headline — measured on `baseline-n3-r2` 2026-08-08,
where `verdicts.py` still showed only the two older runs of that cell name until `goal.py` ran.

> **On-landing chain: `tierlog.py` → `goal.py` → `spread.py` → `verdicts.py`.**

No instrument has a cell-name collision bug — every one globs a pattern yielding one file per cell
directory, where the name is unique. The split is a coverage gap nobody declared, not a defect.

**It predicts which of today's findings held.** F584 came from `verdicts.py` (archive-aware, 16
readings) and survived every re-test unchanged. F585 came from `planshape.py` (live dirs, 6 points)
and collapsed from −0.814 to −0.551 when re-run against the archive. F587 came from an ad-hoc
live-dir scan of four cells and strengthened at nine. That needs no mechanism — it is nine runs
against twenty.

**The change:** one shared `all_runs()` resolver over live dirs + `_archive`, keyed by
`(cell, epoch)` with a ±120s tolerance (result rows and event logs are written 4–5 seconds apart),
retrofitted into the ten. **Deliberately not written tonight** — `goal.py` and `verdicts.py` have
already rolled their own readers, so a third bespoke one is the duplication L355 exists to prevent,
and F598 caught me committing exactly that with the tierlog archive. Retrofitting ten instruments is
a deliberate change, not a late-evening one.

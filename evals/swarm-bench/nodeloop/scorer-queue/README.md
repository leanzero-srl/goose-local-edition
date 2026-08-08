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

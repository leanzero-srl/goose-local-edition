# DEFECT-BOARD — weighted remaining-loss readout

**Status: a standing benchmark option, not a report.** This file is the spec for an instrument the
campaign runs on every batch, plus the current board it produced. Sections 1-3 and 6 are the
instrument (stable). Sections 4-5 are the output of the last run and are expected to be overwritten.

Corpus of the current board: `/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop`,
engine_build `1786277705-235908576` (the binary at `target/release/goose`), **n=8** non-void
scored cells. Mean overall score 0.7559, total remaining loss 0.24411.

---

> ## ⚠️ MEASURED 2026-08-10: DO NOT WORK THIS LIST TOP-DOWN
>
> §3.7 already warns that the ranking *assumes* checks are independent loss. That assumption has now
> been **falsified empirically**, and the counterexample is the board's own rank 1 and rank 2.
>
> `scout_doc_urls-n3-r0` drove **`vendor_conditional` 0.33 → 1.00** and **`resync_conditional_ratio`
> 0.33 → 1.00** — precisely what the board recommends. In the same cell the entire sync family went
> to **0.00** (`sync_completeness`, `vendor_all_pages`, `payment_row_shape`, `total_field`,
> `summary_accuracy`, `concurrent_sync_safe`, `resync_idempotent`, `chronological_order`), **Tier B
> collapsed 0.936 → 0.361**, and the cell scored 0.7386 (−2.24 SD). It was also *faster* — 73 min
> against 94.9 — because it did less work.
>
> **The mechanism: the app implemented conditional requests so well it cached itself into emptiness.**
> Perfect 304 handling with `vendor_all_pages` at 0.00 means it negotiated freshness correctly and
> never fetched the payments at all.
>
> **The sync family is the PRECONDITION for conditional requests meaning anything.** Satisfying a
> downstream check by starving its upstream is a net loss the ranking cannot see, because weighted
> loss is computed per check with no edge between them.
>
> **So before acting on any row: name what must already work for that check to be meaningful, and
> confirm the fix cannot trade it away.** The ranking tells you where score is lost. It does not tell
> you that the loss is independently recoverable, and on this board it demonstrably is not. (n=1, and
> not noise-shaped: noise does not zero eight related checks while perfecting the two they depend on.)

## 1. The method

For every check `k` in a cell set, over the cells in one slice:

```
loss(k) = (1 - mean_over_cells(score(k))) * weight(tier(k)) / n_checks(tier(k))
```

| tier | weight | n_checks | per-check budget = weight/n_checks |
|---|---|---|---|
| A | 0.25 | 6 | 0.041667 |
| B | 0.30 | 12 | 0.025000 |
| C | 0.25 | 7 | 0.035714 |
| D | 0.20 | 10 | 0.020000 |
| **total** | **1.00** | **35** | — |

`weight` sums to 1.00 and each tier score is the unweighted mean of its checks, so
`overall = Σ_tier w·mean_tier` and therefore

```
Σ_k loss(k)  ==  1 - mean_over_cells(overall score)
```

**RECONSTRUCTION IS THE SELF-CHECK, AND IT RUNS EVERY TIME.** Compute both sides independently —
the left from the raw `checks[]` array, the right from the `score` field of each `verdict.json` —
and print the delta. Gate: **|delta| < 1e-4**. Anything larger means the tier membership, the check
counts (A6 B12 C7 D10 must be constant across cells) or the corpus filter is wrong, and no number
below it may be read. Observed deltas on the four slices run so far: 4.5e-06 (current 3-node, n=4),
1.0e-05 (current 1-node, n=3), 9e-06 (current all, n=8), 7e-06 (prior build, n=7). All are
attributable to the 4-decimal rounding of the tier means stored in `verdict.json`.

The reconstruction proves the arithmetic, and nothing else. It is silent on whether the checks are
independent (§3.7), on whether a zero means anything (§3.1), and on n (§3.5).

---

## 2. Why it beats the overall score

The overall score says a run lost 0.2441. It does not say of what. Weighted remaining loss splits
that number across the 35 checks with the tier weights applied, so it answers three questions the
score cannot:

1. **Which checks hold the loss.** On the current binary the top two checks hold 0.03982 =
   16.3% of all remaining loss; the top twelve hold roughly two thirds. "Improve quality" becomes a
   ranked list with a price on each line.
2. **What each item is worth if fixed.** A tier-A check at zero costs 0.0417; a tier-D check at
   zero costs 0.0200 — less than half. Two checks with the same mean are not the same prize, and
   the score alone cannot see that. It also bounds claims: **no tier-D check can ever be "one of
   the largest sources of loss" against a tier-A check at the same mean** — that is arithmetic,
   available before any measurement.
3. **What a fix is allowed to claim.** A proposed change gets a pre-stated target number
   (e.g. "merged vendor_conditional + resync_conditional_ratio below 0.020, from 0.03982") instead
   of "the score should go up".

What it does **not** do, and what §3 exists to contain: it assumes checks are independent loss
buckets. They are not. On the current binary 11 checks carry the byte-identical score vector
`[0,1,1,1,1,1,0,1]` — combined loss 0.08411, **34% of all remaining loss from one event** (two
cells where the server never bound). Ranked by check, that reads as eleven things to fix; ranked by
mechanism it is one. See §3.7.

---

## 3. Rules the instrument must obey

Each rule is followed by the one-line reason it exists. Seven of the eight were paid for by a
finding that had to be retracted.

**3.1 Positive control before any zero is interpreted.** Assert the check name is present in the
cells you globbed (`vendor_conditional` in 15/15, `chronological_order` in 118/118) *before* reading
a count — a zero from a blind query and a zero from a clean system are the same integer.

**3.2 Never pool across `engine_build`.** Group by `engine_build` and report each separately; the
prior build (1786178750, n=7) has `server_runs`, `health_shape` and `modules_present` at 1.000
across every cell while the current build has two cells that never bind — different failure mode,
different binary, and merging them invents a population that never ran.

**3.3 Exclude `void` cells, and say how many.** 103-104 of ~119 cells in this directory are void
(`F664: harness self-test FAILED`); a corpus reported as "119 cells" when 88% never measured
anything is a fabricated denominator.

**3.4 An empty or all-excluded result is "examined nothing", never "no problems".** All 104 cells of
build 1785868965-235742608 are void: the correct sentence is "I examined nothing on that build and
can make no claim, positive or negative", not "that build is clean".

**3.5 Report n on every line, and report what n cannot settle in the same sentence as the number.**
This campaign has already measured a **46-point spread on identical config** (commit 03ac84aa5), so
at n=8 a mean of 0.375 is 3 of 8 binary observations and the *order* of the top three is not
distinguishable — while the *presence* of an item in the top block may still be.

**3.6 Always carry the score vector; the mean alone is not a diagnosis.** `{0,0,0,1}` (intermittent
defect — the capability exists, reliability does not) and `{0.25,0.25,0.25,0.25}` (capability
ceiling — it never works) have the same mean 0.25 and need opposite fixes. Two real examples: a
0/1-only bimodality filter would have missed `store_atomic_upsert` entirely, whose poles are 0.5
(`INSERT OR REPLACE`) and 1.0 (`ON CONFLICT DO UPDATE`) with nothing between them, 5 cells vs 3;
and `summary_bounds_utc` has **three** levels in one check — 0.0 (endpoint absent), 0.7 (present but
local offsets `-05:00`, `+09:00`), 1.0 — whose 0-component and 0.7-component need different fixes.
Print `min / max / per-cell vector` beside every mean.

**3.7 Collapse collinear checks before ranking, and rank cells before checks.** Loss assumes
independence; identical score vectors are one event billed N times. Current binary: the 11-check
`[0,1,1,1,1,1,0,1]` cluster = 0.08411 (34%) from two never-booted cells; `vendor_retry_date` and
`vendor_cursor_expiry` are identical in 15/15 by construction. Per-*cell* concentration is the
honest headline: `baseline-n1-r0` carries 35.8% of all remaining loss and `doc_fetch-n3-r0` 32.3%
— 68.1% in two cells of eight; add `baseline-n3-r0` and three cells hold 76.9%.

**3.8 Classify every zero as DEFECT / DERIVATIVE / NON-OBSERVATION before it is ranked.** Seven of
the eight items adjudicated so far were refuted, and the dominant cause was ranking a zero the
scorer had already labelled `n/a`. Three distinct kinds:
- **DEFECT** — the behaviour was exercised and was wrong (`13 requests carried If-None-Match, 0
  answered 304`).
- **DERIVATIVE** — the app never booted or the sync yielded no rows, so the check is charging a
  failure already charged elsewhere (`n/a — masked by an earlier failure`).
- **NON-OBSERVATION** — the probe never fired (`never reached the date-form throttle`, `too few
  rows`, `no second-sync requests`). This is not evidence of anything, and scoring it 0.0 is the
  single mechanism that manufactured most of the refuted board.

---

## 4. Current board — engine_build 1786277705-235908576, n=8

Mean overall 0.7559; total remaining loss 0.24411; reconstruction delta 9e-06. Every mean is a mean
of 8 mostly-binary observations. Verdicts: **CONFIRMED** = survived a falsification attempt;
**REFUTED** = a pre-stated refutation criterion fired; **OPEN** = *no falsification attempt has been
run*, which is not evidence of validity.

| # | check | tier | mean (n=8) | weighted loss | share | verdict | disposition |
|---|---|---|---|---|---|---|---|
| 1 | vendor_conditional | C | 0.375 | 0.02232 | 9.1% | **CONFIRMED** | Fix §4.1. Ranking survives; the quoted *magnitude* does not. |
| 2 | resync_conditional_ratio | D | 0.125 | 0.01750 | 7.2% | REFUTED | Duplicate of #1 — merge, do not fix separately (§5). |
| 3 | serves_page | A | 0.625 | 0.01562 | 6.4% | OPEN | 2 of 3 zeros are the never-booted cells; conditional on booted cells mean 0.833, loss 0.00694. |
| 4 | summary_bounds_utc | B | 0.425 | 0.01437 | 5.9% | REFUTED | Named defect (local offsets) is worth 0.00188; rest is echo (§5). |
| 5 | vendor_retry_date | C | 0.625 | 0.01339 | 5.5% | REFUTED | Non-observation + duplicate of #6 (§5). |
| 6 | vendor_cursor_expiry | C | 0.625 | 0.01339 | 5.5% | REFUTED | Bench injector artefact (§5). |
| 7 | summary_accuracy | B | 0.500 | 0.01250 | 5.1% | REFUTED | Presence check wearing an accuracy name; real root cause found (§5). |
| 8 | server_runs | A | 0.750 | 0.01042 | 4.3% | OPEN | Head of the 11-check collinear cluster: combined 0.08411 = 34% from one event. |
| 9 | health_shape | A | 0.750 | 0.01042 | 4.3% | OPEN | Same cluster, same vector. |
| 10 | sync_shape | A | 0.750 | 0.01042 | 4.3% | OPEN | Same cluster. Tier-A budget makes it rank high on a 0.750 mean. |
| 11 | vendor_cursor_paging | C | 0.750 | 0.00893 | 3.7% | OPEN | Identical failing cells to #12 on this build (8/8). |
| 12 | vendor_all_pages | C | 0.750 | 0.00893 | 3.7% | REFUTED | Claim came from a superseded build (§5). |
| 15 | chronological_order | B | 0.7448 | 0.00638 | 2.6% | REFUTED | Every zero is `too few rows` (§5). |
| — | store_atomic_upsert | D | 0.688 | 0.00625 | 2.6% | OPEN | Vector `[0.5,0.5,1,1,0.5,0.5,1,0.5]` — two discrete idioms, no partials; boot- and node-independent. Fix is reproducibility (name the idiom in the spec), not capability. |
| — | ui_error_actionable | D | 0.775 | 0.00450 | 1.8% | OPEN | Vector `[1,1,0.4,0.4,1,1,0.4,1]`, low pole 0.4 = generic error text. Boot-independent. |
| — | ui_polish | D | ~0.85 | 0.00400 | 1.6% | OPEN | Mostly a graded 4/5-vs-5/5 ceiling (recurring miss: retry affordance / disables while syncing) with one 0 borrowed from the n3-r0 cluster. |
| — | ui_currency | B | 0.875 | 0.00312 | 1.3% | OPEN | Single-cell (`baseline-n3-r0`); part of the broken-UI cluster with #3, ui_states, ui_polish. n=1 failure — no rate claim. |

Ranks 13-14 and 16-35 omitted; ranks 1-12 are exhaustive and taken from the full ranked table.

### 4.1 Proposed fix — the one survivor

**vendor_conditional (with resync_conditional_ratio merged: 0.03982, 16.3% of remaining, n=8).**

Mechanism, settled deterministically and independent of n: the app stores **one** ETag scalar,
overwrites it on every response, and replays it on the *next* request, so it is always exactly one
page behind. `bench/vendor_service.py:140` makes the ETag a pure function of `(offset, limit)` —
`sha256(f"meridian-{offset}-{limit}")[:16]`, content-independent — so this cannot be a flaky mock.
Recomputing the expected ETag per request from `vendor-trace.jsonl`: `baseline-n3-r0` 13/13
mismatched, `baseline-n1-r1` 16/16, `baseline-n3-r1` 13/13; every mismatch equals the immediately
preceding request's tag.

Three engine-side edits, all in `crates/goose-cli/src/commands/swarm.rs`:

1. **`swarm.rs:18011` — fix the remediation string.** It currently reads "Store the vendor's ETag
   and send If-None-Match on the **next** fetch", which followed literally *produces the measured
   defect*. Replace with wording that names the key: key each page's ETag by the exact request that
   produced it (path + offset + limit) and send `If-None-Match` only when re-issuing that same
   request. Highest confidence of the three — it is a plain contradiction between the advice and
   the measurement.
2. **`repeated_post_verdict` (`swarm.rs:3566`) — add the cheapness clause.** It returns `Idempotent`
   whenever `inserted==0` and `total` is flat, which is exactly the failing app's signature. Decide
   from the documented `fetched` field: both bodies carry it, `first.fetched > 0`,
   `second.fetched >= first.fetched`, `second.inserted == 0` → `NotCheap`. **Fail open to
   `Unreadable` whenever `fetched` is absent** — `sync_shape` means only 0.750 on this build, so
   absence is a live risk, and a false finding against a freshly built app is the most expensive
   mistake this gate can make.
3. **Run an arm with `GOOSE_SWARM_PROBE_ADVERTISED_POST=1`.** The probe exists and is wired
   (`swarm.rs:17975-18020`) but the gate is unset in **15/15** scored cells and no `run.jsonl`
   anywhere contains `probed_post` — edits 1 and 2 are dead code until an arm runs with it on.
   Keep the default OFF (it is the first *write* the gate would issue).

Not proposed: an AST scan for ETag keying — the body-only test is decidable and cheap; a pattern
list is not.

**Magnitude correction that must travel with this item.** The `0.25 / 0.02679 / "44% of ALL
remaining weighted score loss"` now baked into the doc comment at `swarm.rs:3547-3548` and a test
name at `:7703` is reproducible **only** over the four best-scoring 3-node cells. Picking the best
cells removes the loss everywhere else; on the full n=8 corpus the honest figures are
**0.375 / 0.02232 / 9.1%**, or 0.03982 / 16.3% merged. Those three constants should be corrected in
source before anything is built on "44%".

---

## 5. Refuted items — kept visible

A board that lists only survivors hides the fact that someone checked. 7 of 8 adjudicated items were
refuted; **none was refuted for small n**, which is not a refutation criterion.

| check | claimed | claim's provenance | measured on current binary (n=8) | why refuted |
|---|---|---|---|---|
| resync_conditional_ratio | mean 0, loss 0.0200 | 0.20/10 is exactly the **tier-D ceiling** — the number you get by assuming mean 0 rather than measuring it | 0.125 / 0.01750 (rank 2) | Reproduces in no partition. Duplicate of vendor_conditional — same ETag mechanism at two strictnesses, sign agreement 13/15. Pre-registered as non-reproducing in F409/F402 ("a property of the RUN, not the code"). 2 of 8 zeros read `no second-sync requests` with consequence `n/a` — the app crashed before the second sync. Structural cap: a tier-D check at literal zero loses less than any tier-A/B/C check at zero. |
| vendor_retry_date | 0.3333 / 0.02381 | one n=3 slice (baseline-n1-r0/r1/r2, scores 0,0,1) | 0.625 / 0.01339 (rank 5); prior build 0.7143 / 0.01020 (rank 14) | Every zero reads `never reached the date-form throttle`, consequence `n/a` — NON-OBSERVATION. When it fires it passes 10/10. Strictly downstream of vendor_cursor_expiry by construction (`vendor_service.py:214-221`: the date-form 429 is gated on the 410 having fired), identical in 15/15. The probe file's own docstring forbids this: "if the harm cannot be named, the check does not belong here." |
| vendor_cursor_expiry | 0.3333 / 0.02381 | same n=3 slice | 0.625 / 0.01339 (rank 6) | Bench artefact. `vendor_service.py:217` keys the one-shot 410 to a **fixed global request ordinal** (`nth == CURSOR_EXPIRES_NTH(3) and bool(cursor)`), so any app that opens with a cheap count probe shifts every index and the fault is never injected for the whole run. Confirmed 15/15: the 410 fires iff `first_limit == 100`. Of 15 cells, the 11 actually exercised all scored 1.0 (`restarted from page 0: True`). True loss attributable to cursor-expiry behaviour: zero. |
| vendor_all_pages | 0.4545 / 0.01948 | reproduces exactly and only on the pre-batch SNAPSHOT, build 1786178750, n=11 (5/11) — a superseded engine | 0.750 / 0.00893 (**rank 12**) | Wrong build; not near the top; duplicate of vendor_cursor_paging on this build (identical mean, identical failing cells, 8/8). Both current failures are the two worst cells overall (0.301 and 0.369) — one made a single vendor call, the other 8 all at offset 0. Not a pagination defect; there is no addressable target. |
| summary_bounds_utc | 0.2818 / 0.01795 | 3.10/11 exactly — the 11 snapshot cells of build 1786178750 | 0.425 / 0.01437 (rank 4) | Detail contradicts the name: 9 of 15 cells read `oldest=None newest=None`, no timezone involved. Decomposed on the current binary: 43% dead-app cells, 43% summary endpoint returns nothing, **13% the named UTC defect** = 0.00188 weighted (~0.8% of remaining). On the claim's own build, 94% of the loss is cells with an empty payments table where `None` is the correct answer. And `summary_bounds_utc > 0 ⟺ summary_accuracy == 1.0` in 15/15 — one event, two prizes. |
| summary_accuracy | 0.3333 / 0.01667 | only by pooling build 1786178750's 7 top-level cells with its 11 snapshot cells (n=18) | 0.500 / 0.01250 (rank 7) | On that pooled corpus its vector is **byte-identical** to total_field, sync_completeness, resync_idempotent, payment_row_shape and chronological_order — six checks, 0.10 of score, one upstream cause. Every failure anywhere reads `total_minor=None` (missing, never wrong); when present it is exactly 4409197, 4/4. Accuracy has never once failed. 2 of 4 current zeros are cells where `server_runs=0`. |
| chronological_order | 0.3636 / 0.01591 | pre-batch SNAPSHOT, build 1786178750, n=11 (4 pass / 7 `too few rows`) | 0.7448 / 0.00638 (**rank 15/35**) | `score_build.py:281-282` returns `g(0.0, "too few rows", "n/a")` when `len(data) < 2` — the scorer's own not-applicable branch scored as a fail. `chronological_order == 0` is a strict subset of `sync_completeness == 0` in 15/15, zero counterexamples. The only genuine ordering deficit in either corpus is `23/24 adjacent pairs ordered` in one cell = **0.00013** weighted. 98-100% of this check's loss is sync-emptiness wearing an ordering label. |

### 5.1 What the refutations converge on

Two mechanisms produced almost all seven.

**Bench-side: not-applicable scored as failure.** The same branch recurs in at least seven places —
`check_httpdate_retry_after` and `check_cursor_expiry_recovery` and `check_retry_after_honoured`
(`bench/probes/vendor_trace.py`), `chronological_order` "too few rows" and `payment_row_shape`
"no rows" and `resync_conditional_ratio` "no second-sync requests" and `request_efficiency`
"no vendor requests observed" and `concurrent_sync_safe` "concurrent sync not observed"
(`bench/score_build.py`). Fix all of them in one pass — return a NA sentinel excluded from **both**
the numerator and that cell's tier denominator — or the phantom returns next batch. Fixing one
instance schedules the class's return.

**Engine-side: the contract gate cannot see the endpoint that is actually broken.**
`run_spec_contract()` issues only bare GETs, curls them with `-o /dev/null` (status only, body
discarded), and runs the GET loop **before** any POST block, against a virgin scratch DB. Three
consequences, all measured: an advertised GET that answers 200 with a hollow body increments
`verified` and produces no finding; a handler that only breaks once rows exist (a store/api row-key
disagreement → `KeyError` → 500 → `total_minor=None` *and* `oldest=None newest=None`) is certified
green; and every requirement behind `POST /api/sync` — "a second sync must be cheap and must not
duplicate rows" — has never once been visible to the repair loop. All 8 current-build `run.jsonl`
emit "advertised endpoint(s) were NOT probed because this check only issues bare GETs" 2-6 times
each and zero idempotency findings. This is the shared root of the *real* defects behind four of
the refuted items, and it is why the survivor's fix (§4.1) is a probe, not a prompt.

---

## 6. How to run it

Reimplementation, not a script path — the working scripts live in a session scratchpad and will not
survive.

```
1. glob  runs/nodeloop/*/verdict.json        (depth 1; and the sibling nodeloop-result.json)
2. ASSERT len(files) > 0 AND at least one populated checks[]   <- positive control, before any count
3. ASSERT the check under discussion is present in ~all cells  <- §3.1, on the same object
4. engine_build key = f"{int(mtime)}-{size}" of target/release/goose  (exact match on BOTH parts;
   e.g. mtime 1786277705.51, size 235908576 -> "1786277705-235908576")
5. drop void==true and score==null; RECORD how many of each were dropped   <- §3.3
6. GROUP BY engine_build; never merge groups                              <- §3.2
7. per group: per-check mean + full per-cell score vector + min/max        <- §3.6
8. loss(k) = (1-mean)*w(tier)/n_checks(tier); assert A6 B12 C7 D10 constant across cells
9. SELF-CHECK: |Σ loss - (1 - mean(score))| < 1e-4, printed                <- §1
10. group checks by identical score vector; report clusters as one line    <- §3.7
11. also report per-CELL loss share, sorted                                <- §3.7
```

Report per group: `n`, void count, mean score, total loss, reconstruction delta, the ranked table,
the collinear clusters, and the per-cell concentration.

## 6.1 How to read a result

1. **Reconstruction delta first.** Over 1e-4 → stop; nothing below it is readable.
2. **n and the void count next.** n<8 settles no score claim in this campaign; 46 points of spread
   on identical config is the calibration.
3. **Per-cell concentration before per-check ranking.** If two cells hold ~68% of the loss, the
   readout is describing a crash, not a residual — exclude the falsified arm or apply a per-cell
   floor before reading the check ranking as a remediation list. (Measured: including
   `doc_fetch-n3-r0` at n=5, top-12 overlap with the n=4 healthy ranking is **5/12**, and seven
   checks with *literally zero* loss in every working cell enter the top 12.)
4. **Then the ranking, cluster by cluster.** Collapse identical vectors to one line item.
5. **Classify each survivor's zeros** as DEFECT / DERIVATIVE / NON-OBSERVATION (§3.8) and read the
   `detail` strings. If the detail says `n/a` or names a different subsystem than the check name,
   it is not a defect in that check.
6. **Read the vector, not the mean** (§3.6): `{0,0,0,1}` is a reliability problem (the capability
   exists — ship reproducibility), `{0.25×4}` is a ceiling (the capability does not exist — ship
   capability), and a 0.5/1.0 split is an idiom coin-flip.
7. **Zero loss is not a pass.** A check at loss 0.000 across every cell may simply never have been
   probed — `vendor_retry_date` scores 1.0 on 10/10 firings and 0.0 on 5 non-firings, and the
   POST-idempotency requirement has never been evaluated at all. §3.4 applies to checks as well as
   to corpora.
8. **A survivor must arrive with a pre-registered falsifier** and a readout that can settle it, or
   it is a hypothesis, not a board item.

---

## 7. Standing context for the next run

- **Matched node curve, 3 pairs, one binary, baseline only:** 3-node 0.9124 vs 1-node 0.6856;
  94.9 vs 112.6 min (15.7% faster); Tier B 0.9359 vs 0.6713. All four GOAL.md targets pass on point
  estimates, but **t=1.49 on df=2 is not significant** and each headline rests on a single pair —
  dropping r0 collapses quality to +0.0760, dropping r1 inverts the speed result. `baseline-n1-r0`
  scored 0.3006 with `server_runs=0`; 15 of 35 checks are lost only in that cell and every one of
  them reads as "the fleet fixes it" while being nothing of the kind.
- **doc_fetch scored 0.369 (-7.05 SD) and its pre-registered falsifier fired** — `server_runs`
  went 1.00 → 0.00 and seven checks unrelated to sync regressed. 4789 bytes in *every worker prompt*
  crowds out the task. `scout_doc_urls`, which instead tells scouts to fetch, scored 0.8843
  (-0.36 SD, inside noise). **The delivery mechanism is the defect, not the document.** The
  doc_fetch cell must be excluded from any remediation ranking (§6.1 step 3).
- **`run_spec_contract()` issues only bare GETs and does not probe advertised POST endpoints.**
- **Untested exclusions:** no current-build cell was void and none had a null score, so those two
  filters removed nothing on this slice. They are not verified by this run — they had no work to do.
- **Not examined on the current build:** the 104 void cells of build 1785868965-235742608 (examined
  nothing, no claim either way) and the pre-batch SNAPSHOT corpus, which contains **zero** cells on
  the current build and is therefore disjoint from it.

---

## 8. F751 — COMPLETE ships GREEN at round 0 on the worst apps of the build

The COMPLETE phase exists to "refuse to ship a red app". On the current binary it did the opposite:
the two lowest-scoring cells are exactly the two whose verifier found **nothing** and therefore never
dispatched a repair.

    engine_build 1786340680-235925264, every non-void cell
      baseline-n1-r0        n=1  0.9283   round-0 findings 1   fixes 1   verify rounds 2
      probe_post-n3-r0      n=3  0.8986   round-0 findings 1   fixes 6   verify rounds 3
      scout_doc_urls-n3-r0  n=3  0.7386   round-0 findings 0   fixes 0   verify rounds 1
      baseline-n3-r0        n=3  0.7226   round-0 findings 0   fixes 0   verify rounds 1

`baseline-n3-r0`'s own stderr reads *"complete: GREEN at round 0 — the built app runs and its checks
pass"* about an app the scorer puts at 0.7226, and it emitted the same
`complete_result{passed:true, verified:false, remaining_findings:0}` as the 0.9283 cell.

**The repair loop is NOT broken — detection is.** A census over every readable cell on all three
builds found **zero** cells with round-0 findings > 0 and no fix dispatched. Repair always runs when
it is given a reason; on these two cells it was never given one.

The verifier names its own hole in `inconclusive_reasons`, on every cell of every build:

> spec-contract: probed 2 advertised GET endpoint(s); 1 advertised endpoint(s) were NOT probed
> because this check only issues bare GETs: **POST /api/sync**.

That is the Tier B surface — 52% of all score lost in this campaign. The checker announces it cannot
see the endpoint that carries the majority of the defect, and then round-0 GREEN ships the app.

**The one arm that closes the hole is the one arm where 3-node repair engaged.** `probe_post` probes
advertised POST endpoints; it is the only 3-node cell on this build with a round-0 finding, it drove
**6** fix dispatches over 3 verify rounds, and it is the best 3-node score on the build (0.8986). At
n=1 that is a MECHANISM readout — the gate fired and drove repair — not a score claim.

### What this does and does not license

- **Mechanism, valid at n=1:** the round-0 verdict does not track app quality, and a probe that
  reaches POST turns a silent GREEN into six repair dispatches.
- **NOT licensed:** "no repair ⇒ low score". Build 1786277705 contains a green-no-repair cell that
  scored **0.926**. The rule fails there and is not claimed.
- **Repair-engagement rate by node count** (suggestive only, all three builds): 1-node **5/5**,
  3-node **5/9**. Fisher one-tailed p = 0.126 — not significant, recorded so it is not rediscovered
  as news.

### Falsifier, pre-registered

If the next 3-node baseline cells continue to reach round-0 GREEN with zero findings while scoring
below their 1-node partners, detection is the binding constraint and `probed_post` moves ahead of
every remaining tuning arm. **REFUTED if** a 3-node cell scores at or below 0.75 *with* round-0
findings > 0 and a dispatched repair — that would put the loss after detection, not in it.

Instrument: `repaircensus.py`. Its control asserts one cell that repaired (`baseline-n1-r0`, 1
finding / 1 fix) and one that did not (`baseline-n3-r0`, 0/0) before any aggregate prints, so a
parser that sees no repairs and a parser that sees repairs everywhere both hard-fail.

### F751 REFUTED — by its own falsifier, 25 minutes after it was written

Registered: *"REFUTED if a 3-node cell scores at or below 0.75 with round-0 findings > 0 and a
dispatched repair — that would put the loss after detection, not in it."*

`baseline-n3-r1` landed at **0.7110** with **round-0 findings 1, six fix dispatches across three
verify rounds**, `complete_result{passed:false, remaining_findings:1}`. Detection worked. Repair
engaged harder than on any cell except `probe_post`. COMPLETE correctly refused to call it green.
The app is still the worst 3-node score on the build.

**So detection is NOT the binding constraint.** What survives of F751 is only the narrower fact it
was built on: `baseline-n3-r0` and `scout_doc_urls-n3-r0` DID reach round-0 GREEN with zero findings
on 0.72 and 0.74 apps, and that remains a real false green in a phase whose stated job is to refuse
one. What is dead is the causal reading — that the blindness explains the node curve — and with it
the elevation of `probed_post` to "lead candidate". Six fix dispatches produced 0.8986 on
`probe_post` and 0.7110 here; fix count does not predict score.

The `probed_post` emission shipped anyway, on its own merit: the code comment claimed the field was
"emitted either way" and no such field existed.

### What the r1 cell DID establish: the gap is one tier, not a diffuse quality loss

    cell                    score   A       B       C       D      wall
    baseline-n1-r0         0.9283  1.000  1.0000  0.857  0.820   73 min
    probe_post-n3-r0       0.8986  1.000  1.0000  0.714  0.850  110 min
    scout_doc_urls-n3-r0   0.7386  1.000  0.3611  0.857  0.750   73 min
    baseline-n3-r0         0.7226  1.000  0.3611  0.857  0.750   75 min
    baseline-n3-r1         0.7110  0.833  0.3611  0.857  0.900  119 min

A, C and D are comparable across all five cells. **Tier B alone separates them**, and on this build
it takes exactly two values — 0.3611 and 1.0000. At weight 0.30 that single tier is ~0.19 of score,
which is the entire observed gap.

⚠️ **NOT a claim that Tier B is all-or-nothing.** The wider corpus refutes that directly: build
1786178750 shows 0.2222 / 0.3611 / 0.975 / 1.0 and build 1786277705 shows 0.25 / 0.8333 / 0.9306 /
0.975. Intermediate values exist. The clustering is a property of these five cells, checked rather
than assumed, and stated as such.

Also recorded: 3-node wall time is 75 and 119 minutes on identical config. Any speed claim from a
single pair is noise.

---

## 9. F752 — Tier B is not twelve checks. It is one defect counted eight times.

The per-check readout of the two Tier-B values, all five current-build cells:

    check                     n1-r0  probe_post  scout_doc  n3-r0  n3-r1
    sync_completeness          1.00     1.00      0.00      0.00   0.00
    resync_idempotent          1.00     1.00      0.00      0.00   0.00
    local_pagination           1.00     1.00      0.33      0.33   0.33
    payment_row_shape          1.00     1.00      0.00      0.00   0.00
    total_field                1.00     1.00      0.00      0.00   0.00
    chronological_order        1.00     1.00      0.00      0.00   0.00
    summary_accuracy           1.00     1.00      0.00      0.00   0.00
    summary_bounds_utc         1.00     1.00      0.00      0.00   0.00
    input_validation           1.00     1.00      1.00      1.00   1.00
    ui_states                  1.00     1.00      1.00      1.00   1.00
    ui_currency                1.00     1.00      1.00      1.00   1.00
    ui_offline                 1.00     1.00      1.00      1.00   1.00

Their own detail strings settle what is happening:

    sync_completeness    0/247 payments after one sync
    payment_row_shape    no rows
    total_field          total=0
    chronological_order  too few rows
    summary_accuracy     total_minor=0 (want 4409197)
    summary_bounds_utc   oldest=None newest=None
    resync_idempotent    second sync inserted=0 total=0

**Seven of those are the same sentence in seven grammars: there is no data.** The sync returns
nothing, and every check that needs a row to look at reports zero. This is the isolation failure the
loop doctrine names outright — *"each check must fail on its own defect and nothing else; shared
setup that can fail will collapse a mostly-correct result to zero"* — and it is in MY scorer, not in
the engine.

### The correction, stated as loudly as the original claim

Every "Tier B is 52% of all score lost" line in this campaign should be read as **"the sync returning
zero rows is 52% of all score lost."** The tier is not a broad vendor-contract weakness across twelve
independent behaviours. It is one upstream failure with a wide blast radius inside the instrument:
one defect costs 8/12 × 0.30 ≈ 0.20 of score, while a genuinely independent Tier-B defect costs
1/12 × 0.30 ≈ 0.025 — an 8× weighting nobody chose.

The four checks that hold at 1.00 in every cell (`input_validation`, `ui_states`, `ui_currency`,
`ui_offline`) are exactly the ones that need no synced data. They are not evidence the apps are
partly fine; they are evidence of where the gate sits.

### What this does NOT change

- The apps really are broken. `0/247 payments` is a genuine, total failure of the headline feature,
  not a scoring artefact. The defect is real; only its WEIGHT was inflated.
- Cross-cell comparisons stay valid — every cell is scored by the same instrument, so the ranking of
  0.9283 / 0.8986 / 0.7386 / 0.7226 / 0.7110 is unaffected.
- It does not explain the node curve either. Both 1.0000 cells and all three 0.3611 cells span node
  counts.

### The engine defect this exposed, fixed in `e958c9d2d`

`repeated_post_verdict` walked that exact signature — `inserted:0, total:0, fetched:0` twice — and
returned **`Idempotent`**, which increments `verified`, the counter documented to exist so a consumer
can tell a real pass from having checked nothing. An app that synced zero rows was being
affirmatively verified as idempotent by the one gate built to catch the sync contract. New
`RepeatedPost::Vacuous` arm routes it to `inconclusive` — never a finding, because a vendor with no
rows is a legitimate empty sync, and never `verified`. The test was watched FAILING with the guard
disabled before being trusted green.

### Owed to the scorer, pre-registered

Tier B must stop paying eight times for one gate. The fix is to make the seven data-dependent checks
report **unscored** rather than 0.00 when `sync_completeness` is 0, and to let `sync_completeness`
carry the weight it actually represents. That is a scorer-version bump (`sb-3` → `sb-4`) and it
**re-scores the whole corpus**, so it happens at a boundary, with the old scores kept beside the new
ones and no cross-version comparison. Until then, every Tier B number in this document is a number
about one defect.

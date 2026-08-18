# Where sb-5.3 stops discriminating — the data case

Sources read (all read-only): `bench/score_build.py` (full 60-check registry + weights), 19,509 `verdict.json` files under `evals/swarm-bench/runs/**` (nodeloop + all parked/superseded snapshots), `nodeloop/bridge-ledger.jsonl` (37 rows), `runs/build/*` cloud-entrant verdicts, `runs/calibration/calibration-opus-5.json`, `bench/fixtures.py`, `bench/vendor_service.py`, and the 13 `graded.db` files under `~/Library/Application Support/Goose/benchmark/runs/build/` — the latter turned out to be the **apps' own payment stores** (tables `payments`, `metadata`), not grading data, so they contribute nothing here. `calibration-opus-5.json` is the legacy 14-check/47-payment format and was excluded as incomparable.

## 0. Corpus construction (why the counts are what they are)

- 19,509 verdict files → 309 unique verdict contents scored sb-4/sb-5.x → **214 with a live tree** (95 are rescores against pruned trees: `server_runs: "never bound (No module named vendorsync)"`, all-zero — harness artifacts, not entrant evidence; e.g. `runs/nodeloop/baseline-n2-r5/verdict.json`).
- Deduping by (cell-family, score-vector): **114 of the 214 are the same 0.955 known-good tree** rescored in the `amend_feature` loop (`nodeloop-parked-1786911784/_superseded/amend_feature-n3-r0@…`, one identical score-vector hash across all 114). Final corpus: **93 unique builds** (54 sb-4, 24 sb-5.2, 11 sb-5, 4 sb-5.1), spanning 0.0167–0.9949.
- Segments: **serious** = score ≥ 0.75 → **n=31**. **top** = score ≥ 0.90 → **n=11** (incl. Opus-5 0.9755 and Sonnet-5 0.9692 under sb-5; Haiku-4.5 scored 0.7861). J/P/V rows exist only for the 39 sb-5.x builds (11 serious).
- Score distribution of the 93: 23 dead (<0.31), 39 mid (0.40–0.75), 19 solid (0.75–0.90), 11 top (≥0.90). The mid-band resolves well; the problem is strictly the top.

## 1. Per-check saturation table

`wS5` = effective weight of the check in the sb-5 final score (tier weight ÷ non-hard tier members, core×0.60, HARD 0.10/6). `pass` = fraction scoring exactly 1.0. `S` = serious cohort (n=31; J/P/V n=11), `T` = top cohort (n=11; J/P/V n=2 cloud). `dv` = distinct values observed among serious. **Bold = zero discrimination among serious (passS = 100%).**

| check | T | wS5 | mean(all) | passS% | sdS | dv | meanT | passT% |
|---|---|---|---|---|---|---|---|---|
| **modules_present** | A | .0250 | .806 | 100 | .000 | 1 | 1.000 | 100 |
| **interfaces_declared** | A | .0250 | .810 | 100 | .000 | 1 | 1.000 | 100 |
| **server_runs** | A | .0250 | .753 | 100 | .000 | 1 | 1.000 | 100 |
| serves_page | A | .0250 | .688 | 87 | .335 | 2 | 1.000 | 100 |
| **health_shape** | A | .0250 | .753 | 100 | .000 | 1 | 1.000 | 100 |
| sync_shape | A | .0250 | .484 | 84 | .368 | 2 | 1.000 | 100 |
| sync_completeness | B | .0112 | .272 | 58 | .417 | 4 | 1.000 | 100 |
| resync_idempotent | B | .0112 | .247 | 55 | .465 | 3 | 1.000 | 100 |
| local_pagination | B | .0112 | .473 | 90 | .235 | 3 | 1.000 | 100 |
| payment_row_shape | B | .0112 | .353 | 87 | .296 | 3 | 1.000 | 100 |
| total_field | B | .0112 | .258 | 71 | .454 | 2 | 1.000 | 100 |
| chronological_order | B | .0112 | .354 | 87 | .295 | 3 | 1.000 | 100 |
| summary_accuracy | B | .0112 | .226 | 61 | .487 | 2 | .909 | 91 |
| summary_bounds_utc | B | .0112 | .311 | 58 | .382 | 3 | .827 | 64 |
| input_validation | B | .0112 | .739 | 97 | .025 | 2 | 1.000 | 100 |
| ui_states | B | .0112 | .703 | 87 | .239 | 4 | 1.000 | 100 |
| ui_currency | B | .0112 | .624 | 81 | .316 | 3 | .864 | 73 |
| **ui_offline** | B | .0112 | .806 | 100 | .000 | 1 | 1.000 | 100 |
| row_integrity | B | .0112 | .336 | 65 | .328 | 4 | 1.000 | 100 |
| chronological_order_full | B | .0112 | .353 | 84 | .334 | 3 | 1.000 | 100 |
| json_everywhere | B | .0112 | .727 | 90 | .050 | 2 | .985 | 91 |
| health_semantics | B | .0112 | .591 | 67 | .137 | 3 | 1.000 | 100 |
| vendor_read_docs | C | .0150 | .957 | 94 | .246 | 2 | .818 | 82 |
| **vendor_cursor_paging** | C | .0150 | .656 | 100 | .000 | 1 | 1.000 | 100 |
| vendor_all_pages | C | .0150 | .645 | 97 | .177 | 2 | 1.000 | 100 |
| vendor_retry_secs | C | .0150 | .699 | 97 | .177 | 2 | 1.000 | 100 |
| vendor_retry_date | C | .0150 | .559 | 87 | .335 | 2 | 1.000 | 100 |
| vendor_cursor_expiry | C | .0150 | .634 | 97 | .177 | 2 | 1.000 | 100 |
| client_all_payments | C | .0150 | .226 | 58 | .493 | 2 | 1.000 | 100 |
| client_total_count | C | .0150 | .613 | 90 | .296 | 2 | 1.000 | 100 |
| client_true_order | C | .0150 | .226 | 58 | .493 | 2 | 1.000 | 100 |
| client_create_replay | C/H | .0167 | .624 | 97 | .177 | 2 | 1.000 | 100 |
| **client_idempotency_key** | C/H | .0167 | .742 | 100 | .000 | 1 | 1.000 | 100 |
| client_integer_amounts | C | .0150 | .344 | 84 | .368 | 2 | 1.000 | 100 |
| update_propagation | C/H | .0167 | .482 | 87 | .335 | 2 | 1.000 | 100 |
| restart_persistence | C/H | .0167 | .548 | 97 | .177 | 2 | 1.000 | 100 |
| second_sync_cost | C/H | .0167 | .254 | 32 | .464 | 3 | .545 | 55 |
| request_efficiency | D/H | .0167 | .742 | 94 | .131 | 2 | .952 | 91 |
| uses_max_limit | D | .0150 | .600 | 87 | .302 | 4 | .911 | 91 |
| concurrent_sync_safe | D | .0150 | .301 | 71 | .454 | 2 | 1.000 | 100 |
| **api_content_type** | D | .0150 | .720 | 100 | .000 | 1 | 1.000 | 100 |
| ui_polish | D | .0150 | .667 | 71 | .294 | 3 | .945 | 73 |
| store_atomic_upsert | D | .0150 | .487 | 32 | .241 | 3 | .773 | 55 |
| **store_indexed** | D | .0150 | .806 | 100 | .000 | 1 | 1.000 | 100 |
| client_timeouts | D | .0150 | .688 | 81 | .395 | 2 | .636 | 64 |
| ui_error_actionable | D | .0150 | .671 | 87 | .243 | 3 | 1.000 | 100 |
| j_loads_data | J | .0300 | .218 | 55 | .386 | 3 | 1.000 | 100 |
| j_console_clean | J | .0300 | .846 | 82 | .386 | 2 | 1.000 | 100 |
| j_sync_journey | J | .0300 | .301 | **9** | .269 | 5 | .750 | **0** |
| j_error_state | J | .0300 | .436 | 82 | .386 | 2 | 1.000 | 100 |
| **j_empty_state** | J | .0300 | .487 | 100 | .000 | 1 | 1.000 | 100 |
| **p_list_latency** | P | .0167 | .590 | 100 | .000 | 1 | 1.000 | 100 |
| p_page_interactive | P | .0167 | .231 | 73 | .445 | 2 | 1.000 | 100 |
| **p_sync_wall** | P | .0167 | .667 | 100 | .000 | 1 | 1.000 | 100 |
| v_dates_readable | V | .0167 | .231 | 73 | .445 | 2 | 1.000 | 100 |
| v_status_distinct | V | .0167 | .115 | **9** | .308 | 3 | .500 | **0** |
| v_pagination | V | .0167 | .400 | 73 | .305 | 3 | 1.000 | 100 |
| v_filter | V | .0167 | .462 | 91 | .287 | 2 | 1.000 | 100 |
| **v_responsive_375** | V | .0167 | .615 | 100 | .000 | 1 | 1.000 | 100 |
| v_styling | V | .0167 | .515 | 82 | .116 | 2 | .945 | 100 |

Headline saturation counts:
- **25/60 checks pass for ≥90% of serious entrants — 42.8 pts of the 100-pt sb-5 weight is guaranteed by "kinda works".** At the ≥80% threshold: 41/60 checks, **70.3 pts**.
- **32/60 checks produced ≤2 distinct values across all 93 builds** — effectively binary in practice, including every vendor_* check, every client_* check, all three P checks, and even the nominally-continuous `summary_accuracy` (its `1−err×10` partial band **never fired once** in 93 builds: every miss was >10% off).
- **44/60 checks sit at meanT = 1.000** for the top cohort — zero resolution above 0.90.
- 15/60 checks contribute < 0.10 pts of weighted separation (w × sdS) among serious entrants.

## 2. Where the top compression mathematically comes from

**(a) Weight dilution.** In sb-5 a B check is worth 0.60×0.30/16 = **0.01125** of the final score; a J check is 0.15/5 = **0.030** (2.7× more). The 16-member B tier means even a full binary miss moves the needle 1.1 pts. Conversely tier A (6 easy checks × 0.025 = 15 pts) is 100%-passed by every serious build — pure pedestal.

**(b) The top's entire loss budget is 5 checks, and 4 of them are broken or noise.** Mean total loss per sb-5 top build is **2.76 pts**, distributed:

| check | pts/run | share of top loss | nature |
|---|---|---|---|
| v_status_distinct | 0.833 | 30.1% | **fixture ceiling** (see below) |
| client_timeouts | 0.750 | 27.1% | source grep `timeout\s*=` |
| j_sync_journey | 0.750 | 27.1% | **probe artifact** (see below) |
| ui_currency | 0.281 | 10.2% | source grep `Intl.NumberFormat` |
| ui_polish | 0.150 | 5.4% | 5 source greps |

**(c) Opus vs Sonnet: 57/60 identical checks.** The full 0.63-pt gap is `client_timeouts` (opus 1.0/sonnet 0.0 — Sonnet wrote no literal `timeout=`), `ui_currency` (opus 0.5/sonnet 1.0 — manual vs Intl formatting), `ui_polish` (0.8 vs 1.0). **The leaderboard order above 0.95 is decided by regex greps of the source, not by anything either app does.**

**(d) v_status_distinct has a structural ceiling of 0.5.** `vendor_service.py:91` sets **every** payment `"status": "settled"`; only `mutate_statuses()` flips 25 rows to "refunded" post-sync. Result across 22 scoring sb-5 builds ≥0.5: 19 saw only one status on page 1 ("only 'SETTLED' … badge-styled" → 0.5 cap — both cloud models score exactly this), 1 build (0.7690) got 1.0 by ordering luck (a refunded row happened to land on its page 1). A perfect frontend cannot score 1.0 here; the check currently punishes the fixture, and it is 30% of all top-band headroom.

**(e) j_sync_journey's `view_refreshed` part fails for 16 of 17 builds whose button was found — including BOTH Opus and Sonnet** (each 0.75: "button_found, disabled_while_syncing, completed"). One build ever scored the part (0.8592 run). When the two strongest models on earth and the whole local fleet fail the same sub-part, the strong prior is probe semantics (the view is already populated pre-click, so no refresh is observable), not 17 identical app bugs. Currently 27% of top-band headroom.

**(f) The P tier is a dead instrument at this workload.** Measured across 20 measurable sb-5 builds: `p_list_latency` p95 **0.24–0.74 ms** vs 150 ms budget (**203–625× slack**); `p_page_interactive` **15–35 ms** vs 2000 ms (57–133×); `p_sync_wall` 1.4–5.1 recorded units vs 60,000 budget (one outlier at 7,851). Every real measurement lands in the top ladder bucket; the only zeros are `None` (feature absent/probe dead). The P checks are binary aliveness checks wearing a stopwatch — 5 pts of weight measuring nothing.

**(g) request_efficiency has a live vacuous-pass defect.** `ratio = 7/reqs` clamps at 1.0, so *fewer* requests than the measured optimum score full marks. Among 31 serious builds, **13 score 1.0 with 3–5 requests, of which 5 synced 0/247 rows and 5 synced only 100/247** (e.g. the 0.7820 build: "3 vendor requests", sync_completeness 0/247, request_efficiency 1.0). A build that fetches nothing is graded "optimally efficient" — the exact cheap-by-absence trap `second_sync_cost`'s docstring warns about. Only 16/31 earned the 1.0 honestly at exactly 7.

**(h) The local↔cloud gap is one defect fanned out, not a gradient.** Best local sb-5 build (baseline-n3-r3, 0.8645) vs Opus (0.9755): the 11.10-pt gap decomposes into client_all_payments +1.50, client_true_order +1.50, concurrent_sync_safe +1.50, total_field +1.12, summary_accuracy +1.12, resync_idempotent +1.12, sync_completeness +0.67, health_semantics +0.28, row_integrity +0.17 — **~7.9 pts are the single root cause "synced 100/247"** (ROOT_BLOCKS only attributes sync=0, not sync=partial). The moment the fleet lands full sync — and the operator-reported 0.8911 says it nearly has — it sits beside Sonnet with nothing left to distinguish them but grep noise. This is the compression mechanism, measured.

## 3. Check families ranked by separation bought per unit of finesse added

**1. Rendered-frontend family (J + V) — biggest buy, and the natural home for the harder-frontend/3D tier.** Highest weight-per-check (J = 0.030), highest sdS in the corpus (j_loads_data/.386, j_error_state/.386, v_dates_readable/.445), and 57% of top-band headroom already lives here — except it's currently spent on two validity defects. Fix those (fixture status mix; view_refreshed semantics) *and* add graded visual/3D checks: computed-style deltas, layout-metric assertions, and — for 3D — deterministic raw-WebGL/canvas probes (`gl.readPixels` region hashes at fixed frames, draw-call/buffer introspection via a wrapped context, CSS-3D matrix parsing from `getComputedStyle`). The corpus proves rendered-truth checks are the only family where cloud models still drop points for real reasons (v_pagination passT 73% before sb-5.3's hidden-table fix; run 9 scored 0.9528 on a page showing "Backend unreachable" — the sb-5.3 header documents it).

**2. Efficiency / HARD family — the best-behaved top discriminators, needing two mechanical fixes.** `second_sync_cost` is the single strongest live top-band discriminator (passT 55%, sdS .464) and `store_atomic_upsert` second (passT 55%). Fix `request_efficiency`'s clamp (defect (g)); make `second_sync_cost` continuous (observed conditional/304 ratios: 0/9, 3/33, 30/33, 3/3, 6/6, 9/9 — today the 30/33-conditional-but-0-304 build scores the same 0.0 as the 0/9 build; the 0.4 "mechanism exists" rung **never fired once** among 31 serious builds — a dead rung).

**3. Data-correctness compound family (summary_accuracy, row_integrity, chronological_order_full, health_semantics) — reopened by the harder API schema.** All saturate at top (passT 91–100%) because the schema is flat and single-currency: one integer sum, one status enum, one ordering. `summary_accuracy` is binary in practice (dv=2 over 93 builds). A v3 schema with multi-currency ledgers (per-currency EXPECTED_SUMs), nested refund references, and derived aggregate endpoints turns each of these into an N-bucket graded check with real partial credit — same probe machinery, no LLM judge.

**4. Static-grep family — 15.9 pts of sb-5 weight rides on regex/AST, and it produces 100% of the Opus–Sonnet gap.** modules_present + interfaces_declared (.050) + ui_states/ui_currency/ui_offline (.034) + ui_polish/ui_error_actionable/store_atomic_upsert/store_indexed/client_timeouts (.075). Convert to behavior: currency read from rendered cell text (the probe already harvests dateTexts — add amountTexts), timeout via a vendor `--stall` scenario that accepts and never answers (deterministic; the check measures whether sync returns within a bound), upsert-atomicity already observable through concurrent_sync_safe. This buys *validity* at the top more than spread — but validity at the top IS the current problem.

**5. P family — dead until the workload scales; thresholds alone buy nothing (proven).** At 247 rows, zero implementations landed between 0.74 ms and the 150 ms budget — there is no threshold in that void that separates anything. Scale the fixture (≥25k rows, computed aggregation endpoint, N+1-punishing detail view) and re-ladder from Bedrock calibration runs.

## 4. Concrete numbers the data supports

1. **request_efficiency:** 1.0 **iff** reqs == 7 **and** sync_completeness == 1.0; 8–9 → 0.75; 10–14 → 0.5; ≥15 → 7/reqs; reqs < 7 → 0 unless completeness == 1.0 *and* all three trap checks passed (the one honest 5-request/247-row build in the corpus had vendor_cursor_expiry = 0.0 — the trap evidence requirement catches exactly it). Evidence: distribution among serious = {7: 16, 5: 6, 3: 6, 4: 1, 15: 2}.
2. **second_sync_cost:** `0.5·(cond/reqs) + 0.5·(304/reqs)` continuous. Evidence: today 17/31 serious score 0.0 including a 30/33-conditional build; 10 score 1.0; the 0.4 rung fired zero times.
3. **v_status_distinct:** fixture must interleave ≥4 statuses so page 1 always carries ≥3; then 1.0 iff ≥3 distinct styled (color,background) pairs, 0.6 for 2, 0.2 for styled-single, 0 for plain text. Evidence: 19/22 scoring builds are fixture-capped at 0.5 today; check currently contributes 30.1% of all top loss while measuring nothing.
4. **j_sync_journey:** drop or re-measure `view_refreshed` (probe from a half-populated db so the click observably changes row count). Evidence: 1/17 button-having builds pass it, including 0/2 cloud.
5. **p_* ladders:** keep the ladder mechanism, scale the workload first; at 247 rows even a 10 ms budget (15× tighter) passes 20/20 measurable builds. Post-scale, set 1.0-bounds at ~2× the reference implementation's measured value via the Bedrock calibration path, keeping the quantized quarters.
6. **summary_accuracy:** the ±10% linear band never fired in 93 builds — replace with per-currency bucket credit under the v3 schema (k of N currency sums exact), which converts a dead continuous formula into real N-step resolution.
7. **j_loads_data:** make the reconcile half compound — rendered == page size **and** DOM-claimed == 247 **and** page-2 navigation renders the next tranche. Evidence: at top it's 100% saturated while mid-band details show three distinct failure shapes (claims None / claims 25 / claims 100) that the current 0.5+0.5 split cannot tell apart.
8. **Root-cause attribution must cover partial sync:** extend ROOT_BLOCKS to attribute when sync_completeness < 1.0 (not only == 0), since 100/247 fans one defect into ≥8 check names and currently reads as breadth (§2h).

## 5. Caveats

- The serious-cohort J/P/V columns rest on 11 sb-5 builds and the top-band J/P/V on 2 cloud runs — small n; directionally consistent with the 39-build all-sb-5 column but the exact percentages will move with the next calibration batch.
- Probe-environment failures (`PROBE UNAVAILABLE: JSONDecodeError`, the F834 class) score 0 in J/V and inflate their sd slightly; I counted one such build in the ≥0.5 band (0.5065).
- The 0.8911 fleet score cited by the operator is not yet in any archived verdict or the bridge-ledger (latest ledger rows top out at 0.9704 single-node / 0.7342 n3-r1); nothing in this report depends on it.
- `graded.db` files contain app payment data, not per-check grades — the per-check corpus is verdict.json-only.
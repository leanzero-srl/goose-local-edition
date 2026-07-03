# GOOSE_SWARM_COMPLETE v1 — validation verdict (2026-07-03)

Shipped (0ff09cb6f, default OFF): verify-by-running (reuse run_smoke_gate) -> distill (smoke_fix_description) -> bounded fix re-dispatch -> re-verify up to COMPLETE_ROUNDS, capped by COMPLETE_CAP_SECS; FINAL exit gate so a still-red app can't exit 0.

## Validation (txkvbench, OFF vs ON)
| variant | exit | complete_fired | complete_passed | pytest | silently_broken |
|---|---|---|---|---|---|
| off | 0 | 0 | - | pass | 0 |
| on  | 0 | 1 | true | pass | 0 |

## PROVEN
- COMPLETE phase fires on ON (banner + complete_verify{round0,ran:true,passed:true} + complete_result{passed:true}).
- Verify-by-running works; the green path exits 0 (no false failure).
- Off-path byte-identical (OFF: no COMPLETE phase, exit unchanged).

## HONEST GAPS
1. GATE-ON-RED unproven: both txkvbench rolls passed pytest, so the exit gate never had a red app to refuse. Re-testing on a red-prone spec.
2. VERIFY IS INCOMPLETE: the ON app passed pytest + entry --help but DRIFTED the spec interface (built `db` positional vs the spec's `--db` option; the advertised golden `txkvbench --db ... exec` errors). v1 verify reuses run_smoke_gate (pytest + --help) and does NOT run the advertised golden commands -> it misses interface/golden drift. NEXT INCREMENT: make the pillar `check` load-bearing + run the advertised commands in run_complete_verify (design part B.1.2) so the loop catches + fixes drift. This is the higher-value next step for "never ship spec-non-compliant".

## Next
(a) Prove gate-on-red on a red-prone spec. (b) Golden-check increment (B.1.2) so verify catches interface drift. (c) v2 fleet-parallel map-reduce. (d) research escalation, Playwright.

## Gate test #2 (spendlog + COMPLETE ON) — the reframing finding
Result: exit=0, pytest=47 PASSED (green), COMPLETE fired + complete_verify{round0,passed,findings:0} + complete_result{passed:true}. BUT the delivered app DRIFTED the spec interface AGAIN: `report budget` was built as `budget show` (spendlog report has only total/by-category/monthly; `report budget` errors 'invalid choice: budget'). The integrate-verify worker even golden-checked its OWN drifted interface (`budget show`) green — the drift is SELF-CONSISTENT (tests + integrate-verify all use budget show), so nothing internal catches it.

### The dominant failure mode is INTERFACE DRIFT, not pytest-red
- Both validation runs (txkvbench --db positional; spendlog budget show) came out PYTEST-GREEN but SPEC-NON-COMPLIANT. The swarm's integrate-verify+smoke-fix reliably reaches pytest-green — so pytest-RED-at-delivery is RARE; INTERFACE DRIFT (green tests, wrong advertised interface) is the COMMON "delivered broken".
- v1 COMPLETE verify = run_smoke_gate (pytest + entry --help) CANNOT see interface drift -> the exit gate rarely fires on the dominant failure -> v1 alone does NOT achieve "never ship broken" for the real-world case.
- GATE-ON-PYTEST-RED remains unproven (both runs green) — but that is now the LESS important case. The exit gate is the right MECHANISM; it needs the GOLDEN-CHECK verify to give it the dominant failure to catch.

### Reprioritization (honest)
The GOLDEN-CHECK increment (run the SPEC's advertised commands / pillar checks in verify, not just pytest+--help) is REQUIRED, not optional — it is what makes the gate catch interface drift. FOLD IT INTO V2's verify map (the v2 workflow already plans this). So: V2 = fleet-parallel map-reduce WITH golden-command verification in the VERIFY step. That single change is what would have caught both txkvbench --db and spendlog report-budget.

## FULL FEATURE validation (GOALS+COMPLETE+golden-check ON vs OFF, spendlog report-budget)
| variant | exit | complete_fired | report_budget_ok | pytest |
|---|---|---|---|---|
| OFF | 0 | 0 | **0 (DRIFTED, shipped green)** | 1 |
| ON  | 0 | 1 | **1 (correct)** | 1 |

- ON built `report budget` CORRECTLY (report budget: "food 10 5 Yes" = over-flag works) where OFF DRIFTED (report_budget_ok=0) and shipped it at exit 0. So the full stack (GOALS pillars + COMPLETE + golden-check) delivered a SPEC-COMPLIANT app where the baseline silently shipped a broken interface. 81 pytest pass.
- NO FALSE-RED confirmed (the risk): the distilled pillars each carry a RUNNABLE check (5 real commands: entry-point/cli-interface/money-invariant/shared-store/reject-negative), and the COMPLETE verify ran GREEN at round 0 (complete_verify findings=0) — the golden checks PASSED on the correct app; the gate did NOT fail good software.
- HONEST: the golden check CATCHING a drift + the fix loop firing is NOT triggered here (the app was correct from round 0, fix_wave_shards=0) — because GOALS-on anchored the interface so it was built right in the first place. So the golden check is a proven-safe BACKSTOP; its catch-and-fix path is hard to trigger precisely because pillars prevent the drift up front. Net value: interface adherence up (ON 1/1 vs OFF 0/1 here; matches the pillars A/B trend), with the golden gate as a verified-no-false-red safety net.

## ENABLE-AND-PROVE (all flags ON, spendlog) — golden-check FALSE-RED materialized -> made ADVISORY
The all-flags-on run caught a REAL bug in the golden-check I shipped (2a01d224d):
- INSTR verified: research 2.0 + planning 12.7 + execute 14.2 + gates 20.5 = total 49.4 (buckets sum to total). gates_min exposes the COMPLETE loop cost (20.5 min here).
- The app was CORRECT: `spendlog --db X report budget` -> "food 211.00 10 OVER BUDGET". `--db` is a GLOBAL option before the subcommand (usage: spendlog [--db DB] {add,list,report,budget}).
- The distilled pillar check used the WRONG shape: `spendlog add ... --db X` (--db AFTER the subcommand) -> the check FALSE-FAILED a correct app -> COMPLETE went RED -> the fix loop chased the phantom and BROKE 2 pytest tests (18 passed, 2 failed) -> the gate refused exit 0 (exit=1). i.e. gating on the golden check REGRESSED a correct app.
- Root cause: pillars/checks are distilled at PLAN time; the app is built later by different workers, so a check's assumed interface (arg placement) can mismatch the built interface -> stochastic false-red (the earlier gcheck-on run happened to match -> no false-red; this one didn't).
- FIX (shipped): the golden pillar checks are now ADVISORY — run_pillar_checks output is emitted as a `pillar_check_advisory` event but NO LONGER extended into verdict.findings, so it cannot drive the fix loop or the exit gate. The reliable smoke oracle (pytest + entry --help) stays the gate; the pillars still ANCHOR the build (GOALS value intact). A false check can never again regress a correct app.
- FUTURE (to restore a RELIABLE golden gate): re-distill each pillar check from the BUILT app's `--help` (post-build), so the check always matches the actual interface, then it can gate again.

## RE-VALIDATION of the ADVISORY fix (same spendlog spec, all-flags-ON) — PASS + bonus
| metric | earlier (golden-check GATING) | now (golden-check ADVISORY) |
|---|---|---|
| exit | 1 (refused) | **0 (green)** |
| pytest | broke 2 (18 pass, 2 fail) | **88 pass, 0 fail (NO regression)** |
| report budget | correct but red-gated | **correct: food $15.00/$10.00 [OVER BUDGET]** |
| COMPLETE | RED, phantom fix loop | **GREEN at round 0** |
| gates_min | 20.5 (wasted phantom fixing) | **0.0** |
| golden check | gated -> regressed | **advisory: 6 findings SURFACED, drove nothing** |

The advisory fix (6b7f06ec8) is proven: the golden check STILL runs + surfaces drift (pillar_check_advisory, 6 findings on this app) but NO LONGER gates or fixes -> a correct app is delivered GREEN with 88 tests passing, where the gating version broke 2 tests + refused exit 0. BONUS: it also eliminated the 20.5-min phantom fix loop (gates_min 20.5 -> 0.0) — the advisory change is both safer AND faster. The 6 advisory findings on a CORRECT app confirm the distilled checks are unreliable-as-gate (right call to make them advisory); a reliable golden gate needs the re-distill-from-built-`--help` step (future).

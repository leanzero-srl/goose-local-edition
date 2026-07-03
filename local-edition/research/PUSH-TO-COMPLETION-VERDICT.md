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

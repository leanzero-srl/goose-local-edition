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

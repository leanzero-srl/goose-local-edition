# Pre-boundary results — DO NOT COMPARE ACROSS THIS LINE

These units ran on the engine built 2026-07-31 23:30, before the day's fixes. That binary does not
contain `GOOSE_SWARM_DETAIL_BUDGET_SECS` or the `detail_fallback` event at all — verified with
`strings` — so the `detail_budget` arm could not have fired and would have recorded a confident
"no effect".

Kept because the EVIDENCE is still good: `baseline-n3-r0` is the run whose `meridian` module got a
116-character brief naming five behaviours and no endpoints, then called `/payments` where the
vendor serves `/v1/payments`. Crunched by running it: total_count() 0 of a required 247,
fetch_all_payments() empty, create_payment() dead on an uncaught 404 — against a 50.0% headline
with tier A at 100%.

What is void is its use as a BASELINE. Results now carry `engine_build`, `complete()` requires a
match, and `loop.sh results` refuses to average across a rebuild.

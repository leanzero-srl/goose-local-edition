# Engine events no instrument reads (F632, 21 runs, modern corpus)

Third-pass audit: 43 distinct event types, 31 genuinely read, 12 blind.
`prose only` = the name appears in a docstring/gate description but never in a comparison.

- ~~**spec_contract**~~ **READ (F634)** — 0 findings, but 1 in 3 advertised checks INCONCLUSIVE and ungraded by anything
- **spec_contract (orig)** (33x) — QUALITY — prose only in crunch.py, phases.py
- ~~**cross_module_drift**~~ **READ (F633)** — guard working: 0 findings, 6-12 modules checked/run
- **cross_module_drift (orig)** (18x) — QUALITY — never mentioned
- ~~**orphan_files**~~ **READ (F635)** — 75% of orphaned files are REPLAN-INJECTED output; 9 firings, all 3-node, zero 1-node
- **orphan_files (orig)** (8x) — QUALITY — never mentioned
- ~~**sink_capped**~~ **READ (F636)** — fires 1 of 21 runs (0/9 at one node, 1/12 at three)
- **sink_capped (orig)** (1x) — QUALITY — prose only in review.py
- **complete_missing_deliverables** (1x) — QUALITY — never mentioned
- ~~**http_timeout_scan**~~ **READ (F637)** — the only detector that FINDS defects; fix loop clears 92%; arm gap 4.2x per run but only 1.65 SE
- **http_timeout_scan (orig)** (33x) — SPEED — never mentioned
- ~~**scouts_planned**~~ **READ (F642)** — fan is structurally FLEET-BLIND (`select_lenses` ignores node count; 3 lenses on greenfield) AND research wall-time does not drop with the fleet: median 395s vs 377s = 4.6%, 0.64 SE
- **scouts_planned (orig)** (21x) — SPEED — never mentioned
- ~~**sink_plan**~~ **READ (F636)** — cap scales with tree_bytes (1.13x) but is binding in only 1 of 21 runs; non-issue
- **sink_plan (orig)** (21x) — SPEED — never mentioned
- ~~**research_tools**~~ **READ (F644)** — `can_look_things_up` FALSE in 42/42 runs, `available` EMPTY always: NO MCP research
  extension has ever been registered. But the alarm dies at its premise — scouts keep the developer shell and curl, so
  **80.7% of 119 findings are grounded in a real fetch** (0 runs with zero grounded). Arm gap +0.069 at 0.94 SE — a hint.
- **research_tools (orig)** (21x) — other — prose only in sweep.py
- **review** (18x) — other — prose only in armcheck.py, dispatch_audit.py, occupancy.py, phases.py, review.py, selftest.py, sinkwatch.py, sweep.py, tierlog.py
- **run_overview** (18x) — other — never mentioned
- **complete_fix_completed** (17x) — other — prose only in verdicts.py

## Why this list exists

Twice in one hour an unread signal surfaced by accident (F630, F631), and the
second cost a published number. Start here before building a proxy.

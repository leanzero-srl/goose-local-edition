# Vigil actions — the queue the surgeons are dispatched from

Filed by the tick-surgeon (`~/goose-builds/loop-state/note.sh action <surface> "<text>"`) and by the
orchestrator. One row per action, newest last. **STATUS**: `OPEN` → `CLAIMED <agent|sha>` → `DONE <sha>` |
`DROPPED <reason>` | `SCHEDULED <run>`.

The rule (Mihai, 2026-09-01, after r6d's research fan burned 165 minutes at 59% spec-lookups under four
ticks that said `continue`: *"The tick-surgeon should have caught this. please augment it so that it also
takes notes and feeds in the right place for the other surgeons to take action on"*): **every OPEN row is
triaged at the orchestrator's next turn** — IMPLEMENT (dispatch the surgeon for that surface, batched by
file, mark CLAIMED), DROP (with the reason, never silently), or SCHEDULED (a named run). **No OPEN row
survives two ticks.** A surgeon brief cites the VA id; the closing commit sha goes in the row. TICK-NOTES.md
keeps findings; THIS file keeps the work they demand. `surface` ∈ swarm.rs · scheduler · panel · tick.py ·
prompt · design · harness.

| id | filed | surface | status | action |
|---|---|---|---|---|
| VA-001 | 09-01 | swarm.rs | CLAIMED fan-cut surgeon | research (r6d): 38 planned, 27 dispatched at 165m, 13 SPEC-LOOKUP (198 lane-min) + 3 DUP + D1 decided 3× · 'api-q1 sort/status/currency — request.md:148' -> opener prompt asks for "the QUESTIONS that must be answered" with no lookup/decision split; fan dispatches one lane per question -> spec-lookups become cited SPEC FACTS at synthesis (no lane); decisions once + dedup vs landed minis; ONE lane per slice |
| VA-002 | 09-01 | swarm.rs | OPEN (refuter measuring r5 6k vs r6c 21k) | build (r6c): brief_median 21,104 vs r5 6,054; reasoning 2.44M vs 1.48M chars; BUILD 608m vs 325m -> ledger block + dep_block + minis injected into every worker brief -> BRIEF DIET: cut the sections the long-pole lanes re-read, keep cited facts; measure which section the thinking re-derives (gate 7 quotes) before cutting |
| VA-003 | 09-01 | swarm.rs | CLAIMED fan-cut surgeon (E7 corrections) | research (r6d q5): steer-cut turn on a structured-output lane receives "You MUST call the final_output tool NOW" + the relay note in one delivery -> agent.rs `Some(None)` arm precedes the pending-steers arm; relay drain unreachable during a judge look -> `Some(None) if has_pending_steers => {}`; drain inside the probe select or delete same-slice relay under one-lane-per-slice |
| VA-004 | 09-01 | scheduler | CLAIMED fan-cut surgeon | placement (r6d): aux order was [workhorse, gabee, mihai] — planner_rank speed_weights matched host+model_id and mihai's `mihai-qwen3.8-27b` matched none of {local,gabee,worksmacstudio} -> pattern gap -> match on device id as well |
| VA-005 | 09-01 | swarm.rs | OPEN | synthesis (r6c/r6d): `spec_documented_keys` misses keys documented in prose shape (not table) -> the extractor reads header-named table cells only -> extend to the prose "`key` — description" shape with a test on request.md |
| VA-006 | 09-01 | design | OPEN — needs Mihai's ruling | repair (r6c): `complete_result.passed=true` while two render criticals (zero rows, black 3D field) stood -> `final_passed = criticals.is_empty()` partitions on the engine_critical TEXT class only -> either render-gate findings join the partition or the event says "boots-and-answers", never "passed" |
| VA-007 | 09-01 | swarm.rs | SCHEDULED r7 | judge (r6d): 30/63 looks ended in `shell` no-ops; abandoned looks; no refutation memory across looks -> the judge holds no tools by axiom (E14 landed); the DESK (reader with memory) is the r7 design in DESIGN-JUDGE-DESK.md |

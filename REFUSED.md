# REFUSED — items culled by review, kept for possible revival

Mihai, 2026-08-30 00:19 EEST: *"What does not survive the culling of your reviews does not survive and that's it. Put it
somewhere as refused item so that maybe we can check it again and either revive it or not."*

One entry per refused item: what it was, which review killed it and WHY (the refuter's words), the date,
and what would have to change to revive it. An item here was never built or was built and reverted — it is
NOT the same as EXPERIMENTS-LEDGER.md, which records what RAN and what it measured. Check this file before
proposing something new: a refused item that matches gets revived-or-re-refused here, not re-invented.

## P1-11 — Delete supervision code (judge.rs 1,385 lines, omni wiring, stream-loop judge branches, distill_pillars, sink idle-fill,
- **source:** Part I §9 row 11
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** The design's own MILD amendment defers it to r4 ('deletion only if r3-without-supervision is not worse on the criticals'), and r2 strengthened the keep-with-cost-cut case: nudge efficacy 2/2 on tool-using workers (contract-service-boot 19:13:44 closed CONTRACTS within a minute; api-endpoints 19:19:51 → tool_calls 0→2 by 19:21:05) while the judge's failures were input/transport defects that A-2/N-7 fix, not existence defects. Deleting now would also retire 2b1e755ac before it is ever exercised.
- **revive if:** r3 completes with the judge ON and the omni_judge=false experiment arm (P1-10) measures not-worse on the criticals with the cost delta in hand — then r4 deletes per the design

## II-6 — Bonus tasks out of the join's claim gate (scheduler.rs:1204-1212, :1396-1406)
- **source:** Part II §II.7 row 6
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Mooted by its own gating clause: 'lever off ⇒ row DROPPED, no bonus tasks exist and the finding is answered upstream' — and dynamic_replan=false is decided in P1-10 (r2: the replanner's two 296/327-char template tasks held the sink 32m38s behind the B2 gate, replanned 19:45:05Z → sink 20:22:20Z). Building the predicate change for a lever that is off is throwaway plumbing.
- **revive if:** dynamic_replan is re-enabled with r3+ evidence that replanning earns its cost — the claim-gate change then lands WITH it as one batch

## II-12 — Research writes quarantined to .swarm/research-writes/
- **source:** Part II §II.7 row 12
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Mooted by P1-5: the RESEARCH fan and coverage lanes that produced the stray writes (slice-camera-system's viz_camera.js/test_camera.js at root, TICK-NOTES 08-29 21:53) are deleted; OPEN and SYNTHESIS are single schema-reply calls that write nothing. Instrumenting a deleted layer is exactly the interaction the queue must not carry; II-1's fs_delta still catches any residual stray write from ANY phase, so the detector exists without the quarantine pass.
- **revive if:** coverage/RESEARCH lanes are revived (see P1-5's revival path) — the quarantine pass lands with them

## II-13 — Coverage dispatches per enumerated part (amends Part I step 5)
- **source:** Part II §II.7 row 13
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Mooted by P1-5's coverage deletion (decided under full autonomy; the row itself was owner-gated with 'the table's DELETE stands' and never run). Its measured motivation (~20 of RESEARCH's 48 min at 1/6 occupancy behind coverage_complete) is dissolved, not fixed, by the deletion.
- **revive if:** if r3's plan_repaired.after shows advertised surfaces going unowned that repair rule (d) + the skeleton failed to cover — the sse-endpoint class coverage caught in r2 — coverage returns WITH per-part dispatch as one change

## II-15 — pre_review kept constrained to off-build-lane nodes
- **source:** Part II §II.7 row 15
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** r2's score is in and the ledger absorbs its one proven value: the persist→inject mechanic (swarm.rs:28403-28427 → :33005) is explicitly the pattern §II.2 generalizes, and its single landed fix (vendor_sync limit=100, sink edit 22:08Z) is a spec-mismatch class the formed message now carries deterministically. Against that: 9 calls of 220-340 s on done tasks while the sink waited, and a 7,535 s pre_review holding gabee through the 11-minute retry starvation (seq 530). NOW's standing no-throwaway plan already switches it off with one env line; building placement constraints for a switched-off layer is plumbing for a deletion.
- **revive if:** r3's gate+ledger provably misses a semantic spec-mismatch finding of the limit=100 class (a scored defect the formed message did not carry) — re-arm via the existing env line, constrained to non-build nodes, in the same change

## A-4 — tail_review dry-streak / stop-when-dry rule
- **source:** assessment forensic ('no stop-when-dry rule') — named top defect
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** The layer is OFF in r3 (P1-10 env =0; scheduler.rs:59-64 verified) and its code dies with the r4 deletion; a dry-streak rule built now is instrumentation on a deleted layer — the exact 'no throwaway plumbing' violation (Mihai 22:20: 'if something gets deleted and redone what's the point'). The transport-error-reads-as-clean-review half of the defect (swarm.rs:28490-28493 found==None ⇒ had_findings=false) is fixed for the surviving judge path by A-2.
- **revive if:** tail_review is ever re-armed — it may not come back without (a) the dry-streak rule and (b) the A-2 transport/clean distinction, as preconditions in the same commit

## N-2 — afa644ddd — GOOSE_PROVIDER_READ_TIMEOUT_SECS=1800 as a keep
- **source:** NOW r2-table row 2
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** SUPERSEDED by II-7's read-window-off (II.4 names the verdict explicitly so a KEEP-class row is not overruled silently): the 581 s silent live slot (TICK-NOTES 08-29 21:23) is answered better by deleting the window than widening it — 1800 s is still a time cap on silence, which the owner's rule forbids. The env line main.ts:2808 is deleted in the same II-7 change.
- **revive if:** II-7's lms-ps liveness replacement proves unshippable or wrong in isolation tests before r3 — then 1800 s returns as the interim, explicitly labeled a violation awaiting the real fix

## G-3 — Dispatch a slice the moment its brief lands (redesign step A)
- **source:** agenda :2676
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Mooted by P1-5 + III-1: there are no briefs to land — RESEARCH is deleted and the planning runway shrinks to OPEN → SYNTHESIS → repair (r2 planning: 22+2+48+5+7+6 = 90 min becomes ~30), then the SKELETON task starts BUILD immediately. The item's goal (stop making BUILD wait on planning) is achieved by removing the wait, not by overlapping it; its close condition (task_dispatched before plan_loaded) becomes structurally impossible AND unnecessary.
- **revive if:** r3 measures OPEN+SYNTHESIS still dominating wall time (>25% of run) — then early dispatch of the skeleton task before plan_loaded is the targeted form to build

## G-4 — The judge living outside the phase machinery (the 'warden' full design)
- **source:** agenda :2683
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** The owner's never-redesign-from-scratch rule cuts against landing a whole supervision re-architecture in the same r3 that deletes CONTRACTS/RESEARCH and reshapes INTEGRATE; the interim fixes that shipped or are queued answer the measured harms: the re-stream (N-1) breaks the reasoning-loop class the warden targeted, the probe no longer freezes the lane's durable logs (c3b211582, d949b667c), N-7 gives the judge the delivered files, A-2 stops transport-as-verdict. r2's evidence says the judge's problem was inputs and cost, not placement (2/2 efficacy on tool-using workers).
- **revive if:** r3's judge-ON arm shows steering still failing on planner/reasoning calls DESPITE the re-stream (judge_restream events with no behaviour change), while gate-on-completion findings sit undelivered — that is the specific measurement the out-of-phase placement answers

## G-5 — qwen3.8-flash direct-token cloud plan (blocked on a cloud binary)
- **source:** agenda :1899
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Not engine/desktop work for r3 — it is a cloud benchmark arm, and this session starts no benchmark run per the finished-run rule; the block is external (a sealed cloud binary), and the standing route around sealed binaries is OpenRouter (memory: openrouter-is-the-way-around-sealed-binaries), which needs no r3 engine change.
- **revive if:** after r3 ships and the owner asks for the cloud arm — route the model through OpenRouter per the memory rather than rebuilding

## G-6 — The number to beat (0.0274 published / 0.2006 corrected target)
- **source:** agenda :2604
- **refused:** 2026-08-30 02:30 EEST, by the r2-assessment queue review
- **why:** Not a buildable change — it is the campaign's success criterion; it closes when a hermetic score beats 0.2006, which r3's run (post-queue) tests. Everything in this queue is the means; queuing the goal itself would be a row with no diff.
- **revive if:** n/a — remains the standing yardstick; re-examined only if the owner changes the target

## REVIEW-deletion (Part I §9 step 5's REVIEW half) — delete the one-round REVIEW
- **source:** Part I §9 row 5
- **refused:** 2026-08-30 02:30 EEST, transcribed out of P1-5 per the refuter (a refusal may not hide inside a build row)
- **why:** r2 measured the one-round REVIEW working: 7 min, 9 findings → 10 patch touches, sharing 0 / owning-nothing 0, no round 2. Deleting a mechanism the run just proved earns its keep contradicts the evidence rule; RESEARCH/coverage die, REVIEW stays.
- **revive if:** a later run shows REVIEW's round adding wall time without changing the plan flags (findings that patch nothing).


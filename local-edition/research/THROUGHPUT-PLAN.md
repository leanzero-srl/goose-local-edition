# Swarm throughput plan — adversarially vetted (audit w06exoqzw, 2026-07-03)
Master goal: cut wall-clock TIME at EXACTLY constant quality via node utilization. 12 proposals, each with a skeptic verdict. On the 3-node fleet, MOST "obvious" parallelism ideas are NO-OPs or quality-changers — the audit protected the stable system.

## SHIP (safe, quality-invariant)
- **INSTR-post-execute-buckets (code, HIGH, ship-gated)** — the post-execute gate phases (COMPLETE/SMOKE/REVIEW) run AFTER t_exec and are NOT in any timing bucket or total_m (swarm.rs ~7317..7691). Add a t_gates timestamp + gates_m bucket. Pure instrumentation, zero behavior change. PREREQUISITE: without it, no post-execute parallelism ("the single-node fix tail while the fleet idles") can be proven faster. LAND FIRST.
- **K6 weight/instances (knob, MED, ship-with-guard)** — the PRIMARY concurrency lever + quality-SAFE (placement is output-neutral: files_conflict gate + dep-scoped context slices are invariant to slot count). BUT thrash-risk: an uncapped config weight override (swarm.rs:1073-1077 "user wins") can exceed real LM Studio PARALLEL -> queue/timeout/kill storm; instances>VRAM -> eviction flap that can flip a working run to FAILED (attempts exhaustion). GUARDS: clamp effective weight to probed PARALLEL (kill the silent uncapped path), VRAM-headroom gate on instances, thrash back-off + exclude infra-eviction transients from the attempt count. Only raise where the hardware genuinely serves PARALLEL>1 (verify first).

## NO-OP on the 3-node fleet (no delta; don't ship as throughput wins)
- K2-research-width: REDESIGN — only 4 lenses exist (cap already saturated at 4); "fleet width" with a 1..8 clamp + no floor sets max=3<4 -> DROPS the edge-cases lens = a regression. Broken as specified.
- K3-best-of-n: NO-OP (parallel_planning already sizes to fleet width on 3 nodes). Guard: don't bake a config floor of 2 (thrashes a degraded <2-node fleet).
- C1-concurrent-judge: NO-OP at capacity<=3 (K_max=floor(3/2)=1); at capacity>=4 it CHANGES quality (more aggressive kill+redispatch = different attempts land). Wide-fleet-only knob, not for the target fleet.

## REJECT / REDESIGN (inherently change quality)
- K4-split (lower SPLIT_SECS 900->450): REJECT — split-storm on HEALTHY tasks; ChildSpec has NO description (child dispatched semantically empty) + cohesion drift. Inherent content-quality change.
- **K5 = GOOSE_SWARM_COMPLETE_PARALLEL (MY SHIPPED V2, 6790b07d5): REDESIGN — REAL BUG.** My "shadow isolation => disjoint writes" claim is FALSE: the fix agents dispatch speculative:false, so run() roots them in the REAL cwd with NO shadow (shadow is speculative-only, swarm.rs:5814-5825); owned_files is only a PROMPT hint (weak models violate it). => concurrent agents CAN write shared/coupled files -> race + NON-DETERMINISTIC verdict + possible completion-rate regression. Default-OFF so the default path is unaffected, but the flag is NOT safe to enable as-is. FIX: real per-fix shadow+promote (needs the dead promote path fixed, below) or a merge/conflict model; until then keep OFF + doc-warn.
- C5-parallel-post-fixes (SMOKE/REVIEW fix): REDESIGN — those fixes are deliberately cross-file (owned_files=[]); partitioning them boxes the agent away from the real fix target (callee / the shared importer) -> worse artifact. Wire findings never name a file -> all unassigned -> zero parallelism anyway.
- **C2 = GOOSE_SWARM_SPECULATE: REDESIGN — PRE-EXISTING REAL BUG.** promote_speculative (swarm.rs:6276-6284) is DEAD CODE — scheduler.rs never calls it (0 'promote' hits); resolve_speculation is first-wins + aborts the primary + marks Done on a TEXT-only ctx merge, so a twin-win STRANDS the twin's files in a TempDir that's never drained -> the real tree loses the integrator's files (MISSING code), most often on the sink. Default-OFF, but broken. Also first-wins commits a nondeterministic twin. FIX: wire promote into the twin-win path + a VERIFY-BEFORE-COMMIT gate (twin wins only if its shadow passes the SAME acceptance oracle) so outputs are acceptance-equivalent. This is the ONLY mechanism that fills the SINK idle window (the biggest one) — highest-value throughput work, but the riskiest; do it with a fresh adversarial pass.
- C3-shared-types-leaf: REDESIGN — under-declared-dependency risk on a fresh build (no symbols exist at plan time to partition). Amendment-only + real static analysis, later.
- K1-max-replans (1->2): ship-with-guard — replan fires DURING the sink (not before); a bonus SUCCESS can land unverified code AFTER the sink's PASS. Guard: exclude integrate-verify from the replan trigger, OR mandatory verdict-gating re-verify (GOOSE_SWARM_COMPLETE on).

## Biggest REAL opportunity + honest read
The sink tail (integrate-verify runs alone on 1 node while 2 idle) is the largest idle window. The only mechanism for it (SPECULATE) is BROKEN. On the 3-node fleet the EXECUTE middle is already well-utilized (DAG + fanout for research/plan/contracts), so the wins are: (1) MEASURE the tail (INSTR), (2) the weight lever IF hardware supports PARALLEL>1 (K6+guards), (3) fix SPECULATE correctly to fill the sink (big, careful). My V2 needs the isolation fix before it's safe to enable.

---

## FIX DESIGNS — adversarially reviewed (workflow wcrg7pjap, 2026-07-03). All 3 = implement-WITH-CHANGE.

### SAFE now (skeptic HIGH): INSTR + K1
- INSTR gates_m: add t_gates + gates_m bucket after the post-execute phases, include in total_m. Verified compiles + logic sound. CAVEAT: redefines total_min (no longer = research+planning+execute); the harness times externally so it doesn't break, but keep all buckets summing to total. Default-off phases => gates_m ~= 0 by default.
- K1 replan-sink guard: exclude the integrate-verify sink from the dynamic-replan in_flight trigger (don't replan while the sink is the sole in-flight task). Preserves mutual-exclusion + stuck-bail; no deadlock (sink has a wall-clock cap; replan is opportunistic). Single call site. SHIP.

### K6 weight — REQUIRED CHANGE: WARN-ONLY, do NOT silently clamp
Skeptic: swarm "weight" = concurrent AGENT tasks (bursty, idle LM Studio slots between LLM calls), so oversubscribing weight>PARALLEL to overlap gaps is a LEGIT throughput tactic; silently clamping inverts the documented "user wins" contract. => Do NOT clamp. WARN on mismatch (weight > probed PARALLEL) only; keep the user's weight. (The real thrash guard is the back-off + excluding infra-eviction transients from the attempt count, from the original audit.)

### SPECULATE fix — implement-with-change, but 2 HARD defects (default-OFF, no rush; PRESENT the diff before shipping)
Design: verify_speculative (run_smoke_gate on the shadow, GREEN-only, fail-closed) + promote on a locked win + a lock-split of resolve_speculation. Skeptic found:
1. OWNS-NOTHING CORRUPTION (critical): integrate-verify owns [] -> verify passes on the green shadow, win locks, promote copies ZERO files, the primary integrator is aborted, commit marks Done => ships the primary's PARTIAL tree, discards the twin's whole-tree edits. REQUIRED: make 'skip owns-nothing tasks in pick_speculation_target' MANDATORY, OR fail-close verify_speculative when owned_files is empty.
2. ABORT-WITHOUT-JOIN double-write: the scheduler holds only a tokio AbortHandle (the JoinHandle is dropped at spawn); abort() is non-blocking, so a primary write already dispatched to a spawn_blocking thread can land AFTER promote. REQUIRED: keep the primary JoinHandle + await it after abort (join before promote), or a cooperative-cancel the worker checks. This is genuine hard concurrency -> implement carefully + PRESENT before shipping; SPECULATE stays default-OFF.

### V2 (COMPLETE_PARALLEL) fix — implement-with-change (tractable; no abort-join since fix agents run to completion)
Flip fix-agent dispatch speculative:false -> true (the ONLY consumer of req.speculative is make_shadow) => each shard writes its own shadow, not the real cwd. REQUIRED additions (do NOT exist yet):
1. MANDATORY file-key normalization in group_findings_by_file / extract_file_from_finding (strip leading ./, collapse //, unify separators) so two spellings of one file can't become two shards -> two promotes to the same real dst -> torn write.
2. The per-shard promote call, a discard_shadow(task_id) on every non-promoted shard (undefined today), and the post-loop SERIAL cross-file fallback (a serial v1 fix if still red after the parallel rounds, so a cross-file fix is never dropped).
3. It is the FIRST production + CONCURRENT caller of make_shadow/promote (only a single-threaded unit test today) -> gate hard + a concurrent unit test.

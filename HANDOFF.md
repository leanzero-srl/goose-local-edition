# HANDOFF: fix/measured-load (goose 3.0.131, measured-load admission)

Worktree /Users/mihaiperdum/Projects/goose-measured, branch fix/measured-load (--no-track, from origin/main efdea890d = 3.0.130).
Commits so far: 9febfd775 (WIP core), 50aedce38 (fmt). `cargo check -p leanzero-link --lib` is clean. Tests do NOT compile yet.
Do not release (the coordinator bundles 3.0.131). Repo identity, no AI credit lines. Delete this file in the final commit.

## Owner rule (2026-10-25, tightened)
"use resources to their fullest; the goal is to not kill the machines but USE THEM". Busy = MEASURED load only.
Shared defaults (must match machine-sync heavy-run/node-run, which is being changed in parallel, NOT landed yet in machine-sync a10dbef):
max_load_per_core 2.0 (backstop only), max_heavy max(2, cores // 4), default_need_gb 8, min_free 20.
Slot files: slot 0 = ~/.cache/heavy-run.lock, slot i = ~/.cache/heavy-run.lock.i (i < max_heavy).
Cards/verdicts show how much more a Mac can take: "can take ~11 more 8 GB jobs".

## DONE (crates/leanzero-link)
- load.rs: NodeLimits gains max_heavy (Option, json "max_heavy"), max_load_per_core (2.0), default_need_gb (8.0); `default_max_heavy(ncpu)`,
  `max_heavy_for(ncpu)`. HeavySlot::try_take_any(base,max,holder)->Option<(slot,idx)>, `heavy_slot_path(base,i)`, `heavy_slots_used(base,max)`,
  `heavy_holders_reason(base,max)` ("busy: 3/3 heavy slots held: <holder 0>; ..."). OwnLoad gains heavy_slots_used, heavy_slots_max
  (0 = older report), can_take, verdict (line). heavy_slot_held now = ALL slots full (so a 3.0.130 placer stops excluding on one slot).
  sample_own probes all slots.
- jobs.rs: UserActivePolicy = Allow (default) | Refuse; parse "last_resort" -> Allow. New `Gate {min_free, max_load_per_core, default_need_gb}`
  (Gate::new(&limits,min_free), Gate::with_floor(x)). `verdict(own, need, heavy, &gate)` -> Verdict {busy, user_active, score, avail_gb,
  can_take, line}; busy reasons all start "busy: " and carry numbers ("busy: memory 12% free < 20% floor", "busy: memory 41 GB above the
  20% floor < need 48 GB", "busy: 3/3 heavy slots held", "busy: load 2.40/core >= 2 backstop"); OK line "ok: 1/3 heavy, 94 GB free,
  load 0.28/core, can take ~11 more 8 GB jobs[, in use]". New RefusalCode::CpuOverloaded (409, heavy only). `stamp_summary(own,&gate)`
  fills OwnLoad.can_take/verdict; load_with_jobs stamps it (both branches). `admit(own, req, &gate, policy)`: user_active refuses ONLY
  under Refuse, with numbers. Placement {node_id,user_active,score,can_take,verdict}. plan_placement(self_id, rows, need, heavy, &gate,
  strategy) sorts own_rank (strategy) -> can_take desc -> user_active (TIEBREAK) -> score. rank_placements delegates to plan_placement.
  submit: takes ANY free slot via try_take_any (slot 0 compatible with an external flock), refusal reason = heavy_holders_reason.
  watch_adopted retakes any slot. JobRegistry::gate(), JobRegistry::measured_busy().
- manager.rs: plan uses &reg.gate(); try_place always sends allow_user_active=true (3.0.130 receivers on last_resort take it).
- control.rs /v1/swarm/execute: the "node status != Idle -> 409" flag guard is REPLACED by ctx.jobs.measured_busy() (audit item: a Mac
  refused all remote prompts whenever any session of its own was live). Session-level double-reply guard stays in link_serve.rs:318.

## TODO (in order)
1. Lib tests in crates/leanzero-link/src/jobs.rs ~1830-2150: update to new signatures (`&Gate::with_floor(20.0)` instead of `20.0`,
   UserActivePolicy::LastResort is gone, user-active is no longer "last" -> rewrite `user_active_policy`,
   `placement_ok_first_then_user_active_by_score`, `plan_placement_keeps_user_active_last...` to assert the tiebreak semantics;
   score asserts 46.2/26.2 are unchanged math). Placement literals need can_take/verdict.
2. FAILING-FIRST fixtures (owner's three live states) as lib tests + a control_service test:
   a) laptop 94% free, 128 GB, load 0.28/core, slot 0 held by an EXTERNAL flock (take HeavySlot::try_take_at(slot0) in the test) -> a
      heavy submit is ADMITTED on slot 1; second heavy goose job concurrently also admitted (max_heavy 2 via limits) -> third refused
      heavy_slot_held with "2/2 heavy slots held".
   b) user_active=true, 79% free -> admitted under default policy; refused only under Refuse (reason has numbers).
   c) overload: load 2.4/core -> heavy refused cpu_overloaded "load 2.40/core >= 2 backstop"; 12% free -> memory_headroom "12% free < 20% floor".
   d) verdict line OK shape "ok: 1/3 heavy, 94 GB free, load 0.28/core, can take ~N more 8 GB jobs".
   e) NodeLimits::from_json reads max_heavy/max_load_per_core/default_need_gb; default_max_heavy(16)=4? NO: 16/4=4; (8)=2; (28)=7.
3. tests/control_service.rs: line ~2061 `UserActivePolicy::LastResort` -> Refuse or Allow; ~2196-2211 user_active test now expects
   202 under default (and 409 user_active only with Refuse); heavy_slot_held tests (2206, 2676-2689, 2842) must hold ALL slots (set
   limits max_heavy 1 in that JobsConfig, or flock slot 0 and .1); `execute_409_when_node_busy_and_never_calls_executor` (~1362):
   live session must now give 202; add a 409 case via a sampler with mem_free_pct below the floor (check how spawn_node_full builds
   ctx.jobs / its sampler; the default sampler reads the REAL Mac, so inject one). Also tests/manager.rs 3184/3299/3416 compile check.
   queue.rs:365 test fine.
4. crates/goose/src/acp/server/link.rs: placement_json add "canTake", "verdict"; tests ~2597/2631 Placement literals need new fields.
   jobs_config() unchanged (policy parse handles last_resort).
5. UI ui/desktop/src: acp/leanzero-link.ts OwnLoad add heavy_slots_used, heavy_slots_max, can_take?, verdict?; placement type add
   canTake/verdict. components/leanzero-swarm/macLoad.ts: UserActivePolicy 'allow'|'refuse', USER_ACTIVE_POLICIES ['allow','refuse'],
   JOBS_DEFAULTS.policy 'allow', parsePolicy maps last_resort -> allow. MacLoadPanel.tsx: drop policy_last_resort (line ~119), show
   own.verdict line on each card (it carries the numbers + "can take ~N more"), JobsSettings description (line ~115 says "one heavy job
   at a time" -> slots/measured). Relabel: policy label "While someone uses this Mac" options "Take jobs" / "Refuse jobs"; add a hint
   key e.g. macLoad.policyHint "Busy means measured: memory above the floor, free heavy slots, load per core. Someone at the keyboard
   only breaks ties." Remove macLoad.policy.lastResort from all 16 locales (src/i18n/messages/*.json; en, de, es, fr, hi, id, it, ja,
   ko, ms, pt, ru, tr, vi, zh-CN, zh-TW) and add the new key translated in all 16. RunJobDialog shows candidates: show verdict text.
   Update macLoad.fixtures.ts / *.test.tsx (MacLoadPanel.test.tsx, RunJobDialog.test.tsx, macLoad.test.ts) for policy + verdict line.
6. Module doc at top of jobs.rs (lines 1-22) still says "ONE heavy slot" and "owner at the keyboard is the LAST resort": rewrite.
   load.rs OwnLoad heavy_slot_held doc done. NodeLimits doc comment "keys and defaults exactly as heavy-run (lines 27-37)" -> mention new keys.
7. Gates (hermit Node 24 for UI; builds via ~/Projects/machine-sync/bin/heavy-run, strip=false):
   cd worktree; source bin/activate-hermit; heavy-run cargo fmt --check; heavy-run cargo clippy -p leanzero-link -p goose --all-targets -- -D warnings;
   heavy-run cargo test -p leanzero-link; heavy-run cargo test -p goose --lib link; cd ui/desktop && pnpm lint:check && pnpm typecheck &&
   pnpm vitest run src/components/leanzero-swarm && pnpm i18n:check; check-acp-schema (just/ scripts: grep "acp-schema" in Justfile/package.json).

## Audit (item 5 of the brief) results
- FIXED: control.rs execute idle guard (flag: any live session); manager try_place allow_user_active flag.
- FINE (measured): swarm_router EntryFact::Busy = free slots 0 of capacity (swarm_router.rs:2838, 1361); nodes/resolve.rs queues, not skips;
  memfree.rs refuses only on its config switch; manager drops peers only Offline/leaving.
- Display only: RunJobDialog in-use label, MacLoadPanel "Someone is using it", useSwarmRun busyNodes labels.
- KEPT on purpose (physical/measurement, not load flags): benchmark running, lid closed+sleeps, heavy on battery, thermal >= 2. Same as node_fleet.py.

## Traps
- flock is per open file description: a probe from this process on a slot it holds reads "held" (used in submit: used-1).
- Old peers (3.0.130) only know slot 0 and refuse heavy when it is held; nothing to do on our side.
- A 3.0.130 placer cannot parse the new refusal code cpu_overloaded (it shows the raw text); acceptable.

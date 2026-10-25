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

## DONE since 26dcca562 (this agent, commit after 7c55a938f)
- Steps 1-3 + 6: lib tests rewritten to the new semantics; failing-first fixtures in jobs.rs tests
  (fixture_laptop_with_one_held_slot..., fixture_user_active_mac_at_79..., fixture_cpu_is_a_backstop...,
  fixture_limits_file_reads..., fixture_slot0_external_flock...). control_service.rs: spawn_node_full now injects a
  calm sampler + unique jobs root (JobRegistry::shared is keyed by root: the default root made every node share one
  sampler); execute 202 with a live session, 409 only at 12% free; test_limits max_heavy 2; refusal test fills BOTH
  slots; new two_heavy_goose_jobs_run_concurrently_next_to_an_external_slot0_flock (max_heavy 3, card can_take 11)
  and user_active_is_admitted_by_default_and_refused_only_under_refuse; heavy-run interop uses --max-heavy 2 and
  SKIPS a heavy-run without --max-heavy (machine-sync's new heavy-run is uncommitted there and flipped mid-run once).
  load_with_jobs now probes the REGISTRY's lock files with its slot count (card == what admission finds).
  `cargo test -p leanzero-link` green 3 runs in a row.
- Step 4: link.rs placement_json adds canTake + verdict; its two tests updated.
- Step 5 (partial): leanzero-link.ts OwnLoad heavy_slots_used/max, can_take, verdict; JobPlacement canTake/verdict.
  macLoad.ts policy 'allow'|'refuse', default allow, last_resort -> allow. MacLoadPanel: verdict line on each card
  (data-testid mac-load-verdict, warn tone when it starts "busy:"), policy_last_resort removed, policyHint added
  (jobs-settings-policy-hint), allowJobsHint en text "as many heavy jobs as this Mac has slots and memory for".
  16 locales: macLoad.policy.lastResort removed, macLoad.policyHint added (translated).

## TODO (next agent)
1. RunJobDialog.tsx: show each candidate's `verdict` (placements from jobsPlace) next to its Mac name.
2. allowJobsHint: en text changed; re-translate macLoad.allowJobsHint in the 15 other locales (they still say
   "one heavy job at a time").
3. UI tests: macLoad.test.ts (parsePolicy last_resort -> allow, USER_ACTIVE_POLICIES), MacLoadPanel.test.tsx
   (no jobs-policy-last_resort; verdict line renders; policy hint), RunJobDialog.test.tsx (verdict), macLoad.fixtures.ts
   (add verdict/can_take/heavy_slots_* to fixtures). grep -rn "last_resort\|lastResort" ui/desktop/src.
4. Gates (clippy for the 3 crates was started by this agent; its log is
   /private/tmp/claude-501/-Users-mihaiperdum-Projects/9235260e-4bbb-41c2-aefc-cd516bd51e28/scratchpad/clippy.log —
   re-run anyway after UI work is irrelevant to it):
   C='profile.dev.package."*".strip=false'  (laptop is macOS 27: needed on every cargo build/test; for clippy put
   --config AFTER `clippy`)
   source bin/activate-hermit; cargo fmt --all -- --check
   ~/Projects/machine-sync/bin/heavy-run --timeout 5400 -- cargo clippy --config "$C" -p leanzero-link -p goose -p goose-server --all-targets -- -D warnings
   heavy-run -- cargo --config "$C" test -p leanzero-link
   heavy-run -- cargo --config "$C" test -p goose --lib -- link link_serve swarm_router
   cd ui/desktop && pnpm lint:check && pnpm typecheck && pnpm vitest run && pnpm i18n:check; check-acp-schema
   (grep acp-schema in Justfile/package.json; OwnLoad gained can_take/verdict — regenerate if the schema carries it).
5. Commit (repo identity, NO AI credit lines), delete this HANDOFF.md in the final commit. Don't release.

## Audit (item 5 of the brief) results
- FIXED: control.rs execute idle guard (flag: any live session); manager try_place allow_user_active flag.
- FINE (measured): swarm_router EntryFact::Busy = free slots 0 of capacity (swarm_router.rs:2838, 1361); nodes/resolve.rs queues, not skips;
  memfree.rs refuses only on its config switch; manager drops peers only Offline/leaving.
- Display only: RunJobDialog in-use label, MacLoadPanel "Someone is using it", useSwarmRun busyNodes labels.
- KEPT on purpose (physical/measurement, not load flags): benchmark running, lid closed+sleeps, heavy on battery, thermal >= 2. Same as node_fleet.py.

## Traps
- A dropped flock can read 'held' for a moment: a child forked by another test shares the open file description
  until exec. Poll, don't assert once (fixture_slot0 does).
- flock is per open file description: a probe from this process on a slot it holds reads "held" (used in submit: used-1).
- Old peers (3.0.130) only know slot 0 and refuse heavy when it is held; nothing to do on our side.
- A 3.0.130 placer cannot parse the new refusal code cpu_overloaded (it shows the raw text); acceptable.

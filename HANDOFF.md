# HANDOFF: job placement strategies + origin queue (feat/job-strategies)

Worktree ~/Projects/goose-strategies (Gabee), branch feat/job-strategies (--no-track) from origin/main 7a351ec9c.
Owner spec: see the brief (local-first DEFAULT / selfish; LEANZERO_LINK_JOBS_STRATEGY overrides node-limits.json
"strategy"; UI segmented control in JobsSettings + per-job override in RunJobDialog; FIFO queue on the origin
instead of 409; re-place ~15 s; queued id/state/position/refusals; cancel while queued; survives reconnect).
NOT COMPILED YET. Nothing built. Commit is WIP.

## DONE (uncompiled)
- jobs.rs: STRATEGY_KEY, QUEUE_TICK (15 s); PlacementStrategy {LocalFirst (default), Selfish}, serde kebab-case
  ("local-first"/"selfish"), parse/as_str/ALL/from_limits(); PlacementPlan{candidates, refusals};
  plan_placement(self_id, rows, need, heavy, min_free, strategy): non-candidates -> NodeRefusal (JobsDisabled
  "does not take jobs", MemoryUnreadable "reports no load", or the verdict's busy refusal); sort = user_active
  class, then self first (local-first) / last (selfish), then score. StrategySource (fn, like JobsSwitch).
  JobsConfig += strategy (default from node-limits) + queue_tick. JobRequest += strategy: Option (per-job
  override). JobAccepted += queued: Option<u32> (position). JobView += placed_on: Option<PlacedOn{node_id,job_id}>.
  New QueueInfo{position, strategy, refusals}, NodeRefusal{node_id, code, reason}.
- load.rs: JobBrief += queue: Option<QueueInfo>; NodeLimits += strategy (reads "strategy" key, warns on unknown).
- queue.rs (new, `pub mod queue` in lib.rs): JobQueue (manager-owned): enqueue/snapshot/set_refusals/
  mark_placed(false = cancelled meanwhile -> caller cancels the placed job)/briefs/list/lookup -> QueueLookup
  {Waiting, Placed(on, stub), Cancelled}/cancel_waiting/drain_lock/ensure_ticker/retire_ticker_if_empty.
  3 unit tests inside. Uses Option::is_none_or (Rust >= 1.82; check toolchain).
- control.rs: ControlConfig.pending_jobs: Option<Arc<JobQueue>> (default None) -> Ctx; self_load prepends
  queue briefs (card JOBS list); jobs_list prepends queue.list(); jobs_get checks queue first; jobs_cancel
  cancels a waiting queue id first.

## TODO, in order
1. Fix JobRequest literals for the new `strategy` field: crates/leanzero-link/tests/manager.rs (2),
   jobs.rs tests, check crates/goose-sdk-types/src/custom_requests/schedule.rs (may be a different type).
   JobsConfig literals all use ..default (check tests/control_service.rs).
2. manager.rs (Core += `queue: Arc<JobQueue>`, built in with_mesh_factory; in connect at ~line 1752 set
   `control_config.pending_jobs = Some(self.queue.clone())`):
   - `place_job_with(need, heavy, strategy) -> PlacementPlan` (rows as place_job today, plan_placement);
     keep `place_job` = place_job_with(default strategy).candidates.
   - extract `submit_to(target, req)` (existing self/peer branch).
   - `try_place(&req, strategy) -> Result<(String, JobAccepted), Vec<NodeRefusal>>`: candidates in order,
     allow_user_active |= c.user_active, a LinkError::Job refusal or proxy error -> NodeRefusal, next one.
   - submit_job: targeted = unchanged (error, no queue). Untargeted: local_jobs()? first (NotConnected stays
     an error); strategy = req.strategy or core.config.control.jobs.strategy.get(); if queue non-empty ->
     enqueue at back (strict FIFO, refusals from plan); else try_place, Err -> enqueue; return
     (self_id, JobAccepted{job_id: qid, queued: Some(pos)}); ensure_queue_ticker().
   - `pub async fn drain_job_queue()`: hold drain_lock; for each snapshot entry: head-blocked -> only refresh
     refusals from plan; else try_place: Ok -> mark_placed (false -> cancel_job the placed one), Err ->
     set_refusals + head_blocked. Not connected -> refusal {self, Internal, "Link not connected"}.
   - ticker: Weak<Core> + seams.clone(); loop { sleep(queue_tick); upgrade or break; LinkManager{core,seams}
     .drain_job_queue(); if queue.retire_ticker_if_empty() break }. (No Drop on LinkManager; checked.)
   - list_jobs(self) = queue.list() + reg.list(); job_view(self, qid): Waiting/Cancelled -> view, Placed ->
     job_view(on.node, on.job) with placed_on set (stub on error); job_log(self, qid): waiting -> empty chunk
     done=false, cancelled -> done=true, placed -> redirect; cancel_job(self, qid): cancel_waiting, else
     Placed -> redirect, Cancelled -> JobQueue::cancel_refusal.
3. Tests: jobs.rs unit tests for plan_placement per strategy x {own-fits, own-full, all-full, own-locked
   (heavy + own heavy_slot_held)} incl. selfish with others full + own fits -> self. tests/manager.rs
   (copy the harness of `a_running_job_survives_disconnect_and_reconnect_and_stays_cancellable` ~3249, sampler
   behind an Arc<AtomicBool> for memory fits): FIFO with 3 (queued positions 1,2,3, placed in order after
   flip, queue_tick ~200 ms), cancel while queued, disconnect+connect keeps queue then drains. NodeLimits
   "strategy" parse test in load.rs.
4. ACP (crates/goose-sdk-types/src/custom_requests.rs ~5136/5227): SubmitRequest += strategy: Option<String>;
   SubmitResponse += queued: bool, queue_position: Option<u32>; PlaceRequest += strategy; PlaceResponse +=
   strategy: String, refusals: Vec<Value>. link.rs: jobs_config() sets `strategy: StrategySource::from_fn(||
   Config::global().get_param::<String>(STRATEGY_KEY).ok().and_then(parse).unwrap_or_else(from_limits))`;
   submit copies req.strategy (parse, bad -> invalid_params) into job.strategy; place uses place_job_with.
   Regenerate: `cargo run -p goose --bin generate-acp-schema` then `cd ui/desktop && pnpm run build-goose-sdk`.
   Add link.rs tests (acp::server::link).
5. UI: macLoad.ts JOBS_KEYS.strategy = 'LEANZERO_LINK_JOBS_STRATEGY', STRATEGIES, parseStrategy; liveJobs
   must EXCLUDE briefs with `queue` (waiting at origin, not running) + a "· N waiting" line;
   leanzero-link.ts types (JobBrief.queue, JobView.placed_on, JobPlacement refusals, submit strategy/queued);
   JobsSettings: Segmented "Placement" local-first/selfish (copy the policy control, saves at once);
   RunJobDialog: Segmented override (default = setting), pass strategy to jobsPlace + jobsSubmit, show
   refusals per Mac when nothing fits ("will wait in the queue"); queued job opens JobDialog on self/qid;
   JobDialog shows position + refusals + "Placed on X" when placed_on; job row on card shows "queued #n".
   vitest: setting, dialog override, queued row. i18n: all 16 locales (`pnpm i18n:check`).
6. Gates via heavy-run on Gabee, strip=false config (`cargo --config 'profile.dev.package."*".strip=false'`,
   AFTER the subcommand for clippy): fmt, clippy -D warnings leanzero-link + goose, `cargo test -p
   leanzero-link`, `cargo test -p goose acp::server::link`, ui lint:check, typecheck, vitest (hermit Node 24),
   i18n:check. Then rebase onto origin/main if 3.0.129 release commit landed (keep both). Commit with repo
   identity, NO AI credit lines (public repo). Don't release.

## Notes
- machine-sync node-run/node_fleet.py does NOT read "strategy" yet (grep 2026-10-25): goose uses the names the
  brief gave; tell the coordinator.
- Registry is process-shared per ledger root (D1 fix); queue must stay in Core, not control.

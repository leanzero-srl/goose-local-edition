# HANDOFF: job placement strategies + origin queue (feat/job-strategies)

Worktree ~/Projects/goose-strategies (Gabee), branch feat/job-strategies (--no-track) from origin/main 7a351ec9c.
Owner spec: see the brief (local-first DEFAULT / selfish; LEANZERO_LINK_JOBS_STRATEGY overrides node-limits.json
"strategy"; UI segmented control in JobsSettings + per-job override in RunJobDialog; FIFO queue on the origin
instead of 409; re-place ~15 s; queued id/state/position/refusals; cancel while queued; survives reconnect).
RUST SIDE DONE + GATED (2026-10-25). UI (step 5) is next.

## DONE, compiled and gated (Rust)
- jobs.rs: PlacementStrategy (local-first default / selfish), plan_placement, StrategySource, JobRequest.strategy,
  JobAccepted.queued, JobView.placed_on, QueueInfo, NodeRefusal. load.rs: NodeLimits.strategy. queue.rs: JobQueue.
- control.rs: pending_jobs shown in /nodes self load (prepended briefs), jobs list/get/cancel.
- manager.rs: Core.queue (survives reconnect; connect_inner sets control_config.pending_jobs). place_job_with ->
  PlacementPlan; place_job = default-strategy candidates. submit_job: targeted unchanged; untargeted = try each
  candidate (submit_to), else / if others wait -> enqueue (strict FIFO) -> (self_id, {job_id: qid, queued: pos});
  ticker every jobs.queue_tick (15 s) via Weak<Core>, retires when empty. drain_job_queue (pub): head blocked ->
  rest only refresh refusals; not connected -> refusal {self, internal, "Link not connected"}; a placement that
  lands after a cancel is cancelled. list_jobs/job_view/job_log/cancel_job on self follow queue ids (placed ->
  placed job's view with placed_on; waiting log = empty done:false; cancelled = done:true; 2nd cancel =
  already_finished).
- ACP: jobsSubmit += strategy (bad -> invalid_params), response += queued: bool, queuePosition?; jobsPlace +=
  strategy, response += strategy: string, refusals: [{nodeId, code, reason}] (camelCase, like placements).
  link.rs: configured_strategy() = LEANZERO_LINK_JOBS_STRATEGY -> node-limits.json -> local-first, used by
  jobs_config() StrategySource and jobsPlace. Schema + ui/sdk/src/generated regenerated (just generate-acp-types;
  needed `cd ui && pnpm install --frozen-lockfile` first on Gabee).
- Tests: jobs.rs plan_placement each strategy x own-fits/own-full/all-full/own-locked + selfish-others-full +
  user-active class + refusal codes + strategy serde; load.rs strategy key; tests/manager.rs mod job_queue
  (FIFO of 3 placed in order, cancel while queued, disconnect keeps queue then drains); link.rs ACP tests.
- Gates green: cargo fmt --check; clippy -p leanzero-link -p goose --all-targets -D warnings; cargo test -p
  leanzero-link (all suites); cargo test -p goose --lib acp::server::link (54).

## WIRE SHAPES the UI must read
- Queued job brief/view (node-native snake_case, inside job JSON): state "queued", queue: {position, strategy,
  refusals: [{node_id, code, reason}]}, placed_on: {node_id, job_id} once placed (view is then the placed job's,
  its brief.job_id = the placed id; keep following the QUEUE id on THIS node).
- jobsSubmit response {nodeId, jobId, queued, queuePosition?}; jobsPlace {placements, strategy, refusals}.

## NEXT: step 5 UI (ui/desktop), then its gates
5. UI: macLoad.ts JOBS_KEYS.strategy = 'LEANZERO_LINK_JOBS_STRATEGY', STRATEGIES, parseStrategy; liveJobs
   must EXCLUDE briefs with `queue` (waiting at origin, not running) + a "· N waiting" line;
   leanzero-link.ts types (JobBrief.queue, JobView.placed_on, JobPlacement refusals, submit strategy/queued);
   JobsSettings: Segmented "Placement" local-first/selfish (copy the policy control, saves at once);
   RunJobDialog: Segmented override (default = setting), pass strategy to jobsPlace + jobsSubmit, show
   refusals per Mac when nothing fits ("will wait in the queue"); queued job opens JobDialog on self/qid;
   JobDialog shows position + refusals + "Placed on X" when placed_on; job row on card shows "queued #n".
   vitest: setting, dialog override, queued row. i18n: all 16 locales (`pnpm i18n:check`).
Gates (hermit Node 24: `source bin/activate-hermit`): cd ui/desktop && pnpm run lint:check, pnpm run typecheck,
vitest, pnpm i18n:check. Then rebase onto origin/main if the 3.0.129 release commit landed (keep both). Commit
with repo identity, NO AI credit lines (public repo). Don't release.
Builds: /private/tmp/claude-501/hr.sh pattern = source bin/activate-hermit + ~/Projects/machine-sync/bin/heavy-run
(bare zsh has NO cargo on Gabee; hermit provides it). strip=false: `--config 'profile.dev.package."*".strip=false'`.

## Notes
- machine-sync node-run/node_fleet.py does NOT read "strategy" yet (grep 2026-10-25): goose uses the names the
  brief gave; tell the coordinator.
- Registry is process-shared per ledger root (D1 fix); queue must stay in Core, not control.

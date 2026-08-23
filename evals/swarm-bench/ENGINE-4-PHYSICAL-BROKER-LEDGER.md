# Engine 4 — physical admission broker ledger

Branch: `codex/swarm-physical-broker`  
Base: `10ab496465a7b9cdff94298ec64cde8e8f87c03e`
Sealed-repair integration authority: `1af43ef0264eefbc1ec28bf30cbbc662e7e397ce`

This ledger is the compaction-safe authority for Engine 4. It records the implementation
contract and the evidence used to accept or reject each increment. It does not authorize a
benchmark fleet, LM Studio mutation, scorer change, or SB7 change.

## Evidence from the pre-edit audit

- `DeviceCfg.in_flight` is a logical-lane counter. `SwarmDevice.host` and the `lms ps`
  `PARALLEL` column are already parsed in goose-cli but are discarded before the scheduler.
  Two different loaded model identifiers on one Mac are therefore treated as independent
  machines.
- Build, judge, pre-review, operator Q&A, sink review, tail review, test generation,
  speculation, and dynamic replan make independent admission decisions. There is no common
  priority or freshness check.
- A task future returning or being aborted immediately releases its logical slot. Neither
  event proves that the provider has ended the correlated decode. A replacement can therefore
  be admitted into phantom-free capacity.
- The judge has a semantic model path worth preserving, but its scheduler integration can
  abort an admitted worker, accept it, split its task, or re-dispatch it. Those actions are not
  safe until provider-terminal correlation and contract-complete split children exist.
- Tail and sink dimension reviews are selected from global tree state. They are not bound to
  an immutable source task/version, so they cannot be admitted by a freshness-preserving
  broker.
- Historical F623 and r1/r2 evidence reject both `configured lane is a physical node` and
  `idle means run a judge`. Useful review work and verified physical capacity must both exist.

## Frozen Engine 4 invariants

1. The broker identifies a physical host separately from a logical device and a loaded model
   instance. Host capacity is not the sum of aliases or model rows. Unverified identity or
   capacity cannot be used to claim physical idleness.
2. All model requests admitted by the Engine 4 path enter one priority queue. Critical DAG
   work and task/version-bound auxiliary evidence compete in that queue; node count never
   creates or changes a task, split, dependency, contract, or acceptance requirement.
3. A queued auxiliary opportunity names its source task, source attempt/version, and evidence
   purpose. It is discarded if that version is no longer current. Global `the tree is idle`
   work is not admissible.
4. A newly ready critical task outranks queued auxiliary work. Admission is the preemption
   boundary: an admitted request is never killed merely to make room for another request.
5. Occupancy is released only by a terminal receipt whose request id and physical host match
   the admission receipt. Local future completion, local stream drop, cancellation request, or
   an unrelated terminal receipt does not free capacity.
6. A request that completed locally but lacks provider-terminal evidence remains a physical
   claim and blocks replacement admission. The run reports the unresolved claim instead of
   guessing that the provider is idle.
7. There is no permanent judge slot and no `idle => judge` rule. Semantic judge inference is
   preserved. In the broker path its result is observation-only: no abort, deterministic
   semantic verdict, split, accept, or re-dispatch side effect.
8. Blind sink/tail review, speculation, and capacity-triggered replan are not silently adapted.
   They are rejected in the Engine 4 path until they can supply immutable task/version authority
   and the same terminal lifecycle contract.
9. Every queue, admission, local completion, stale rejection, terminal mismatch, and verified
   terminal transition emits an exact correlated event containing the work id, request id,
   logical device, physical host, model instance, work role, and source version where applicable.
10. Engine 4 is opt-in while provider-terminal telemetry is incomplete. Enabling it must not
    alter the advertised DAG; its replay tests compare the exact input DAG with dispatched task
    ids.

## Intended increments

- [x] Pure broker state machine and replay tests for physical-host capacity, exact terminal
  correlation, priority, and freshness.
- [x] Carry same-run, unambiguous `lms ps` identity/capacity through goose-cli as a separate
  `PhysicalFleetSnapshot` and expose it in `pool_resolved`. `DeviceCfg` remains a logical lane;
  putting physical truth in it would recreate the alias/capacity bug.
- [x] Add request/terminal lifecycle receipts to a separate dispatcher boundary without pretending an
  ordinary future return is provider-terminal.
- [x] Integrate build plus task-derived auxiliary opportunities through one admission seam;
  make unsafe legacy auxiliary/intervention paths fail closed while Engine 4 is active.
- [x] Scheduler replays for one logical task/one host, two lanes/one host, stale auxiliary,
  critical-ready priority, terminal-not-yet-observed occupancy, and DAG identity.
- [x] Format, targeted crate tests, and strict goose-swarm clippy with the shared target directory.
- [x] Integrate on the sealed-repair head, then rerun the broker replays, repair-tree seal tests,
  goose-cli tests, and strict clippy.

## Gate log

- Pure broker increment: `cargo test -p goose-swarm --test physical_broker_replay` (10/10),
  `cargo test -p goose-swarm --lib` (65/65), and
  `cargo clippy -p goose-swarm --all-targets -- -D warnings` pass with the shared target.
- CLI physical-snapshot increment: the real `lms ps` fixture and the duplicate-identifier-on-two-
  hosts rejection tests pass. `cargo clippy -p goose-cli --all-targets -- -D warnings` passes with
  the shared target. The ordinary dispatcher still exposes no provider request/terminal receipts,
  so requesting enforcement fails closed after emitting the snapshot status; shadow observation is
  the default.
- Lifecycle/control-plane increment: `cargo test -p goose-swarm` passes 65 unit, 6 historical judge
  replay, 18 broker replay, 15 physical control-plane replay, and 39 scheduler-mock tests.
  `cargo clippy -p goose-swarm --all-targets -- -D warnings` passes with the shared target. The
  scheduled replay compares the exact input DAG ids, provider-dispatch calls, and completed report;
  all three sets are identical.
- Sealed-repair integration: the exact `1af43ef02` head merged as `c28b17a35` without source
  conflict. `OpenRepairTree`, `SealedCompleteTree`, the canonical ruler, and the archived r1
  post-gate mutation fixture remain present. The six `repair_tree_seal_tests` pass. Full
  `cargo test -p goose-cli` passes 593 tests with one intentional ignore, and full
  `cargo test -p goose-swarm` retains the counts above. Combined
  `cargo clippy -p goose-swarm -p goose-cli --all-targets -- -D warnings` passes. The full CLI
  gate exposed and then closed one integration defect: the default-off
  `GOOSE_SWARM_PHYSICAL_BROKER` reader now has an exact environment-only `retain_disabled`
  registry row instead of bypassing the machine-auditable control catalog.

Red-team refinement: `PARALLEL` is represented only as a model-instance ceiling. Same-run
`lms ps` evidence starts at one host-wide admission; a higher host capacity requires an exact
measured profile. A task admission is a durable correlation envelope, not a decoder claim. Its
initial provider-turn permit is reserved at admission; an exact terminal receipt releases that
permit immediately while the task does local tool work. Every later provider turn re-enters the
same ranked admission queue. The envelope becomes releasable only after local completion, provider
starts are closed, and every started turn has an exact terminal receipt (or an explicit
provider-not-started receipt). Terminal failure/cancellation overrides a contradictory local
success. The broker exposes host occupancy as exact reserved/live provider-turn permits, separately
from queued work and active task envelopes.

Source authority is monotonic compare-and-set: rollback, conflicting equal revisions, stale
removal, generic auxiliary prose, role/priority laundering, unknown routes, and route sets that
exclude the whole verified snapshot fail closed. Host aliases must share the full capacity evidence,
not only its numeric value. Cancelled admission and provider-permit futures withdraw queued work;
an already granted but unconsumed permit may be revoked, but consumed/admitted work has no kill API.
Closing provider starts atomically rejects even a cloned provider call already waiting in the queue,
which prevents a late call from outliving the dispatcher boundary.

Every later-turn queue transition carries its own queue-sequence receipt before the permit event;
equal-rank reacquisition uses the source task's work id rather than the synthetic admission id for
the DAG's deterministic id tie-break. Capacity updates create a new fleet-snapshot id, so two
different capacity truths never share one snapshot identity. Active admissions retain the exact
snapshot and capacity evidence under which they were routed; later admissions cite the new one.
Capacity changes are compare-and-set against the caller's current fleet-snapshot id, so an older
asynchronous measurement cannot overwrite newer evidence. Aliases of one physical instance must
also share exact route evidence.

LM Link routes by model identifier, not by the display host. If one identifier is reported on two
hosts, goose-cli continues its legacy one-logical-worker reconciliation but does not certify either
physical route. Persisted `host` configuration and non-LM-Studio providers are likewise never
promoted to live physical evidence. A run may observe a complete same-run snapshot or an explicit
unavailable event; it never fills missing physical identity from a logical lane.

The lifecycle seam is intentionally a new `ProviderLifecycleDispatcher`, not a default method on
`TaskDispatcher`. The broker adaptively selects among verified physical routes; the scheduler's
placeholder lane does not become a dispatch, timing, or utilization fact. Ready work is queued in
the DAG's fan-out/id order before Tokio can reorder futures, and physical capacity changes admission
timing without changing task creation, dependencies, files, contracts, or the advertised DAG.

The brokered scheduler rejects the old judge, pre-review/QA/tail/testgen, idle-capacity replan,
speculative twin, runtime-review-as-build, and reserved-supervision paths rather than letting any of
them bypass the queue. Runtime review remains task-derived: physical capacity or observed idleness
never creates review work. This does not remove or alter the existing semantic judge; the ordinary
scheduler path remains on its existing behavior. A physical semantic judge is Engine 5 work and
must enter as a trace-versioned, lifecycle-capable, observation-only opportunity; there is no
deterministic verdict or blind `idle => judge` substitute in Engine 4.

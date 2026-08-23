# Engine 4 — physical admission broker ledger

Branch: `codex/swarm-physical-broker`  
Base: `10ab496465a7b9cdff94298ec64cde8e8f87c03e`

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

- [ ] Pure broker state machine and replay tests for physical-host capacity, exact terminal
  correlation, priority, and freshness.
- [ ] Carry verified `lms ps` identity/capacity through goose-cli into `DeviceCfg` and expose it
  in `pool_resolved`.
- [ ] Add request/terminal lifecycle receipts to the dispatcher boundary without pretending an
  ordinary future return is provider-terminal.
- [ ] Integrate build plus task-derived auxiliary opportunities through one admission seam;
  make unsafe legacy auxiliary/intervention paths fail closed while Engine 4 is active.
- [ ] Scheduler replays for one logical task/one host, two lanes/one host, stale auxiliary,
  critical-ready priority, terminal-not-yet-observed occupancy, and DAG identity.
- [ ] Format, targeted crate tests, goose-cli tests, and clippy with the shared target directory.

## Gate log

No implementation gate has run yet.

# Engine 4 semantic observation ledger

Date: 2026-08-23  
Implementation base: `codex/swarm-engine-overhaul@1af43ef02`  
Implementation branch: `codex/swarm-semantic-judge-observer`

## Boundary

This slice adds the observation-only semantic control-plane foundation. It does not change SB7, its
scorer, LM Studio, a benchmark, the shared project tree, or scheduler behavior. It launches no model.

The standing semantic judge is preserved. Measurements remain neutral inputs: recurrence, elapsed
time, counter flatness, stream activity, artifact state, transport state, and occupancy cannot create
an action. Only a response parsed through the strict semantic protocol can contain an action, and an
Engine 4 receipt has `has_intervention_authority() == false` for every action, including `NUDGE`,
`SPLIT_PROPOSAL`, and `ACCEPT_CANDIDATE`.

The pre-Engine-4 `parse_judge_reply` and `JudgeOutcome` call sites still exist on this pre-broker base.
They are legacy intervention machinery, not callers of the new plane. They must not be wired beside
the physical broker. Replacing that live path is an integration operation against the broker branch,
whose physical scheduler rejects legacy judge work; silently running both would double-spend review
capacity and leave the old keyword parser authoritative. The new protocol is the replacement API, not
a compatibility wrapper around the old parser.

## Evidence corrections carried into the corpus

- F924 is represented by the preserved `f924-detail-tail-shape.json`: 27 detail calls, 26 complete
  before the logical tail, 203,447 reasoning characters at interruption, and no proven physical-idle
  interval. The corrected repeated-window share is 0.4033. The withdrawn 0.6758 value is explicitly a
  negative assertion. Recurrence is a cited summons signal; it does not vote for `NUDGE`.
- F163 is represented by the archived 105s/174s/234s flat-counter sequence that later wrote at 294s.
  Provider/stream activity can advance while reasoning characters and completed tool-call counters are
  flat, so the expected semantic action is `CONTINUE`.
- The historical r1 parser false positive is preserved as raw legacy text:
  `VERDICT|OK|HIGH|The worker is healthy; there is no looping or drift.` The strict parser abstains;
  the words `looping` and `drift` in a negative rationale cannot override an action field.
- The useful Fable correction remains a positive case: `verify-e2e::1` ran the summary command while
  its frozen shard owned the all-pages and expired-cursor cases. The expected action is a specific
  `NUDGE`, but the receipt remains observation-only.

## Implemented protocol and snapshot rules

Commits `e2855e6ef`, `fafe85c66`, and `94f6ec36f` add:

1. Exact actions `CONTINUE|NUDGE|SPLIT_PROPOSAL|ROUTE_FINDING|ACCEPT_CANDIDATE|REQUEST_EVIDENCE|ABSTAIN|INCOMPLETE`
   in a strict JSON envelope. Unknown actions, unknown fields, prose, Markdown fences, missing fields,
   malformed JSON, a wrong protocol, and a wrong snapshot hash become `ABSTAIN`.
2. Action-specific payloads. A nudge requires guidance; a route must name a route sealed into the
   snapshot; acceptance must cover exactly the sealed acceptance oracle; incomplete must name known
   unmet requirements; evidence requests cannot be empty; and split observations require at least two
   described boundaries with known requirement/evidence IDs. These checks validate observations only.
   A split boundary is deliberately not a `TaskSpec` and cannot enter a DAG.
3. Evidence citations that resolve only to IDs sealed into the same snapshot. Rationale text cannot
   smuggle in an unknown artifact, requirement, trace, or signal.
4. A canonical SHA-256 snapshot over task/attempt/source revision, contract and artifact versions,
   acceptance oracle, dependency and sibling versions, allowed late-finding routes, artifact excerpts,
   recent trace, previous intervention/response, and neutral signals. Semantically unordered fields are
   sorted, including nested JSON signal objects, so feature-dependent map insertion order cannot change
   identity.
5. Monotonic per-task authority. An older attempt/revision cannot replace a current snapshot; two
   different hashes claiming one version are rejected; a result that returns after a newer snapshot is
   registered is recorded as stale `ABSTAIN`.
6. Asynchronous one-flight observation. Submission returns before the reviewer completes. Identical
   in-flight and completed snapshots are deduplicated, and a different current snapshot cannot start a
   second review while the first owns the task's review lane. Reviewer errors and panics become
   `ABSTAIN` and release the lane.
7. Structured `semantic_observation_requested`, `semantic_observation_deduplicated`,
   `semantic_observation_rejected`, and `semantic_observation_completed` events. They carry snapshot
   identity and `authority: observation_only`, never raw model output.

No duration, token, character, recurrence, review-count, or retry cap was added.

## Frozen corpus and what it proves

`crates/goose-swarm/tests/fixtures/semantic-judge-observation-corpus.json` carries nine cases:

1. corrected F924 long-period recurrence;
2. slow healthy reasoning that advances a falsifiable hypothesis;
3. flat coarse counters while a tool payload streams;
4. advancing but wrong interface work;
5. genuinely complete work with exact oracle coverage;
6. uncertainty caused by missing artifact evidence;
7. the historical Fable task-position correction;
8. the F163 flat-counter negative; and
9. the r1 negative-keyword parser false positive.

The corpus proves parser, grounding, version, deduplication, and lifecycle behavior. Its expected
semantic actions are frozen labels derived from already-reviewed evidence; they do not prove a local
model will produce those actions. No precision, recall, speed, node-value, or quality claim is made
from these offline tests.

Adversarial mutations cover a legacy pipe reply, Markdown-wrapped JSON, unknown `KILL`, stale snapshot,
unknown field, invented evidence ID, empty request, incomplete split payload, response rationale that
contains action keywords, object-order drift, same-snapshot duplication, source supersession, and a
panicking reviewer.

## Verification

Run from this branch:

```text
cargo test -p goose-swarm semantic_observation --lib
cargo test -p goose-swarm --test semantic_observation_corpus
cargo test -p goose-swarm
cargo clippy -p goose-swarm --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The first focused pass completed 9/9 unit tests and 3/3 corpus tests. The final full-crate and strict
lint results are recorded in the closing commit once rerun on the complete branch.

## Broker integration still required before any live observation

This branch intentionally does not acquire a fleet slot. A production adapter must be rebased onto the
physical broker and submit each snapshot as typed `SemanticJudgeObservation` work. The broker—not the
observer—owns role priority, physical eligibility, request admission, provider-start correlation,
provider-terminal proof, and local completion. A broker rejection or source supersession must prevent
the provider call. No caller may invoke this plane from the old inline stream loop or legacy idle-judge
queue.

The reviewed broker head is `c60b309b1`. The exact mapping is:

- snapshot task/attempt/revision/hash -> `TaskVersion { kind: SourceRevisionKind::Trace {
  trace_sequence, snapshot_hash } }`;
- role -> `WorkRole::SemanticJudgeObservation`, with the role-derived
  `WorkPriority::AuxiliaryEvidence` rather than a caller-authored priority;
- source publication -> `PhysicalAdmissionControl::set_source_revision` before queue/admission;
- one admitted physical reviewer -> `AdmittedWork`, passed into the per-submission
  `SemanticObservationReviewer` adapter;
- provider start/terminal -> the admitted lifecycle's correlated request methods; and
- parsed/stale/error receipt -> `AdmittedWork::complete_local` after the observation handle resolves.

The observer accepts its reviewer per submission rather than storing one global reviewer. That is
load-bearing: it lets the adapter bind one exact broker admission, physical route, and provider lifecycle
to one snapshot. A global reviewer would make it possible to use the right semantic prompt on the wrong
physical admission.

`git merge-tree --write-tree c60b309b1 94f6ec36f` produced a conflict-free combined tree
(`7f6bf15f936403f1ce547783d3e03c0e8d397531`). This is a structural merge check only; it is not a compiled
integration or permission to launch a provider call.

Until that adapter and its provider lifecycle tests land, the new plane stays unwired. That is a
correctness boundary, not a feature flag or a request cap.

## Exact Engine 5 gap

Engine 5 begins only after the observation adapter is broker-admitted and measured. It must add all of
the following without weakening this boundary:

1. `NUDGE`: deliver ordinary guidance only at a natural tool/turn boundary. A high-confidence interrupt
   needs cooperative cancellation, a correlated provider-terminal receipt, valid partial-session commit,
   and same-session continuation. The replacement request cannot overlap the old provider request.
2. Neutral recurrence: build a correctly named edge-triggered/debounced summons from positive and
   slow-healthy distributions, with one outstanding observation per state. It cannot corroborate a model
   action, kill, accept, nudge, or start a replacement request.
3. `SPLIT_PROPOSAL`: compile every child through the ordinary task binder with requirement, evidence,
   interface, ownership, dependency, and acceptance closure. The descriptive boundaries in this protocol
   grant no DAG mutation authority.
4. `ROUTE_FINDING`: revalidate the late finding against the current snapshot and route it through exact
   current ownership. A route sealed into an old snapshot is not enough.
5. `ACCEPT_CANDIDATE`: run the objective oracle within its actual scope and prove the current artifact
   still matches the reviewed hash. File existence, silence, elapsed time, reasoning shape, a model
   confidence value, or one narrow passing check cannot accept arbitrary software.
6. `REQUEST_EVIDENCE` / `INCOMPLETE`: acquire evidence as separately admitted, contract-derived work;
   capacity cannot invent the question or turn missing evidence into success.
7. Causal evaluation: compare observation-only with each intervention on matched cases, recording the
   delivered guidance, next worker action, artifact/evidence delta, semantic requirement closure, objective
   oracle result, provider overlap, and decoder-minutes. Repeated `CONTINUE` and raw finding count earn no
   value by themselves.

No Engine 5 action is implemented or implied by this branch.
